use std::sync::{Arc, Mutex, Condvar};
use std::time::{Duration, Instant};
use std::thread;

pub struct Event<T> {
	inner: Arc<(Mutex<Option<T>>, Condvar)>,
}

//impl<T> !Sync for Event<T> {}

impl<T> Default for Event<T> {
	fn default() -> Event<T> {
		Event {
			inner: Arc::new((Mutex::new(None), Condvar::new()))
		}
	}
}

impl<T> Event<T> {
	pub fn new() -> Event<T> {Default::default() }

	pub fn set(&self, t: T) {
		let mut l = self.inner.0.lock().unwrap();
		*l = Some(t);
		self.inner.1.notify_all();
	}

	pub fn is_set(&self) -> bool { self.inner.0.lock().unwrap().is_some() }

	fn wait(&self, res: bool, timeout: Option<Duration>) -> bool {
		let start_time = Instant::now();
		let has_timeout = timeout.is_some();
		let timeout = timeout.unwrap_or_else(|| Duration::from_millis(0));
		let mut l = self.inner.0.lock().unwrap();

		while l.is_some() != res {
			if Arc::strong_count(&self.inner) == 1 {
				return false;
			}

			if !has_timeout {
				l = self.inner.1.wait(l).unwrap();
				continue;
			}

			let elapsed = start_time.elapsed();
            if timeout <= elapsed {
                return false;
            }
            let (v, timeout_res) = self.inner.1.wait_timeout(l, timeout - elapsed).unwrap();
            if timeout_res.timed_out() {
                return false;
            }
            l = v;
		}

		true
	}

	pub fn wait_timeout(&self, timeout: Option<Duration>) -> bool {
        self.wait(true, timeout)
    }

    pub fn wait_clear(&self, timeout: Option<Duration>) -> bool {
        self.wait(false, timeout)
    }
}

impl<T> Clone for Event<T> {
	fn clone(&self) -> Event<T> {Event {inner: self.inner.clone() } }
}

impl<T> Drop for Event<T> {
	fn drop(&mut self) {
		let f = self.inner.0.lock().unwrap();
		self.inner.1.notify_all();
		drop(f);
	}
}


fn main() {
	let e1: Event<i64> = Event::new();
	let e2 = e1.clone();

	let handle = thread::spawn( move || {
			let timer = Instant::now();
			e1.set(2);
			//e1.wait_clear(Some(Duration::from_millis(500)));
			e1.wait_clear(None);
			assert!(timer.elapsed() < Duration::from_millis(500));
		}
	);

	e2.wait_timeout(None);

	let cloned = e2.inner.clone();

	drop(e2);
	
	thread::sleep(Duration::from_millis(1000));
	drop(cloned);

	handle.join().unwrap();
	//println!("before finishing");
}