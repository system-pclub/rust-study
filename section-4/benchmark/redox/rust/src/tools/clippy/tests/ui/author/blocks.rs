#![feature(stmt_expr_attributes)]
#![allow(redundant_semicolon, clippy::no_effect)]

#[rustfmt::skip]
fn main() {
    #[clippy::author]
    {
        ;;;;
    }
}

#[clippy::author]
fn foo() {
    let x = 42i32;
    -x;
}
