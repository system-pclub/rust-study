#![feature(staged_api)]

#[stable(feature = "rust1", since = "1.0.0")]
const fn error(_: fn()) {}

#[stable(feature = "rust1", since = "1.0.0")]
#[rustc_allow_const_fn_ptr]
//~^ ERROR internal implementation detail
const fn compiles(_: fn()) {}

fn main() {}
