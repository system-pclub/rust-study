// aux-build:unstable-macros.rs

#![feature(decl_macro)]
#![feature(staged_api)]
#[macro_use] extern crate unstable_macros;

#[unstable(feature = "local_unstable", issue = "0")]
macro_rules! local_unstable { () => () }

#[unstable(feature = "local_unstable", issue = "0")]
macro local_unstable_modern() {}

#[stable(feature = "deprecated_macros", since = "1.0.0")]
#[rustc_deprecated(since = "1.0.0", reason = "local deprecation reason")]
#[macro_export]
macro_rules! local_deprecated{ () => () }

fn main() {
    local_unstable!(); //~ ERROR use of unstable library feature 'local_unstable'
    local_unstable_modern!(); //~ ERROR use of unstable library feature 'local_unstable'
    unstable_macro!(); //~ ERROR use of unstable library feature 'unstable_macros'
    // unstable_macro_modern!(); // ERROR use of unstable library feature 'unstable_macros'

    deprecated_macro!();
    //~^ WARN use of deprecated item 'deprecated_macro': deprecation reason
    local_deprecated!();
    //~^ WARN use of deprecated item 'local_deprecated': local deprecation reason
}
