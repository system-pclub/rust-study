use std::boxed::Box;
use std::ptr;

#[derive(Debug)]
struct Printer(Vec<i32>);

impl Drop for Printer {
    fn drop(&mut self) {
        println!("Dropping: {:?}", self.0);
    }
}

fn get_an_option_box() -> Option<Box<Printer>> {
    Some(Box::new(Printer(vec![1, 2, 3])))
}

fn get_an_option() -> Option<Printer> {
    Some(Printer(vec![1, 2, 3]))
}

fn use_after_free() -> *const Printer {
    let ret = match get_an_option() {
        None => ptr::null(),
        Some(ref box_raw) => {
            let ptr: *const Printer = box_raw;
            ptr
        }
    };
    println!("use_after_free: return");
    ret
}

fn fix_use_after_free() -> *const Printer {
    let ret = match get_an_option_box() {
        None => ptr::null(),
        Some(box_raw) => {
            let ptr: *const Printer = Box::into_raw(box_raw);
            ptr
        }
    };
    println!("fix_use_after_free: return");
    ret
}

fn destroy(ptr: *const Printer) {
    unsafe {
        println!("Drop manually");
        drop(Box::from_raw(ptr as *mut Printer));
    }
}

fn main() {
    let mut ptr = use_after_free();
    ptr = fix_use_after_free();
    destroy(ptr);
}
