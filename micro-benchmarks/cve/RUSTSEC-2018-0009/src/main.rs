extern crate crossbeam;

use crossbeam::queue::MsQueue;

#[derive(Debug)]
struct Printer(Vec<i32>);

impl Drop for Printer {
    fn drop(&mut self) {
        println!("Dropping: {:?}", self.0);
    }
}

/**
 * How to reproduce
 *      - git clone https://github.com/crossbeam-rs/crossbeam
 *      - git checkout v0.4.0 (buggy version)
 *      - git checkout v0.4.1 (fix version)
 *      - Set the right path of crossbeam in Cargo.toml
 *      - run this program: cargo run
*/
fn main() {
    let queue: MsQueue<Printer> = MsQueue::new();

    // 200 loop is enough to trigger GC to work.
    for i in 0..200 {
        queue.push(Printer(vec![i]));
        let item = queue.pop();
    }
}
