extern crate slice_deque;

use slice_deque::SliceDeque;

/**
 *  This bug need to be reproduced with release build
 */
const VALUE: [i32; 3] = [45, 46, 47];

fn main() {
    let mut v = SliceDeque::new();
    // construct the slice that can trigger the bug
    v.push_back(VALUE);
    v.push_back(VALUE);
    v.push_back(VALUE);
    v.push_front(VALUE);

    // trigger the bug
    let first = v.pop_front().unwrap();
    println!("first: {:?}", first);
    let second = v.pop_front().unwrap();
    println!("value_second: {:?}", second);
}
