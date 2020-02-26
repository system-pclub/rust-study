mod foo {
    pub const b: u8 = 2;
    pub const d: u8 = 2;
}

use foo::b as c;
use foo::d;

const a: u8 = 2;

fn main() {
    let a = 4; //~ ERROR refutable pattern in local binding: `0u8..=1u8` and `3u8..=std::u8::MAX
    let c = 4; //~ ERROR refutable pattern in local binding: `0u8..=1u8` and `3u8..=std::u8::MAX
    let d = 4; //~ ERROR refutable pattern in local binding: `0u8..=1u8` and `3u8..=std::u8::MAX
    fn f() {} // Check that the `NOTE`s still work with an item here (cf. issue #35115).
}
