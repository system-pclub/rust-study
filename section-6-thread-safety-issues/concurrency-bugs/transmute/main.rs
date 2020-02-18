use std::sync::Arc;
use std::thread;


#[derive(Debug)]
struct Point {
	x: f32,
	y: f32,
}

fn main() {

	let p = Arc::new(Point{x:1.2, y:3.2});

    {
    	let p = Arc::clone(&p);
		thread::spawn(move || {
			let p :* mut Point = unsafe{ std::mem::transmute(Arc::into_raw(p))};
			let p1 = unsafe{& mut *p};
			p1.x = 200.0;
			println!("{:?}", p1);

			/*
			unsafe { 
				//let x: * mut Point = Arc::into_raw(p);
				let p = Arc::into_raw(p) as * mut Point;
				(*p).x = 200.0;
				println!("{:?}", *p);
			}
			*/
		}).join().unwrap();
	}

	println!("{:?} in main thread", p);
}