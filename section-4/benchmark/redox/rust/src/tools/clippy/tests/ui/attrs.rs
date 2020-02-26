#![warn(clippy::inline_always, clippy::deprecated_semver)]
#![allow(clippy::assertions_on_constants)]
#[inline(always)]
fn test_attr_lint() {
    assert!(true)
}

#[inline(always)]
fn false_positive_expr() {
    unreachable!()
}

#[inline(always)]
fn false_positive_stmt() {
    unreachable!();
}

#[inline(always)]
fn empty_and_false_positive_stmt() {
    unreachable!();
}

#[deprecated(since = "forever")]
pub const SOME_CONST: u8 = 42;

#[deprecated(since = "1")]
pub const ANOTHER_CONST: u8 = 23;

#[deprecated(since = "0.1.1")]
pub const YET_ANOTHER_CONST: u8 = 0;

fn main() {
    test_attr_lint();
    if false {
        false_positive_expr()
    }
    if false {
        false_positive_stmt()
    }
    if false {
        empty_and_false_positive_stmt()
    }
}
