// run-pass

#![feature(const_generics)]
//~^ WARN the feature `const_generics` is incomplete and may cause the compiler to crash

use std::fmt;

struct Array<T>(T);

impl<T: fmt::Debug, const N: usize> fmt::Debug for Array<[T; N]> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_list().entries((&self.0 as &[T]).iter()).finish()
    }
}

fn main() {
    assert_eq!(format!("{:?}", Array([1, 2, 3])), "[1, 2, 3]");
}
