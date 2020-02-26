# Call expressions

> **<sup>Syntax</sup>**\
> _CallExpression_ :\
> &nbsp;&nbsp; [_Expression_] `(` _CallParams_<sup>?</sup> `)`
>
> _CallParams_ :\
> &nbsp;&nbsp; [_Expression_]&nbsp;( `,` [_Expression_] )<sup>\*</sup> `,`<sup>?</sup>

A _call expression_ consists of an expression followed by a parenthesized
expression-list. It invokes a function, providing zero or more input variables.
If the function eventually returns, then the expression completes. For
[non-function types](../types/function-item.md), the expression f(...) uses
the method on one of the [`std::ops::Fn`], [`std::ops::FnMut`] or
[`std::ops::FnOnce`] traits, which differ in whether they take the type by
reference, mutable reference, or take ownership respectively. An automatic
borrow will be taken if needed. Rust will also automatically dereference `f` as
required. Some examples of call expressions:

```rust
# fn add(x: i32, y: i32) -> i32 { 0 }
let three: i32 = add(1i32, 2i32);
let name: &'static str = (|| "Rust")();
```

## Disambiguating Function Calls

Rust treats all function calls as sugar for a more explicit, fully-qualified
syntax. Upon compilation, Rust will desugar all function calls into the explicit
form. Rust may sometimes require you to qualify function calls with trait,
depending on the ambiguity of a call in light of in-scope items.

> **Note**: In the past, the Rust community used the terms "Unambiguous
> Function Call Syntax", "Universal Function Call Syntax", or "UFCS", in
> documentation, issues, RFCs, and other community writings. However, the term
> lacks descriptive power and potentially confuses the issue at hand. We mention
> it here for searchability's sake.

Several situations often occur which result in ambiguities about the receiver or
referent of method or associated function calls. These situations may include:

* Multiple in-scope traits define methods with the same name for the same types
* Auto-`deref` is undesirable; for example, distinguishing between methods on a
  smart pointer itself and the pointer's referent
* Methods which take no arguments, like `default()`, and return properties of a
  type, like `size_of()`

To resolve the ambiguity, the programmer may refer to their desired method or
function using more specific paths, types, or traits.

For example,

```rust
trait Pretty {
    fn print(&self);
}

trait Ugly {
  fn print(&self);
}

struct Foo;
impl Pretty for Foo {
    fn print(&self) {}
}

struct Bar;
impl Pretty for Bar {
    fn print(&self) {}
}
impl Ugly for Bar{
    fn print(&self) {}
}

fn main() {
    let f = Foo;
    let b = Bar;

    // we can do this because we only have one item called `print` for `Foo`s
    f.print();
    // more explicit, and, in the case of `Foo`, not necessary
    Foo::print(&f);
    // if you're not into the whole brevity thing
    <Foo as Pretty>::print(&f);

    // b.print(); // Error: multiple 'print' found
    // Bar::print(&b); // Still an error: multiple `print` found

    // necessary because of in-scope items defining `print`
    <Bar as Pretty>::print(&b);
}
```

Refer to [RFC 132] for further details and motivations.

[`std::ops::Fn`]: ../../std/ops/trait.Fn.html
[`std::ops::FnMut`]: ../../std/ops/trait.FnMut.html
[`std::ops::FnOnce`]: ../../std/ops/trait.FnOnce.html
[RFC 132]: https://github.com/rust-lang/rfcs/blob/master/text/0132-ufcs.md

[_Expression_]: ../expressions.md
