//use std::mem::cast;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time;
use std::mem;

const N: usize = 10;

#[derive(Debug)]
struct Printer(Vec<usize>);

impl Drop for Printer {
    fn drop(&mut self) {
        println!("Dropping: {:?}", self.0);
    }
}


fn bug() {
    let shared_vec = unsafe {
        Arc::new(Mutex::new(
            mem::transmute::<Box<Printer>, *const ()>(Box::new(Printer(Vec::new())))
        ))
    };

    println!("step 1");
    let vec1 = shared_vec.clone();
    {
        let mut val = vec1.lock().unwrap();
        let mut v = unsafe {
            mem::transmute::<*const (), Box<Printer>>(*val)
        };
        v.0.push(1);
    }

    println!("step 2");
    let vec_2 = shared_vec.clone();
    {
        let mut val = vec_2.lock().unwrap();
        let mut v = unsafe {
            mem::transmute::<*const (), Box<Printer>>(*val)
        };
        v.0.push(2);
    }
    println!("Done");
}

fn patch() {
    let shared_vec = Arc::new(Mutex::new(Box::new(Printer(Vec::new()))));
    let mut thread_vec = Vec::new();
    for i in 0..N {
        let my_vec = shared_vec.clone();
        let handle = thread::spawn(move || {
            thread::sleep(time::Duration::from_micros(5));
            let mut v = my_vec.lock().unwrap();
            v.0.push(i);
        });
        thread_vec.push(handle);
    }

    for handle in thread_vec {
        handle.join();
    }
    println!("shared_vec: {:?}", shared_vec);
}

fn main() {
    bug();
    // patch();
}
