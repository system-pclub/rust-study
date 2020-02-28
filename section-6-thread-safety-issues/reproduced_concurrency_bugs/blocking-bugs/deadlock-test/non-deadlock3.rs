use std::sync::{Arc, Mutex};

fn main() {
	let data = Arc::new(Mutex::new(0));

	{

		*data.lock().unwrap() = 20;

	    let  num = data.lock().unwrap();
	
        println!("{}", num);
        
	}


	let num = data.lock().unwrap();

	assert!(*num == 20);
}