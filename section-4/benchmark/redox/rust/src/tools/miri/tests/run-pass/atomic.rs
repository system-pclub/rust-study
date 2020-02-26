use std::sync::atomic::{fence, AtomicBool, AtomicIsize, AtomicU64, Ordering::*};

fn main() {
    atomic_bool();
    atomic_isize();
    atomic_u64();
    atomic_fences();
}

fn atomic_bool() {
    static mut ATOMIC: AtomicBool = AtomicBool::new(false);

    unsafe {
        assert_eq!(*ATOMIC.get_mut(), false);
        ATOMIC.store(true, SeqCst);
        assert_eq!(*ATOMIC.get_mut(), true);
        ATOMIC.fetch_or(false, SeqCst);
        assert_eq!(*ATOMIC.get_mut(), true);
        ATOMIC.fetch_and(false, SeqCst);
        assert_eq!(*ATOMIC.get_mut(), false);
        ATOMIC.fetch_nand(true, SeqCst);
        assert_eq!(*ATOMIC.get_mut(), true);
        ATOMIC.fetch_xor(true, SeqCst);
        assert_eq!(*ATOMIC.get_mut(), false);
    }
}

fn atomic_isize() {
    static ATOMIC: AtomicIsize = AtomicIsize::new(0);

    // Make sure trans can emit all the intrinsics correctly
    assert_eq!(ATOMIC.compare_exchange(0, 1, Relaxed, Relaxed), Ok(0));
    assert_eq!(ATOMIC.compare_exchange(0, 2, Acquire, Relaxed), Err(1));
    assert_eq!(ATOMIC.compare_exchange(0, 1, Release, Relaxed), Err(1));
    assert_eq!(ATOMIC.compare_exchange(1, 0, AcqRel, Relaxed), Ok(1));
    ATOMIC.compare_exchange(0, 1, SeqCst, Relaxed).ok();
    ATOMIC.compare_exchange(0, 1, Acquire, Acquire).ok();
    ATOMIC.compare_exchange(0, 1, AcqRel, Acquire).ok();
    ATOMIC.compare_exchange(0, 1, SeqCst, Acquire).ok();
    ATOMIC.compare_exchange(0, 1, SeqCst, SeqCst).ok();

    ATOMIC.store(0, SeqCst);

    assert_eq!(ATOMIC.compare_exchange_weak(0, 1, Relaxed, Relaxed), Ok(0));
    assert_eq!(ATOMIC.compare_exchange_weak(0, 2, Acquire, Relaxed), Err(1));
    assert_eq!(ATOMIC.compare_exchange_weak(0, 1, Release, Relaxed), Err(1));
    assert_eq!(ATOMIC.compare_exchange_weak(1, 0, AcqRel, Relaxed), Ok(1));
    ATOMIC.compare_exchange_weak(0, 1, AcqRel, Relaxed).ok();
    ATOMIC.compare_exchange_weak(0, 1, SeqCst, Relaxed).ok();
    ATOMIC.compare_exchange_weak(0, 1, Acquire, Acquire).ok();
    ATOMIC.compare_exchange_weak(0, 1, AcqRel, Acquire).ok();
    ATOMIC.compare_exchange_weak(0, 1, SeqCst, Acquire).ok();
    ATOMIC.compare_exchange_weak(0, 1, SeqCst, SeqCst).ok();
}

fn atomic_u64() {
    static ATOMIC: AtomicU64 = AtomicU64::new(0);

    ATOMIC.store(1, SeqCst);
    assert_eq!(ATOMIC.compare_exchange(0, 0x100, AcqRel, Acquire), Err(1));
    assert_eq!(
        ATOMIC.compare_exchange_weak(1, 0x100, AcqRel, Acquire),
        Ok(1)
    );
    assert_eq!(ATOMIC.load(Relaxed), 0x100);
}

fn atomic_fences() {
    fence(SeqCst);
    fence(Release);
    fence(Acquire);
    fence(AcqRel);
}
