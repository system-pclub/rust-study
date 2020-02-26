// aux-build:option_helpers.rs
// compile-flags: --edition 2018

#![warn(clippy::all, clippy::pedantic)]
#![allow(
    clippy::blacklisted_name,
    clippy::default_trait_access,
    clippy::missing_docs_in_private_items,
    clippy::non_ascii_literal,
    clippy::new_without_default,
    clippy::needless_pass_by_value,
    clippy::print_stdout,
    clippy::must_use_candidate,
    clippy::use_self,
    clippy::useless_format,
    clippy::wrong_self_convention,
    clippy::unused_self,
    unused
)]

#[macro_use]
extern crate option_helpers;

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::iter::FromIterator;
use std::ops::Mul;
use std::rc::{self, Rc};
use std::sync::{self, Arc};

use option_helpers::IteratorFalsePositives;

pub struct T;

impl T {
    pub fn add(self, other: T) -> T {
        self
    }

    // no error, not public interface
    pub(crate) fn drop(&mut self) {}

    // no error, private function
    fn neg(self) -> Self {
        self
    }

    // no error, private function
    fn eq(&self, other: T) -> bool {
        true
    }

    // No error; self is a ref.
    fn sub(&self, other: T) -> &T {
        self
    }

    // No error; different number of arguments.
    fn div(self) -> T {
        self
    }

    // No error; wrong return type.
    fn rem(self, other: T) {}

    // Fine
    fn into_u32(self) -> u32 {
        0
    }

    fn into_u16(&self) -> u16 {
        0
    }

    fn to_something(self) -> u32 {
        0
    }

    fn new(self) -> Self {
        unimplemented!();
    }
}

struct Lt<'a> {
    foo: &'a u32,
}

impl<'a> Lt<'a> {
    // The lifetime is different, but that’s irrelevant; see issue #734.
    #[allow(clippy::needless_lifetimes)]
    pub fn new<'b>(s: &'b str) -> Lt<'b> {
        unimplemented!()
    }
}

struct Lt2<'a> {
    foo: &'a u32,
}

impl<'a> Lt2<'a> {
    // The lifetime is different, but that’s irrelevant; see issue #734.
    pub fn new(s: &str) -> Lt2 {
        unimplemented!()
    }
}

struct Lt3<'a> {
    foo: &'a u32,
}

impl<'a> Lt3<'a> {
    // The lifetime is different, but that’s irrelevant; see issue #734.
    pub fn new() -> Lt3<'static> {
        unimplemented!()
    }
}

#[derive(Clone, Copy)]
struct U;

impl U {
    fn new() -> Self {
        U
    }
    // Ok because `U` is `Copy`.
    fn to_something(self) -> u32 {
        0
    }
}

struct V<T> {
    _dummy: T,
}

impl<T> V<T> {
    fn new() -> Option<V<T>> {
        None
    }
}

struct AsyncNew;

impl AsyncNew {
    async fn new() -> Option<Self> {
        None
    }
}

struct BadNew;

impl BadNew {
    fn new() -> i32 {
        0
    }
}

impl Mul<T> for T {
    type Output = T;
    // No error, obviously.
    fn mul(self, other: T) -> T {
        self
    }
}

