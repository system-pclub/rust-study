#![feature(const_raw_ptr_to_usize_cast)]

const BAR: *mut () = ((|| 3) as fn() -> i32) as *mut ();
pub const FOO: usize = unsafe { BAR as usize };
//~^ ERROR any use of this value will cause an error

fn main() {}
