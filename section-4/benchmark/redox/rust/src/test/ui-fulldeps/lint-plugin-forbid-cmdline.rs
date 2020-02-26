// aux-build:lint-plugin-test.rs
// ignore-stage1
// compile-flags: -F test-lint

#![feature(plugin)]
#![plugin(lint_plugin_test)]
//~^ WARN use of deprecated attribute `plugin`
fn lintme() { } //~ ERROR item is named 'lintme'

#[allow(test_lint)] //~ ERROR allow(test_lint) overruled by outer forbid(test_lint)
pub fn main() {
    lintme();
}
