// run-rustfix

#![feature(stmt_expr_attributes)]
#![allow(unused_variables)]

struct Outer {
    inner: u32,
}

#[deny(clippy::ref_in_deref)]
fn main() {
    let outer = Outer { inner: 0 };
    let inner = (&outer).inner;
}
