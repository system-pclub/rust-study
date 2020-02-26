#![feature(rustc_attrs)]

fn func<#[rustc_synthetic] T>(_: T) {}

struct Foo;

impl Foo {
    pub fn func<#[rustc_synthetic] T>(_: T) {}
}

struct Bar<S> {
    t: S
}

impl<S> Bar<S> {
    pub fn func<#[rustc_synthetic] T>(_: T) {}
}

fn main() {
    func::<u8>(42); //~ ERROR cannot provide explicit generic arguments
    func(42); // Ok

    Foo::func::<u8>(42); //~ ERROR cannot provide explicit generic arguments
    Foo::func(42); // Ok

    Bar::<i8>::func::<u8>(42); //~ ERROR cannot provide explicit generic arguments
    Bar::<i8>::func(42); // Ok
}
