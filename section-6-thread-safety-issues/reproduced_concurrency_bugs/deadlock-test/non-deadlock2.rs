use std::sync::{Arc, Mutex};

fn main() {
	let data = Arc::new(Mutex::new(0));

	{
	    let _ = data.lock().unwrap();
	
        let _ = data.lock().unwrap();
        
	}


	let num = data.lock().unwrap();

	assert!(*num == 0);
}