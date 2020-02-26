// revisions: curr object_safe_for_dispatch

#![cfg_attr(object_safe_for_dispatch, feature(object_safe_for_dispatch))]
#![feature(arbitrary_self_types)]

use std::rc::Rc;

trait Foo {
    fn foo(self: &Rc<Self>) -> usize;
}

trait Bar {
    fn foo(self: &Rc<Self>) -> usize where Self: Sized;
    fn bar(self: Rc<Self>) -> usize;
}

impl Foo for usize {
    fn foo(self: &Rc<Self>) -> usize {
        **self
    }
}

impl Bar for usize {
    fn foo(self: &Rc<Self>) -> usize {
        **self
    }

    fn bar(self: Rc<Self>) -> usize {
        *self
    }
}

fn make_foo() {
    let x = Rc::new(5usize) as Rc<dyn Foo>;
    //[curr]~^ ERROR E0038
    //[curr]~| ERROR E0038
    //[object_safe_for_dispatch]~^^^ ERROR E0038
}

fn make_bar() {
    let x = Rc::new(5usize) as Rc<dyn Bar>;
    x.bar();
}

fn main() {}
