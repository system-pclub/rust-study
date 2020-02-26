// ignore-macos
// ignore-windows

#![feature(lang_items, link_args, start, libc)]
#![link_args = "-nostartfiles"]
#![no_std]

use core::panic::PanicInfo;
use core::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

#[warn(clippy::main_recursion)]
#[start]
fn main(argc: isize, argv: *const *const u8) -> isize {
    let x = N.load(Ordering::Relaxed);
    N.store(x + 1, Ordering::Relaxed);

    if x < 3 {
        main(argc, argv);
    }

    0
}

#[allow(clippy::empty_loop)]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[lang = "eh_personality"]
extern "C" fn eh_personality() {}
