// run-rustfix

#![allow(unused_parens)]

fn f() -> usize {
    42
}

#[warn(clippy::range_plus_one)]
fn main() {
    for _ in 0..2 {}
    for _ in 0..=2 {}

    for _ in 0..3 + 1 {}
    for _ in 0..=3 + 1 {}

    for _ in 0..1 + 5 {}
    for _ in 0..=1 + 5 {}

    for _ in 1..1 + 1 {}
    for _ in 1..=1 + 1 {}

    for _ in 0..13 + 13 {}
    for _ in 0..=13 - 7 {}

    for _ in 0..(1 + f()) {}
    for _ in 0..=(1 + f()) {}

    let _ = ..11 - 1;
    let _ = ..=11 - 1;
    let _ = ..=(11 - 1);
    let _ = (1..11 + 1);
    let _ = (f() + 1)..(f() + 1);

    const ONE: usize = 1;
    // integer consts are linted, too
    for _ in 1..ONE + ONE {}

    let mut vec: Vec<()> = std::vec::Vec::new();
    vec.drain(..);
}
