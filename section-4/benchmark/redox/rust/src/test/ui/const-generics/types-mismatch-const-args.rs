#![feature(const_generics)]
//~^ WARN the feature `const_generics` is incomplete and may cause the compiler to crash

// tests the diagnostic output of type mismatches for types that have const generics arguments.

use std::marker::PhantomData;

struct A<'a, T, const X: u32, const Y: u32> {
    data: PhantomData<&'a T>
}

fn a<'a, 'b>() {
    let _: A<'a, u32, {2u32}, {3u32}> = A::<'a, u32, {4u32}, {3u32}> { data: PhantomData };
    //~^ ERROR mismatched types
    let _: A<'a, u16, {2u32}, {3u32}> = A::<'b, u32, {2u32}, {3u32}> { data: PhantomData };
    //~^ ERROR mismatched types
}

pub fn main() {}