/// Checks implementation of the following lints:
/// * `OPTION_MAP_UNWRAP_OR`
/// * `OPTION_MAP_UNWRAP_OR_ELSE`
#[rustfmt::skip]
fn option_methods() {
    let opt = Some(1);

    // Check `OPTION_MAP_UNWRAP_OR`.
    // Single line case.
    let _ = opt.map(|x| x + 1)
                // Should lint even though this call is on a separate line.
               .unwrap_or(0);
    // Multi-line cases.
    let _ = opt.map(|x| {
                        x + 1
                    }
              ).unwrap_or(0);
    let _ = opt.map(|x| x + 1)
               .unwrap_or({
                    0
                });
    // Single line `map(f).unwrap_or(None)` case.
    let _ = opt.map(|x| Some(x + 1)).unwrap_or(None);
    // Multi-line `map(f).unwrap_or(None)` cases.
    let _ = opt.map(|x| {
        Some(x + 1)
    }
    ).unwrap_or(None);
    let _ = opt
        .map(|x| Some(x + 1))
        .unwrap_or(None);
    // macro case
    let _ = opt_map!(opt, |x| x + 1).unwrap_or(0); // should not lint

    // Should not lint if not copyable
    let id: String = "identifier".to_string();
    let _ = Some("prefix").map(|p| format!("{}.{}", p, id)).unwrap_or(id);
    // ...but DO lint if the `unwrap_or` argument is not used in the `map`
    let id: String = "identifier".to_string();
    let _ = Some("prefix").map(|p| format!("{}.", p)).unwrap_or(id);

    // Check OPTION_MAP_UNWRAP_OR_ELSE
    // single line case
    let _ = opt.map(|x| x + 1)
                // Should lint even though this call is on a separate line.
               .unwrap_or_else(|| 0);
    // Multi-line cases.
    let _ = opt.map(|x| {
                        x + 1
                    }
              ).unwrap_or_else(|| 0);
    let _ = opt.map(|x| x + 1)
               .unwrap_or_else(||
                    0
                );
    // Macro case.
    // Should not lint.
    let _ = opt_map!(opt, |x| x + 1).unwrap_or_else(|| 0);

    // Issue #4144
    {
        let mut frequencies = HashMap::new();
        let word = "foo";

        frequencies
            .get_mut(word)
            .map(|count| {
                *count += 1;
            })
            .unwrap_or_else(|| {
                frequencies.insert(word.to_owned(), 1);
            });
    }
}

/// Checks implementation of `FILTER_NEXT` lint.
#[rustfmt::skip]
fn filter_next() {
    let v = vec![3, 2, 1, 0, -1, -2, -3];

    // Single-line case.
    let _ = v.iter().filter(|&x| *x < 0).next();

    // Multi-line case.
    let _ = v.iter().filter(|&x| {
                                *x < 0
                            }
                   ).next();

    // Check that hat we don't lint if the caller is not an `Iterator`.
    let foo = IteratorFalsePositives { foo: 0 };
    let _ = foo.filter().next();
}

/// Checks implementation of `SEARCH_IS_SOME` lint.
#[rustfmt::skip]
fn search_is_some() {
    let v = vec![3, 2, 1, 0, -1, -2, -3];
    let y = &&42;

    // Check `find().is_some()`, single-line case.
    let _ = v.iter().find(|&x| *x < 0).is_some();
    let _ = (0..1).find(|x| **y == *x).is_some(); // one dereference less
    let _ = (0..1).find(|x| *x == 0).is_some();
    let _ = v.iter().find(|x| **x == 0).is_some();

    // Check `find().is_some()`, multi-line case.
    let _ = v.iter().find(|&x| {
                              *x < 0
                          }
                   ).is_some();

    // Check `position().is_some()`, single-line case.
    let _ = v.iter().position(|&x| x < 0).is_some();

    // Check `position().is_some()`, multi-line case.
    let _ = v.iter().position(|&x| {
                                  x < 0
                              }
                   ).is_some();

    // Check `rposition().is_some()`, single-line case.
    let _ = v.iter().rposition(|&x| x < 0).is_some();

    // Check `rposition().is_some()`, multi-line case.
    let _ = v.iter().rposition(|&x| {
                                   x < 0
                               }
                   ).is_some();

    // Check that we don't lint if the caller is not an `Iterator`.
    let foo = IteratorFalsePositives { foo: 0 };
    let _ = foo.find().is_some();
    let _ = foo.position().is_some();
    let _ = foo.rposition().is_some();
}

fn main() {
    option_methods();
    filter_next();
    search_is_some();
}
