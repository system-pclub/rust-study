//! crt0

#![no_std]
#![feature(asm)]
#![feature(linkage)]
#![feature(naked_functions)]

#[no_mangle]
#[naked]
pub unsafe extern "C" fn _start() {
    #[cfg(target_arch = "x86_64")]
    asm!("mov rdi, rsp
        and rsp, 0xFFFFFFFFFFFFFFF0
        call relibc_start"
        :
        :
        :
        : "intel", "volatile"
    );
    #[cfg(target_arch = "aarch64")]
    asm!("mov x0, sp
        bl relibc_start"
        :
        :
        :
        : "volatile"
    );
}

#[panic_handler]
#[linkage = "weak"]
#[no_mangle]
pub unsafe extern "C" fn rust_begin_unwind(pi: &::core::panic::PanicInfo) -> ! {
    extern "C" {
        fn relibc_panic(pi: &::core::panic::PanicInfo) -> !;
    }
    relibc_panic(pi)
}
