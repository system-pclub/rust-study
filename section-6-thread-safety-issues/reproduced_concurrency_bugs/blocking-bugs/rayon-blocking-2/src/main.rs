use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::thread;
use std::time;

struct LockLatch {
    b: AtomicBool,
    m: Mutex<()>,
    v: Condvar,
}

unsafe impl Sync for LockLatch {}

pub trait Latch {
    fn set(&self);
    fn wait(&self);
}

impl LockLatch {
    #[inline]
    pub fn new() -> LockLatch {
        LockLatch {
            b: AtomicBool::new(false),
            m: Mutex::new(()),
            v: Condvar::new(),
        }
    }

    pub fn probe(&self) -> bool {
        self.b.load(Ordering::Acquire)
    }
}

impl Latch for LockLatch {
    /// Set the latch to true, releasing all threads who are waiting.
    fn set(&self) {
        self.b.store(true, Ordering::Release);
        self.v.notify_all();
    }

    /// Spin until latch is set. Use with caution.
    fn wait(&self) {
        let mut guard = self.m.lock().unwrap();
        while !self.probe() {
            thread::sleep(time::Duration::from_millis(2000));
            guard = self.v.wait(guard).unwrap();
        }
    }
}

fn main() {
    let latch = Arc::new(LockLatch::new());
    let cloned_latch = latch.clone();
    thread::spawn(move || {
        cloned_latch.set();
    });

    latch.wait();
    println!("Hello World!");
}
