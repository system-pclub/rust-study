// compile-flags: -Z print-type-sizes
// build-pass (FIXME(62277): could be check-pass?)
// ignore-pass
// ^-- needed because `--pass check` does not emit the output needed.
//     FIXME: consider using an attribute instead of side-effects.

#![feature(start)]

#[start]
fn start(_: isize, _: *const *const u8) -> isize {
    let _x: Option<!> = None;
    let _y: Result<u32, !> = Ok(42);
    0
}
