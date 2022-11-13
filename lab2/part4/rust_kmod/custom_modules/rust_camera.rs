use kernel::prelude::*;
use kernel::{
    delay::coarse_sleep,
    c_str,
    file::{self, File},
    io_buffer::{IoBufferReader, IoBufferWriter},
    miscdev,
    task::Task,
    sync::{smutex::Mutex, Ref, RefBorrow},
    net::{Ipv4Addr, init_ns, TcpStream},
};
use kernel::bindings;
use core::mem;
use core::ptr;
use core::default::Default;
use core::time::Duration;

// constants obtained by printing out values in C
const VIDIOC_STREAMON: u32 = 1074026002;
const VIDIOC_STREAMOFF: u32 = 1074026003;
const VIDIOC_QBUF: u32 = 3227014671; 
const VIDIOC_DQBUF: u32 = 3227014673;

module! {
    type: RustCamera,
    name: "rust_camera",
    author: "Evan Gerritz",
    description: "A simple module that reads camera input.",
    license: "GPL",
}

const OUT_BUF_SIZE: usize = 17*3;

struct RustCamera {
    _dev: Pin<Box<miscdev::Registration<RustCamera>>>,
}

struct Device {
    output: Mutex<[u8; OUT_BUF_SIZE]>,
}

struct kernel_msg {
    start_pfn: u64,
    num_pfns: u64,
    my_type: u64,
    buffer: u64
}

#[repr(C)]
struct timeval {
    tv_sec: u64,
    tv_usec: u64
}

#[repr(C)]
struct v4l2_timecode {
    my_type: u32,
    flags: u32,
    frames: u8,
    seconds: u8,
    minutes: u8,
    hours: u8,
    userbits: [u8; 4],
}

#[repr(C)]
pub struct v4l2_buffer {
    index: u32,
    my_type: u32,
    bytesused: u32,
    flags: u32,
    field: u32,
    align: u32,
    timestamp: timeval,
    timecode: v4l2_timecode,
    sequence: u32,
    memory: u32,
    offset: u32,
    offset2: u32,
    length: u32,
    reserved1: u32,
    reserved2: [u32; 2],
}

const PAGE_SHIFT: u64 = 12;

fn pfn_to_kaddr(pfn: u64) -> u64{
    unsafe { (pfn << 12) + bindings::page_offset_base }
}


impl kernel::Module for RustCamera {
    fn init(name: &'static CStr, _module: &'static ThisModule) -> Result<Self> {
        pr_info!("RustCamera (init)\n");

        // make RustCamera a miscdev as you have done in A1P4
        let state = Ref::try_new( Device {
            output: Mutex::new([0u8; OUT_BUF_SIZE]),
        })?;

        Ok(RustCamera {                  // 438 == 0o666
            _dev: miscdev::Options::new().mode(438).register_new(fmt!("{name}"), state)?,
        })
    }
}

impl Drop for RustCamera {
    fn drop(&mut self) {
        pr_info!("RustCamera (exit)\n");
    }
}

#[vtable]
impl file::Operations for RustCamera {
    type OpenData = Ref<Device>;
    type Data = Ref<Device>;

    fn open(shared: &Ref<Device>, _file: &File) -> Result<Self::Data> {
        pr_info!("rust_camera (open)\n");
        Ok(shared.clone())
    }

    fn read( shared: RefBorrow<'_, Device>, _file: &File,
        data: &mut impl IoBufferWriter, offset: u64 ) -> Result<usize> {
        if data.is_empty() {
            return Ok(0);
        }

        let mut buffer = shared.output.lock();

        let num_bytes: usize = data.len();

        let new_len = num_bytes;
        if new_len > OUT_BUF_SIZE {
            return Err(EINVAL);
        }

        data.write_slice(&mut buffer[..num_bytes])?;
        Ok(num_bytes)
            
    }

    fn write( shared: RefBorrow<'_, Device>, _: &File,
        data: &mut impl IoBufferReader, offset: u64) -> Result<usize> {
        // get userspace data
        pr_info!("RustCamera (write)\n");
        let mut msg_bytes = [0u8; 32];
        data.read_slice(&mut msg_bytes).expect("couldn't read data");
        let msg: kernel_msg = unsafe { mem::transmute::<[u8; 32], kernel_msg>(msg_bytes) };

        pr_info!("151\n");
        Task::spawn(fmt!(""), move || {
            let fname = c_str!("/dev/video2");
            let mut camera_filp = unsafe { bindings::filp_open(fname.as_ptr() as *const i8, bindings::O_RDWR as i32, 0) };
            
            let mut socket = ptr::null_mut();
            let ret = unsafe {
                bindings::sock_create(
                //bindings::sock_create_kern
                    //init_ns().0.get(),
                    bindings::PF_INET as _,
                    bindings::sock_type_SOCK_STREAM as _,
                    bindings::IPPROTO_TCP as _,
                    &mut socket,
                )
            };
            pr_info!("167\n");
            let mut saddr: bindings::sockaddr_in = Default::default();
            saddr.sin_family = bindings::PF_INET as u16;
            saddr.sin_port = 0x401f; // 8000 -> 0x1f40 -> 0x401f
            saddr.sin_addr.s_addr = 0x1000007f; // 127.0.0.1 -> 0x7f000001 -> big endian

            pr_info!("173\n");
            let mut saddr: bindings::sockaddr = unsafe { mem::transmute::<bindings::sockaddr_in, bindings::sockaddr>(saddr) };
            pr_info!("175\n");
            unsafe {
                (*(*socket).ops).connect.expect("no connect fn")(
                    socket,
                    &mut saddr as *mut _,
                    mem::size_of::<bindings::sockaddr_in>().try_into().unwrap(),
                    bindings::O_RDWR.try_into().unwrap()
            )};
            pr_info!("180\n");

            let stream = TcpStream { sock: socket };

            pr_info!("186\n");

            pr_info!("{:?} {:?} {:?}\n", camera_filp, msg.buffer, msg.my_type);
            queue_buffer(camera_filp, msg.buffer);
            start_streaming(camera_filp, msg.my_type);
            for _ in 1..100 {
                queue_buffer(camera_filp, msg.buffer);
                coarse_sleep(Duration::from_millis(25));
                //stream.write(&[69u8; 10], true);
                dequeue_buffer(camera_filp, msg.buffer);
            }
            stop_streaming(camera_filp, msg.my_type);
        }).unwrap();
        Ok(0)
    }
}

fn start_streaming(camera_f: *mut bindings::file, my_type: u64) {
    // Activate streaming
    if unsafe { bindings::vfs_ioctl(camera_f, VIDIOC_STREAMON, my_type) } < 0 {
        pr_info!("streamon failed!\n");
    }
}

fn stop_streaming(camera_f: *mut bindings::file, my_type: u64) {
    if unsafe { bindings::vfs_ioctl(camera_f, VIDIOC_STREAMOFF, my_type) } < 0 {
        pr_info!("streamoff failed!\n");
    }
}

fn queue_buffer(camera_f: *mut bindings::file, buffer: u64) {
    if unsafe { bindings::vfs_ioctl(camera_f, VIDIOC_QBUF, buffer) } < 0 {
        pr_info!("qbuf failed!\n");
    }
}

fn dequeue_buffer(camera_f: *mut bindings::file, buffer: u64) {
    if unsafe { bindings::vfs_ioctl(camera_f, VIDIOC_DQBUF, buffer) } < 0 {
        pr_info!("dqbuf failed!\n");
    }
}
