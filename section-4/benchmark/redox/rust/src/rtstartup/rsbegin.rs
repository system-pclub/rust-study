// rsbegin.o and rsend.o are the so called "compiler runtime startup objects".
// They contain code needed to correctly initialize the compiler runtime.
//
// When an executable or dylib image is linked, all user code and libraries are
// "sandwiched" between these two object files, so code or data from rsbegin.o
// become first in the respective sections of the image, whereas code and data
// from rsend.o become the last ones. This effect can be used to place symbols
// at the beginning or at the end of a section, as well as to insert any required
// headers or footers.
//
// Note that the actual module entry point is located in the C runtime startup
// object (usually called `crtX.o), which then invokes initialization callbacks
// of other runtime components (registered via yet another special image section).

#![feature(no_core, lang_items, optin_builtin_traits)]
#![crate_type = "rlib"]
#![no_core]
#![allow(non_camel_case_types)]

#[lang = "sized"]
trait Sized {}
#[lang = "sync"]
auto trait Sync {}
#[lang = "copy"]
trait Copy {}
#[lang = "freeze"]
auto trait Freeze {}

#[lang = "drop_in_place"]
#[inline]
#[allow(unconditional_recursion)]
pub unsafe fn drop_in_place<T: ?Sized>(to_drop: *mut T) {
    drop_in_place(to_drop);
}

#[cfg(all(target_os = "windows", target_arch = "x86", target_env = "gnu"))]
pub mod eh_frames {
    #[no_mangle]
    #[link_section = ".eh_frame"]
    // Marks beginning of the stack frame unwind info section
    pub static __EH_FRAME_BEGIN__: [u8; 0] = [];

    // Scratch space for unwinder's internal book-keeping.
    // This is defined as `struct object` in $GCC/libgcc/unwind-dw2-fde.h.
    static mut OBJ: [isize; 6] = [0; 6];

    macro_rules! impl_copy {
        ($($t:ty)*) => {
            $(
                impl ::Copy for $t {}
            )*
        }
    }

    impl_copy! {
        usize u8 u16 u32 u64 u128
        isize i8 i16 i32 i64 i128
        f32 f64
        bool char
    }

    // Unwind info registration/deregistration routines.
    // See the docs of `unwind` module in libstd.
    extern "C" {
        fn rust_eh_register_frames(eh_frame_begin: *const u8, object: *mut u8);
        fn rust_eh_unregister_frames(eh_frame_begin: *const u8, object: *mut u8);
    }

    unsafe fn init() {
        // register unwind info on module startup
        rust_eh_register_frames(
            &__EH_FRAME_BEGIN__ as *const u8,
            &mut OBJ as *mut _ as *mut u8,
        );
    }

    unsafe fn uninit() {
        // unregister on shutdown
        rust_eh_unregister_frames(
            &__EH_FRAME_BEGIN__ as *const u8,
            &mut OBJ as *mut _ as *mut u8,
        );
    }

    // MSVC-specific init/uninit routine registration
    pub mod ms_init {
        // .CRT$X?? sections are roughly analogous to ELF's .init_array and .fini_array,
        // except that they exploit the fact that linker will sort them alphabitically,
        // so e.g., sections with names between .CRT$XIA and .CRT$XIZ are guaranteed to be
        // placed between those two, without requiring any ordering of objects on the linker
        // command line.
        // Note that ordering of same-named sections from different objects is not guaranteed.
        // Since .CRT$XIA contains init array's header symbol, which must always come first,
        // we place our initialization callback into .CRT$XIB.

        #[link_section = ".CRT$XIB"] // .CRT$XI? : C initialization callbacks
        pub static P_INIT: unsafe fn() = super::init;

        #[link_section = ".CRT$XTY"] // .CRT$XT? : C termination callbacks
        pub static P_UNINIT: unsafe fn() = super::uninit;
    }
}
