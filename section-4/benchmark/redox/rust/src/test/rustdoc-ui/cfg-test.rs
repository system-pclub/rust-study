// build-pass (FIXME(62277): could be check-pass?)
// compile-flags:--test --test-args --test-threads=1
// normalize-stdout-test: "src/test/rustdoc-ui" -> "$$DIR"

// Crates like core have doctests gated on `cfg(not(test))` so we need to make
// sure `cfg(test)` is not active when running `rustdoc --test`.

/// this doctest will be ignored:
///
/// ```
/// assert!(false);
/// ```
#[cfg(test)]
pub struct Foo;

/// this doctest will be tested:
///
/// ```
/// assert!(true);
/// ```
#[cfg(not(test))]
pub struct Foo;

/// this doctest will be tested, but will not appear in documentation:
///
/// ```
/// assert!(true)
/// ```
#[cfg(doctest)]
pub struct Bar;
