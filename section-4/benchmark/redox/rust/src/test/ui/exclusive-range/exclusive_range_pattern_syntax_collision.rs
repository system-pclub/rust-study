#![feature(exclusive_range_pattern)]

fn main() {
    match [5..4, 99..105, 43..44] {
        [_, 99.., _] => {},
        //~^ ERROR `X..` range patterns are not supported
        //~| ERROR mismatched types
        _ => {},
    }
}
