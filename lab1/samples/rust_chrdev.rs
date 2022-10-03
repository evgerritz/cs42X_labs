// SPDX-License-Identifier: GPL-2.0

//! Rust character device sample.

use kernel::prelude::*;
use kernel::{chrdev, file};

module! {
    type: RustChrdev,
    name: "mymem",
    author: "Evan Gerritz",
    description: "mymem char device driver in Rust",
    license: "GPL",
}

struct RustFile;

#[vtable]
impl file::Operations for RustFile {
    fn open(_shared: &(), _file: &file::File) -> Result {
        Ok(())
    }
}

struct RustMymem {
    _dev: Pin<Box<chrdev::Registration<2>>>,
}

impl kernel::Module for RustMymem {
    fn init(name: &'static CStr, module: &'static ThisModule) -> Result<Self> {
        pr_info!("mymem (init)\n");

        let mut chrdev_reg = chrdev::Registration::new_pinned(name, 0, module)?;

        // Register the same kind of device twice, we're just demonstrating
        // that you can use multiple minors. There are two minors in this case
        // because its type is `chrdev::Registration<2>`
        chrdev_reg.as_mut().register::<RustFile>()?;
        chrdev_reg.as_mut().register::<RustFile>()?;

        Ok(RustChrdev { _dev: chrdev_reg })
    }
}

impl Drop for RustChrdev {
    fn drop(&mut self) {
        pr_info!("Rust character device sample (exit)\n");
    }
}
