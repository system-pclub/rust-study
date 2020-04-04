extern crate slice_deque;

use slice_deque::SliceDeque;

/**
 *  How to reproduce this bug
 *  This bug need to be reproduced with release build
 *      - Get slice_deque source code - git clone https://github.com/gnzlbg/slice_deque
 *      - git checkout 57b1a84
 *      - compile this package and run
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
