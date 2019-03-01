#![feature(test)]

extern crate test;

#[cfg(test)]
mod tests {
    use super::*;
    use test::Bencher;

    use std::sync::atomic::*;

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
}
