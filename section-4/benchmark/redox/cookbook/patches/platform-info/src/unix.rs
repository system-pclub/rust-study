// This file is part of the uutils coreutils package.
//
// (c) Jian Zeng <anonymousknight96 AT gmail.com>
// (c) Alex Lyon <arcterus@mail.com>
//
// For the full copyright and license information, please view the LICENSE file
// that was distributed with this source code.
//

extern crate libc;

use self::libc::{uname, utsname};
use super::Uname;
use std::mem;
use std::ffi::CStr;
use std::borrow::Cow;
use std::io;

macro_rules! cstr2cow {
    ($v:expr) => (
        unsafe { CStr::from_ptr($v.as_ref().as_ptr()).to_string_lossy() }
    )
}

pub struct PlatformInfo {
    inner: utsname,
}

impl PlatformInfo {
    pub fn new() -> io::Result<Self> {
        unsafe {
            let mut uts: utsname = mem::uninitialized();
            if uname(&mut uts) == 0 {
                Ok(Self { inner: uts })
            } else {
                Err(io::Error::last_os_error())
            }
        }
    }
}

impl Uname for PlatformInfo {
    fn sysname(&self) -> Cow<str> {
        cstr2cow!(self.inner.sysname)
    }

    fn nodename(&self) -> Cow<str> {
        cstr2cow!(self.inner.nodename)
    }

    fn release(&self) -> Cow<str> {
        cstr2cow!(self.inner.release)
    }

    fn version(&self) -> Cow<str> {
        cstr2cow!(self.inner.version)
    }

    fn machine(&self) -> Cow<str> {
        cstr2cow!(self.inner.machine)
    }
}
