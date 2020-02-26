#![feature(plugin)]
#![warn(clippy::indexing_slicing)]
// We also check the out_of_bounds_indexing lint here, because it lints similar things and
// we want to avoid false positives.
#![warn(clippy::out_of_bounds_indexing)]
#![allow(clippy::no_effect, clippy::unnecessary_operation)]

fn main() {
    let x = [1, 2, 3, 4];
    let index: usize = 1;
    let index_from: usize = 2;
    let index_to: usize = 3;
    x[index];
    &x[index..];
    &x[..index];
    &x[index_from..index_to];
    &x[index_from..][..index_to]; // Two lint reports, one for [index_from..] and another for [..index_to].
    x[4]; // Ok, let rustc's `const_err` lint handle `usize` indexing on arrays.
    x[1 << 3]; // Ok, let rustc's `const_err` lint handle `usize` indexing on arrays.
    &x[5..][..10]; // Two lint reports, one for out of bounds [5..] and another for slicing [..10].
    &x[0..][..3];
    &x[1..][..5];

    &x[0..].get(..3); // Ok, should not produce stderr.
    x[0]; // Ok, should not produce stderr.
    x[3]; // Ok, should not produce stderr.
    &x[0..3]; // Ok, should not produce stderr.

    let y = &x;
    y[0];
    &y[1..2];
    &y[0..=4];
    &y[..=4];

    &y[..]; // Ok, should not produce stderr.

    let v = vec![0; 5];
    v[0];
    v[10];
    v[1 << 3];
    &v[10..100];
    &x[10..][..100]; // Two lint reports, one for [10..] and another for [..100].
    &v[10..];
    &v[..100];

    &v[..]; // Ok, should not produce stderr.

    //
    // Continue tests at end function to minimize the changes to this file's corresponding stderr.
    //

    const N: usize = 15; // Out of bounds
    const M: usize = 3; // In bounds
    x[N]; // Ok, let rustc's `const_err` lint handle `usize` indexing on arrays.
    x[M]; // Ok, should not produce stderr.
    v[N];
    v[M];
}
