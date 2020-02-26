// ignore-emscripten default visibility is hidden
// compile-flags: -O
// `#[no_mangle]`d static variables always have external linkage, i.e., no `internal` in their
// definitions

#![crate_type = "lib"]
#![no_std]

// CHECK: @A = local_unnamed_addr constant
#[no_mangle]
static A: u8 = 0;

// CHECK: @B = local_unnamed_addr global
#[no_mangle]
static mut B: u8 = 0;

// CHECK: @C = local_unnamed_addr constant
#[no_mangle]
pub static C: u8 = 0;

// CHECK: @D = local_unnamed_addr global
#[no_mangle]
pub static mut D: u8 = 0;

mod private {
    // CHECK: @E = local_unnamed_addr constant
    #[no_mangle]
    static E: u8 = 0;

    // CHECK: @F = local_unnamed_addr global
    #[no_mangle]
    static mut F: u8 = 0;

    // CHECK: @G = local_unnamed_addr constant
    #[no_mangle]
    pub static G: u8 = 0;

    // CHECK: @H = local_unnamed_addr global
    #[no_mangle]
    pub static mut H: u8 = 0;
}

const HIDDEN: () = {
    // CHECK: @I = local_unnamed_addr constant
    #[no_mangle]
    static I: u8 = 0;

    // CHECK: @J = local_unnamed_addr global
    #[no_mangle]
    static mut J: u8 = 0;

    // CHECK: @K = local_unnamed_addr constant
    #[no_mangle]
    pub static K: u8 = 0;

    // CHECK: @L = local_unnamed_addr global
    #[no_mangle]
    pub static mut L: u8 = 0;
};

// The surrounding item should not accidentally become external
fn x() {
    // CHECK: @M = local_unnamed_addr constant
    #[no_mangle]
    static M: fn() = x;

    // CHECK: @N = local_unnamed_addr global
    #[no_mangle]
    static mut N: u8 = 0;

    // CHECK: @O = local_unnamed_addr constant
    #[no_mangle]
    pub static O: u8 = 0;

    // CHECK: @P = local_unnamed_addr global
    #[no_mangle]
    pub static mut P: u8 = 0;
}
// CHECK-LABEL: ; external_no_mangle_statics::x
// CHECK-NEXT: ; Function Attrs:
// CHECK-NEXT: define internal
