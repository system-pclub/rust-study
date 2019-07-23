#![feature(duration_float)]

mod test;

fn main() {
    let strip = 1000;
    test::copy_1gb_pointer();
    test::copy_1gb_slice();
    test::unsafe_iterate();
    test::safe_iterate();
    test::unsafe_index(strip);
    test::safe_index(strip);
    test::bench_array();
    test::bench_offset();
}