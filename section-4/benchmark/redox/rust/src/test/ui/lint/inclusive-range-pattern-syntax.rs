// build-pass (FIXME(62277): could be check-pass?)
// run-rustfix

#![warn(ellipsis_inclusive_range_patterns)]

fn main() {
    let despondency = 2;
    match despondency {
        1...2 => {}
        //~^ WARN `...` range patterns are deprecated
        _ => {}
    }

    match &despondency {
        &1...2 => {}
        //~^ WARN `...` range patterns are deprecated
        _ => {}
    }
}
