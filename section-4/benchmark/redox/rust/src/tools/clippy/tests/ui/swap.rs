#![warn(clippy::all)]
#![allow(
    clippy::blacklisted_name,
    clippy::no_effect,
    clippy::redundant_clone,
    redundant_semicolon,
    unused_assignments
)]

struct Foo(u32);

#[derive(Clone)]
struct Bar {
    a: u32,
    b: u32,
}

fn field() {
    let mut bar = Bar { a: 1, b: 2 };

    let temp = bar.a;
    bar.a = bar.b;
    bar.b = temp;

    let mut baz = vec![bar.clone(), bar.clone()];
    let temp = baz[0].a;
    baz[0].a = baz[1].a;
    baz[1].a = temp;
}

fn array() {
    let mut foo = [1, 2];
    let temp = foo[0];
    foo[0] = foo[1];
    foo[1] = temp;

    foo.swap(0, 1);
}

fn slice() {
    let foo = &mut [1, 2];
    let temp = foo[0];
    foo[0] = foo[1];
    foo[1] = temp;

    foo.swap(0, 1);
}

fn vec() {
    let mut foo = vec![1, 2];
    let temp = foo[0];
    foo[0] = foo[1];
    foo[1] = temp;

    foo.swap(0, 1);
}

#[rustfmt::skip]
fn main() {
    field();
    array();
    slice();
    vec();

    let mut a = 42;
    let mut b = 1337;

    a = b;
    b = a;

    ; let t = a;
    a = b;
    b = t;

    let mut c = Foo(42);

    c.0 = a;
    a = c.0;

    ; let t = c.0;
    c.0 = a;
    a = t;
}
