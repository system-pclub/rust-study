use std::sync::Mutex;
use std::sync::Arc;
use std::thread;

fn main() {
	let lock = Arc::new(Mutex::new(0_u32));

	let lock_new = Arc::clone(&lock);

	let result = thread::spawn(move || {
		let mut data = lock_new.lock().unwrap();
		*data += 4;
		panic!();
	}).join();

	match lock.lock() {
		Ok(data) => println!("{}", *data),
		Err(poisoned) => println!("Poison: {}", poisoned.into_inner()),
	};

}
