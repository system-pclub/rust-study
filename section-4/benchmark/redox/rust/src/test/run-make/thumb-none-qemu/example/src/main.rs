// #![feature(stdsimd)]
#![no_main]
#![no_std]
use core::fmt::Write;
use cortex_m::asm;
use cortex_m_rt::entry;
use cortex_m_semihosting as semihosting;

//FIXME: This imports the provided #[panic_handler].
#[allow(rust_2018_idioms)]
extern crate panic_halt;

entry!(main);

fn main() -> ! {
    let x = 42;

    loop {
        asm::nop();

        // write something through semihosting interface
        let mut hstdout = semihosting::hio::hstdout().unwrap();
        let _ = write!(hstdout, "x = {}\n", x);

        // exit from qemu
        semihosting::debug::exit(semihosting::debug::EXIT_SUCCESS);
    }
}
