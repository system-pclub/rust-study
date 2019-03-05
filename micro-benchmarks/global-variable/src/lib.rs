#![feature(test)]

extern crate test;

#[cfg(test)]
mod tests {
    use super::*;
    use test::Bencher;

    use std::sync::atomic::*;
    use std::sync::RwLock;
    use std::sync::{Arc, Mutex};

    static mut UNSAFE_COUNTER: usize = 0;
    static ATOMIC_COUNTER: AtomicUsize = AtomicUsize::new(0);

    #[bench]
    fn bench_unsafe(b: &mut Bencher) {
        let mut result = 0;
        b.iter(|| {
            unsafe {
                UNSAFE_COUNTER += 1;
                result = UNSAFE_COUNTER;
            }
        });
    }

    #[bench]
    fn bench_atomic(b: &mut Bencher) {
        let mut result = 0;
        b.iter(|| {
            ATOMIC_COUNTER.fetch_add(1, Ordering::Relaxed);
            result = ATOMIC_COUNTER.load(Ordering::Relaxed);
        });
    }

    #[bench]
    fn bench_rwlock(b: &mut Bencher) {
        let lock = RwLock::new(0);
        b.iter(|| {
            {
                let mut w = lock.write().unwrap();
                *w += 1;
            }
            {
                let _result = lock.read().unwrap();
            }
        });
    }

    #[bench]
    fn bench_mutex(b: &mut Bencher) {
        let mlock = Arc::new(Mutex::new(0));
        b.iter(|| {
            {
                let mut guard = match mlock.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                *guard += 1;
            }
            {
                let _guard = mlock.lock().unwrap();
            }
        })

    }
}

