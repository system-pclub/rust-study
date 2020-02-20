extern crate rayon;

fn main(){
	let pool1 = rayon::Configuration::new().num_threads(1).build().unwrap();
	let pool2 = rayon::Configuration::new().num_threads(1).build().unwrap();

	pool1.install(|| { // JOB_A
        // this will block pool1's thread:
        pool2.install(|| { //JOB_B
            // this will block pool2's thread:
            pool1.install(|| { //JOB_C
                // there are no threads left to run this!
                println!("hello?");
            });
        });
	});
}

