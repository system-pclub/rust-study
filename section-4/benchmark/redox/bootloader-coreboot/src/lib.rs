#![no_std]
#![feature(asm)]
#![feature(core_intrinsics)]
#![feature(lang_items)]
#![feature(naked_functions)]

#[macro_use]
extern crate bitflags;
extern crate coreboot_table;
extern crate spin;
extern crate syscall;

#[macro_use]
pub mod arch;

pub mod devices;
pub mod externs;
pub mod loader;
pub mod panic;

#[naked]
#[no_mangle]
pub unsafe fn kstart() -> ! {
    asm!("
        cli
        cld
        mov esp, 0x7000
    " : : : : "intel", "volatile");
    kmain()
}

pub unsafe fn kmain() -> ! {
    println!("Loader");

    loader::main();

    println!("Halt");

    loop {}
}
