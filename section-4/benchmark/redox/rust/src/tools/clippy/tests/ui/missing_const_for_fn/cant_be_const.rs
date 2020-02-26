//! False-positive tests to ensure we don't suggest `const` for things where it would cause a
//! compilation error.
//! The .stderr output of this test should be empty. Otherwise it's a bug somewhere.

#![warn(clippy::missing_const_for_fn)]
#![feature(start)]

struct Game;

// This should not be linted because it's already const
const fn already_const() -> i32 {
    32
}

impl Game {
    // This should not be linted because it's already const
    pub const fn already_const() -> i32 {
        32
    }
}

// Allowing on this function, because it would lint, which we don't want in this case.
#[allow(clippy::missing_const_for_fn)]
fn random() -> u32 {
    42
}

// We should not suggest to make this function `const` because `random()` is non-const
fn random_caller() -> u32 {
    random()
}

static Y: u32 = 0;

// We should not suggest to make this function `const` because const functions are not allowed to
// refer to a static variable
fn get_y() -> u32 {
    Y
    //~^ ERROR E0013
}

// Don't lint entrypoint functions
#[start]
fn init(num: isize, something: *const *const u8) -> isize {
    1
}

trait Foo {
    // This should not be suggested to be made const
    // (rustc doesn't allow const trait methods)
    fn f() -> u32;

    // This should not be suggested to be made const either
    fn g() -> u32 {
        33
    }
}

// Don't lint in external macros (derive)
#[derive(PartialEq, Eq)]
struct Point(isize, isize);

impl std::ops::Add for Point {
    type Output = Self;

    // Don't lint in trait impls of derived methods
    fn add(self, other: Self) -> Self {
        Point(self.0 + other.0, self.1 + other.1)
    }
}

mod with_drop {
    pub struct A;
    pub struct B;
    impl Drop for A {
        fn drop(&mut self) {}
    }

    impl A {
        // This can not be const because the type implements `Drop`.
        pub fn a(self) -> B {
            B
        }
    }

    impl B {
        // This can not be const because `a` implements `Drop`.
        pub fn a(self, a: A) -> B {
            B
        }
    }
}
