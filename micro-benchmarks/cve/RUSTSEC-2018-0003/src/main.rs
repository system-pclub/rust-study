/**
 * Reproduce this bug:
 *      - Get the package rust-smallvec: git clone https://github.com/servo/rust-smallvec
 *      - Set the path of smallvec at Cargo.toml
 *      - Go to rust-smallvec directory and run: git checkout 26b2490
 *      - Build this package and run
 */

extern crate smallvec;

use smallvec::SmallVec;

#[derive(Debug)]
struct Printer(Vec<i32>);

impl Drop for Printer {
    fn drop(&mut self) {
        //println!("Dropping {:?}", self.0);
    }
}

struct Bad;

impl Iterator for Bad {
    type Item = Printer;

    fn size_hint(&self) -> (usize, Option<usize>) {
        (1, None)
    }

    fn next(&mut self) -> Option<Printer> {
        panic!()
    }
}

// If you run this in the Rust playground this prints:
// Dropping 0
// Dropping 0
// Dropping 1
//
// Obviously this is fine with this dummy struct but if you
// used `Box` this would cause a double-free.
//
// What happens is that before iterating,
// `SmallVec::insert_many` moves the existing elements, so
// if you start with an arry that looks like this:
// [a, b, c, (uninitialised)]
//  ^-----^ The elements between these points are at
//          indexes less than `len` and so can be
//          accessed.
//
// You (temporarily) get an array that looks like this:
// [a, b, b, c, (uninitialised)]
//  ^-----^ Accessible elements
//
// When the iterator panics, the `SmallVec` iterates over
// the accessible elements and drops each of them in turn,
// which is bad when there are two copies of the same value
// (you get a double-drop).
fn main() {
    // This doesn't need to be 0, this is unsound with any
    // value here.
    let mut vec: SmallVec<[Printer; 0]> = vec![
        Printer(vec![0]),
        Printer(vec![1]),
        Printer(vec![2]),
        Printer(vec![3]),
    ].into();

    std::panic::catch_unwind(move || {
        vec.insert_many(2, Bad);
    });
}