// run-pass

#![feature(const_generics)]
//~^ WARN the feature `const_generics` is incomplete and may cause the compiler to crash

#[derive(PartialEq, Eq)]
struct NoMatch;

fn foo<const T: NoMatch>() -> bool {
    true
}

fn main() {
    foo::<{NoMatch}>();
}
