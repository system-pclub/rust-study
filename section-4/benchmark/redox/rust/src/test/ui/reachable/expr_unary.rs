#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(dead_code)]
#![deny(unreachable_code)]

fn foo() {
    let x: ! = ! { return; }; //~ ERROR unreachable
    //~| ERROR cannot apply unary operator `!` to type `!`
}

fn main() { }
