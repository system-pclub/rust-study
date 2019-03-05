use std::sync::{Arc, Mutex};

fn main() {
	let data = Arc::new(Mutex::new(0));

	{
	    let mut num = data.lock().unwrap();
	
        *num = 10;

        let num1 = data.lock().unwrap();

        println!("{}", num1);
        
	}


	let num = data.lock().unwrap();

	assert!(*num == 10);
}