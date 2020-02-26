// This should fail even without validation.
// compile-flags: -Zmiri-disable-validation

fn main() {
    let x = [2u16, 3, 4, 5]; // Make it big enough so we don't get an out-of-bounds error.
    let x = &x[0] as *const _ as *const *const u8; // cast to ptr-to-ptr, so that we load a ptr
    // This must fail because alignment is violated. Test specifically for loading pointers,
    // which have special code in miri's memory.
    let _x = unsafe { *x };
    //~^ ERROR tried to access memory with alignment 2, but alignment
}
