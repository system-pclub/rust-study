// aux-build:inline-default-methods.rs
// ignore-cross-compile

extern crate inline_default_methods;

// @has inline_default_methods/trait.Foo.html
// @has - '//*[@class="rust trait"]' 'fn bar(&self);'
// @has - '//*[@class="rust trait"]' 'fn foo(&mut self) { ... }'
pub use inline_default_methods::Foo;
