extern crate termsize;

pub fn main() {
    println!("{:?}", termsize::get().unwrap());
}
