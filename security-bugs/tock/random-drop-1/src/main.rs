#![feature(ptr_internals)]
extern crate libc;
use core::ptr::Unique;
use core::ops::{Deref, DerefMut};
use std::mem;
use std::ptr;

type AppId = u32;

pub struct Owned<T: ?Sized> {
    data: Unique<T>,
    appid: AppId,
}

impl<T: ?Sized> Owned<T> {
    unsafe fn new(data: *mut T, appid: AppId) -> Owned<T> {
        Owned {
            data: Unique::new_unchecked(data),
            appid: appid,
        }
    }
    pub fn appid(&self) -> AppId {
        self.appid
    }
}

impl<T: ?Sized> Deref for Owned<T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { self.data.as_ref() }
    }
}
impl<T: ?Sized> DerefMut for Owned<T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { self.data.as_mut() }
    }
}

/// The data for testing.
#[derive(Debug)]
struct Printer(Vec<i32>);

impl Drop for Printer {
    fn drop(&mut self) {
        println!("Dropping vec: addr: {:?}, len: {}", self.0.as_ptr(), self.0.len());
    }
}

fn alloc_buggy() {
    unsafe {
        // Allocate an uninitialized buffer, because this address have never written
        // need to use write `1` to simulate it is uninitialized memory, otherwise a
        // zero page will be read, this is different from kernel memory allocator.
        let arr : *mut u8 = libc::malloc(mem::size_of::<Printer>()) as *mut u8;
        ptr::write_bytes(arr, 1, mem::size_of::<Printer>());

        let data = Printer(vec![1, 2, 3]);

        let mut owned = Owned::new(arr as *mut Printer, 0);

        // use deference to trigger drop random data
        *owned = data;
    }
}

fn alloc_patch() {
    unsafe {
        // Allocate an uninitialized buffer, because this address have never written
        // need to use write `1` to simulate it is uninitialized memory, otherwise a
        // zero page will be read, this is different from kernel memory allocator.
        let arr : *mut u8 = libc::malloc(mem::size_of::<Printer>()) as *mut u8;
        ptr::write_bytes(arr, 1, mem::size_of::<Printer>());

        let data = Printer(vec![1, 2, 3]);

        let ptr = arr as *mut Printer;
        ptr::write(ptr, data);

        let mut owned = Owned::new(ptr, 0);
    }
}

fn main() {
    alloc_buggy();
    // alloc_patch();
}
