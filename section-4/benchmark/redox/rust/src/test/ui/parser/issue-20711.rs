struct Foo;

impl Foo {
    #[stable(feature = "rust1", since = "1.0.0")]
} //~ ERROR expected one of `async`, `const`, `crate`, `default`, `extern`, `fn`, `pub`, `type`, or

fn main() {}
