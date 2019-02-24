extern crate libc;

use libc::c_char;
use std::ffi::CStr;
use std::str;
use std::ptr;

unsafe fn c_str_to_string(s: *const c_char) -> String {
    str::from_utf8(CStr::from_ptr(s).to_bytes()).unwrap().to_owned()
}


fn main() {
    unsafe {
        c_str_to_string(ptr::null() as *const c_char);
        // let s = String::from("sss");
        // c_str_to_string(s.as_ptr() as *const c_char);
    }
}
