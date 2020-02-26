// run-pass
// aux-build:lint-plugin-test.rs
// ignore-stage1
#![feature(plugin)]
#![plugin(lint_plugin_test)] //~ WARNING use of deprecated attribute
#![allow(dead_code)]

fn lintme() { } //~ WARNING item is named 'lintme'

#[allow(test_lint)]
pub fn main() {
    fn lintme() { }
}
