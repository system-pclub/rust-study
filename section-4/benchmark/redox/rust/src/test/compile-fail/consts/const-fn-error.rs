#![feature(const_fn)]

const X : usize = 2;

const fn f(x: usize) -> usize {
    let mut sum = 0;
    for i in 0..x {
        //~^ ERROR E0015
        //~| ERROR E0017
        //~| ERROR E0080
        //~| ERROR E0744
        //~| ERROR E0019
        sum += i;
    }
    sum
}

#[allow(unused_variables)]
fn main() {
    let a : [i32; f(X)];
}
