mod inefficient_to_string;
mod manual_saturating_arithmetic;
mod option_map_unwrap_or;
mod unnecessary_filter_map;

use std::borrow::Cow;
use std::fmt;
use std::iter;

use if_chain::if_chain;
use matches::matches;
use rustc::hir;
use rustc::hir::intravisit::{self, Visitor};
use rustc::lint::{in_external_macro, LateContext, LateLintPass, Lint, LintArray, LintContext, LintPass};
use rustc::ty::{self, Predicate, Ty};
use rustc::{declare_lint_pass, declare_tool_lint};
use rustc_errors::Applicability;
use syntax::ast;
use syntax::source_map::Span;
use syntax::symbol::{sym, Symbol, SymbolStr};

use crate::utils::usage::mutated_variables;
use crate::utils::{
    get_arg_name, get_parent_expr, get_trait_def_id, has_iter_method, implements_trait, in_macro, is_copy,
    is_ctor_or_promotable_const_function, is_expn_of, is_type_diagnostic_item, iter_input_pats, last_path_segment,
    match_def_path, match_qpath, match_trait_method, match_type, match_var, method_calls, method_chain_args, paths,
    remove_blocks, return_ty, same_tys, single_segment_path, snippet, snippet_with_applicability,
    snippet_with_macro_callsite, span_help_and_lint, span_lint, span_lint_and_sugg, span_lint_and_then,
    span_note_and_lint, sugg, walk_ptrs_ty, walk_ptrs_ty_depth, SpanlessEq,
};

declare_clippy_lint! {
    /// **What it does:** Checks for `.unwrap()` calls on `Option`s.
    ///
    /// **Why is this bad?** Usually it is better to handle the `None` case, or to
    /// at least call `.expect(_)` with a more helpful message. Still, for a lot of
    /// quick-and-dirty code, `unwrap` is a good choice, which is why this lint is
    /// `Allow` by default.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    ///
    /// Using unwrap on an `Option`:
    ///
    /// ```rust
    /// let opt = Some(1);
    /// opt.unwrap();
    /// ```
    ///
    /// Better:
    ///
    /// ```rust
    /// let opt = Some(1);
    /// opt.expect("more helpful message");
    /// ```
    pub OPTION_UNWRAP_USED,
    restriction,
    "using `Option.unwrap()`, which should at least get a better message using `expect()`"
}

declare_clippy_lint! {
    /// **What it does:** Checks for `.unwrap()` calls on `Result`s.
    ///
    /// **Why is this bad?** `result.unwrap()` will let the thread panic on `Err`
    /// values. Normally, you want to implement more sophisticated error handling,
    /// and propagate errors upwards with `try!`.
    ///
    /// Even if you want to panic on errors, not all `Error`s implement good
    /// messages on display. Therefore, it may be beneficial to look at the places
    /// where they may get displayed. Activate this lint to do just that.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// Using unwrap on an `Result`:
    ///
    /// ```rust
    /// let res: Result<usize, ()> = Ok(1);
    /// res.unwrap();
    /// ```
    ///
    /// Better:
    ///
    /// ```rust
    /// let res: Result<usize, ()> = Ok(1);
    /// res.expect("more helpful message");
    /// ```
    pub RESULT_UNWRAP_USED,
    restriction,
    "using `Result.unwrap()`, which might be better handled"
}

declare_clippy_lint! {
    /// **What it does:** Checks for `.expect()` calls on `Option`s.
    ///
    /// **Why is this bad?** Usually it is better to handle the `None` case. Still,
    ///  for a lot of quick-and-dirty code, `expect` is a good choice, which is why
    ///  this lint is `Allow` by default.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    ///
    /// Using expect on an `Option`:
    ///
    /// ```rust
    /// let opt = Some(1);
    /// opt.expect("one");
    /// ```
    ///
    /// Better:
    ///
    /// ```ignore
    /// let opt = Some(1);
    /// opt?;
    /// # Some::<()>(())
    /// ```
    pub OPTION_EXPECT_USED,
    restriction,
    "using `Option.expect()`, which might be better handled"
}

declare_clippy_lint! {
    /// **What it does:** Checks for `.expect()` calls on `Result`s.
    ///
    /// **Why is this bad?** `result.expect()` will let the thread panic on `Err`
    /// values. Normally, you want to implement more sophisticated error handling,
    /// and propagate errors upwards with `try!`.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// Using expect on an `Result`:
    ///
    /// ```rust
    /// let res: Result<usize, ()> = Ok(1);
    /// res.expect("one");
    /// ```
    ///
    /// Better:
    ///
    /// ```
    /// let res: Result<usize, ()> = Ok(1);
    /// res?;
    /// # Ok::<(), ()>(())
    /// ```
    pub RESULT_EXPECT_USED,
    restriction,
    "using `Result.expect()`, which might be better handled"
}

declare_clippy_lint! {
    /// **What it does:** Checks for methods that should live in a trait
    /// implementation of a `std` trait (see [llogiq's blog
    /// post](http://llogiq.github.io/2015/07/30/traits.html) for further
    /// information) instead of an inherent implementation.
    ///
    /// **Why is this bad?** Implementing the traits improve ergonomics for users of
    /// the code, often with very little cost. Also people seeing a `mul(...)`
    /// method
    /// may expect `*` to work equally, so you should have good reason to disappoint
    /// them.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```ignore
    /// struct X;
    /// impl X {
    ///     fn add(&self, other: &X) -> X {
    ///         ..
    ///     }
    /// }
    /// ```
    pub SHOULD_IMPLEMENT_TRAIT,
    style,
    "defining a method that should be implementing a std trait"
}

declare_clippy_lint! {
    /// **What it does:** Checks for methods with certain name prefixes and which
    /// doesn't match how self is taken. The actual rules are:
    ///
    /// |Prefix |`self` taken          |
    /// |-------|----------------------|
    /// |`as_`  |`&self` or `&mut self`|
    /// |`from_`| none                 |
    /// |`into_`|`self`                |
    /// |`is_`  |`&self` or none       |
    /// |`to_`  |`&self`               |
    ///
    /// **Why is this bad?** Consistency breeds readability. If you follow the
    /// conventions, your users won't be surprised that they, e.g., need to supply a
    /// mutable reference to a `as_..` function.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```ignore
    /// impl X {
    ///     fn as_str(self) -> &str {
    ///         ..
    ///     }
    /// }
    /// ```
    pub WRONG_SELF_CONVENTION,
    style,
    "defining a method named with an established prefix (like \"into_\") that takes `self` with the wrong convention"
}

declare_clippy_lint! {
    /// **What it does:** This is the same as
    /// [`wrong_self_convention`](#wrong_self_convention), but for public items.
    ///
    /// **Why is this bad?** See [`wrong_self_convention`](#wrong_self_convention).
    ///
    /// **Known problems:** Actually *renaming* the function may break clients if
    /// the function is part of the public interface. In that case, be mindful of
    /// the stability guarantees you've given your users.
    ///
    /// **Example:**
    /// ```rust
    /// # struct X;
    /// impl<'a> X {
    ///     pub fn as_str(self) -> &'a str {
    ///         "foo"
    ///     }
    /// }
    /// ```
    pub WRONG_PUB_SELF_CONVENTION,
    restriction,
    "defining a public method named with an established prefix (like \"into_\") that takes `self` with the wrong convention"
}

declare_clippy_lint! {
    /// **What it does:** Checks for usage of `ok().expect(..)`.
    ///
    /// **Why is this bad?** Because you usually call `expect()` on the `Result`
    /// directly to get a better error message.
    ///
    /// **Known problems:** The error type needs to implement `Debug`
    ///
    /// **Example:**
    /// ```ignore
    /// x.ok().expect("why did I do this again?")
    /// ```
    pub OK_EXPECT,
    style,
    "using `ok().expect()`, which gives worse error messages than calling `expect` directly on the Result"
}

declare_clippy_lint! {
    /// **What it does:** Checks for usage of `_.map(_).unwrap_or(_)`.
    ///
    /// **Why is this bad?** Readability, this can be written more concisely as
    /// `_.map_or(_, _)`.
    ///
    /// **Known problems:** The order of the arguments is not in execution order
    ///
    /// **Example:**
    /// ```rust
    /// # let x = Some(1);
    /// x.map(|a| a + 1).unwrap_or(0);
    /// ```
    pub OPTION_MAP_UNWRAP_OR,
    pedantic,
    "using `Option.map(f).unwrap_or(a)`, which is more succinctly expressed as `map_or(a, f)`"
}

declare_clippy_lint! {
    /// **What it does:** Checks for usage of `_.map(_).unwrap_or_else(_)`.
    ///
    /// **Why is this bad?** Readability, this can be written more concisely as
    /// `_.map_or_else(_, _)`.
    ///
    /// **Known problems:** The order of the arguments is not in execution order.
    ///
    /// **Example:**
    /// ```rust
    /// # let x = Some(1);
    /// # fn some_function() -> usize { 1 }
    /// x.map(|a| a + 1).unwrap_or_else(some_function);
    /// ```
    pub OPTION_MAP_UNWRAP_OR_ELSE,
    pedantic,
    "using `Option.map(f).unwrap_or_else(g)`, which is more succinctly expressed as `map_or_else(g, f)`"
}

declare_clippy_lint! {
    /// **What it does:** Checks for usage of `result.map(_).unwrap_or_else(_)`.
    ///
    /// **Why is this bad?** Readability, this can be written more concisely as
    /// `result.ok().map_or_else(_, _)`.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```rust
    /// # let x: Result<usize, ()> = Ok(1);
    /// # fn some_function(foo: ()) -> usize { 1 }
    /// x.map(|a| a + 1).unwrap_or_else(some_function);
    /// ```
    pub RESULT_MAP_UNWRAP_OR_ELSE,
    pedantic,
    "using `Result.map(f).unwrap_or_else(g)`, which is more succinctly expressed as `.ok().map_or_else(g, f)`"
}

declare_clippy_lint! {
    /// **What it does:** Checks for usage of `_.map_or(None, _)`.
    ///
    /// **Why is this bad?** Readability, this can be written more concisely as
    /// `_.and_then(_)`.
    ///
    /// **Known problems:** The order of the arguments is not in execution order.
    ///
    /// **Example:**
    /// ```ignore
    /// opt.map_or(None, |a| a + 1)
    /// ```
    pub OPTION_MAP_OR_NONE,
    style,
    "using `Option.map_or(None, f)`, which is more succinctly expressed as `and_then(f)`"
}

declare_clippy_lint! {
    /// **What it does:** Checks for usage of `_.and_then(|x| Some(y))`.
    ///
    /// **Why is this bad?** Readability, this can be written more concisely as
    /// `_.map(|x| y)`.
    ///
    /// **Known problems:** None
    ///
    /// **Example:**
    ///
    /// ```rust
    /// let x = Some("foo");
    /// let _ = x.and_then(|s| Some(s.len()));
    /// ```
    ///
    /// The correct use would be:
    ///
    /// ```rust
    /// let x = Some("foo");
    /// let _ = x.map(|s| s.len());
    /// ```
    pub OPTION_AND_THEN_SOME,
    complexity,
    "using `Option.and_then(|x| Some(y))`, which is more succinctly expressed as `map(|x| y)`"
}

declare_clippy_lint! {
    /// **What it does:** Checks for usage of `_.filter(_).next()`.
    ///
    /// **Why is this bad?** Readability, this can be written more concisely as
    /// `_.find(_)`.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```rust
    /// # let vec = vec![1];
    /// vec.iter().filter(|x| **x == 0).next();
    /// ```
    /// Could be written as
    /// ```rust
    /// # let vec = vec![1];
    /// vec.iter().find(|x| **x == 0);
    /// ```
    pub FILTER_NEXT,
    complexity,
    "using `filter(p).next()`, which is more succinctly expressed as `.find(p)`"
}

declare_clippy_lint! {
    /// **What it does:** Checks for usage of `_.map(_).flatten(_)`,
    ///
    /// **Why is this bad?** Readability, this can be written more concisely as a
    /// single method call.
    ///
    /// **Known problems:**
    ///
    /// **Example:**
    /// ```rust
    /// let vec = vec![vec![1]];
    /// vec.iter().map(|x| x.iter()).flatten();
    /// ```
    pub MAP_FLATTEN,
    pedantic,
    "using combinations of `flatten` and `map` which can usually be written as a single method call"
}

declare_clippy_lint! {
    /// **What it does:** Checks for usage of `_.filter(_).map(_)`,
    /// `_.filter(_).flat_map(_)`, `_.filter_map(_).flat_map(_)` and similar.
    ///
    /// **Why is this bad?** Readability, this can be written more concisely as a
    /// single method call.
    ///
    /// **Known problems:** Often requires a condition + Option/Iterator creation
    /// inside the closure.
    ///
    /// **Example:**
    /// ```rust
    /// let vec = vec![1];
    /// vec.iter().filter(|x| **x == 0).map(|x| *x * 2);
    /// ```
    pub FILTER_MAP,
    pedantic,
    "using combinations of `filter`, `map`, `filter_map` and `flat_map` which can usually be written as a single method call"
}

declare_clippy_lint! {
    /// **What it does:** Checks for usage of `_.filter_map(_).next()`.
    ///
    /// **Why is this bad?** Readability, this can be written more concisely as a
    /// single method call.
    ///
    /// **Known problems:** None
    ///
    /// **Example:**
    /// ```rust
    ///  (0..3).filter_map(|x| if x == 2 { Some(x) } else { None }).next();
    /// ```
    /// Can be written as
    ///
    /// ```rust
    ///  (0..3).find_map(|x| if x == 2 { Some(x) } else { None });
    /// ```
    pub FILTER_MAP_NEXT,
    pedantic,
    "using combination of `filter_map` and `next` which can usually be written as a single method call"
}

declare_clippy_lint! {
    /// **What it does:** Checks for usage of `flat_map(|x| x)`.
    ///
    /// **Why is this bad?** Readability, this can be written more concisely by using `flatten`.
    ///
    /// **Known problems:** None
    ///
    /// **Example:**
    /// ```rust
    /// # let iter = vec![vec![0]].into_iter();
    /// iter.flat_map(|x| x);
    /// ```
    /// Can be written as
    /// ```rust
    /// # let iter = vec![vec![0]].into_iter();
    /// iter.flatten();
    /// ```
    pub FLAT_MAP_IDENTITY,
    complexity,
    "call to `flat_map` where `flatten` is sufficient"
}

declare_clippy_lint! {
    /// **What it does:** Checks for usage of `_.find(_).map(_)`.
    ///
    /// **Why is this bad?** Readability, this can be written more concisely as a
    /// single method call.
    ///
    /// **Known problems:** Often requires a condition + Option/Iterator creation
    /// inside the closure.
    ///
    /// **Example:**
    /// ```rust
    ///  (0..3).find(|x| *x == 2).map(|x| x * 2);
    /// ```
    /// Can be written as
    /// ```rust
    ///  (0..3).find_map(|x| if x == 2 { Some(x * 2) } else { None });
    /// ```
    pub FIND_MAP,
    pedantic,
    "using a combination of `find` and `map` can usually be written as a single method call"
}

declare_clippy_lint! {
    /// **What it does:** Checks for an iterator search (such as `find()`,
    /// `position()`, or `rposition()`) followed by a call to `is_some()`.
    ///
    /// **Why is this bad?** Readability, this can be written more concisely as
    /// `_.any(_)`.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```rust
    /// # let vec = vec![1];
    /// vec.iter().find(|x| **x == 0).is_some();
    /// ```
    /// Could be written as
    /// ```rust
    /// # let vec = vec![1];
    /// vec.iter().any(|x| *x == 0);
    /// ```
    pub SEARCH_IS_SOME,
    complexity,
    "using an iterator search followed by `is_some()`, which is more succinctly expressed as a call to `any()`"
}

declare_clippy_lint! {
    /// **What it does:** Checks for usage of `.chars().next()` on a `str` to check
    /// if it starts with a given char.
    ///
    /// **Why is this bad?** Readability, this can be written more concisely as
    /// `_.starts_with(_)`.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```rust
    /// let name = "foo";
    /// if name.chars().next() == Some('_') {};
    /// ```
    /// Could be written as
    /// ```rust
    /// let name = "foo";
    /// if name.starts_with('_') {};
    /// ```
    pub CHARS_NEXT_CMP,
    complexity,
    "using `.chars().next()` to check if a string starts with a char"
}

declare_clippy_lint! {
    /// **What it does:** Checks for calls to `.or(foo(..))`, `.unwrap_or(foo(..))`,
    /// etc., and suggests to use `or_else`, `unwrap_or_else`, etc., or
    /// `unwrap_or_default` instead.
    ///
    /// **Why is this bad?** The function will always be called and potentially
    /// allocate an object acting as the default.
    ///
    /// **Known problems:** If the function has side-effects, not calling it will
    /// change the semantic of the program, but you shouldn't rely on that anyway.
    ///
    /// **Example:**
    /// ```rust
    /// # let foo = Some(String::new());
    /// foo.unwrap_or(String::new());
    /// ```
    /// this can instead be written:
    /// ```rust
    /// # let foo = Some(String::new());
    /// foo.unwrap_or_else(String::new);
    /// ```
    /// or
    /// ```rust
    /// # let foo = Some(String::new());
    /// foo.unwrap_or_default();
    /// ```
    pub OR_FUN_CALL,
    perf,
    "using any `*or` method with a function call, which suggests `*or_else`"
}

declare_clippy_lint! {
    /// **What it does:** Checks for calls to `.expect(&format!(...))`, `.expect(foo(..))`,
    /// etc., and suggests to use `unwrap_or_else` instead
    ///
    /// **Why is this bad?** The function will always be called.
    ///
    /// **Known problems:** If the function has side-effects, not calling it will
    /// change the semantics of the program, but you shouldn't rely on that anyway.
    ///
    /// **Example:**
    /// ```rust
    /// # let foo = Some(String::new());
    /// # let err_code = "418";
    /// # let err_msg = "I'm a teapot";
    /// foo.expect(&format!("Err {}: {}", err_code, err_msg));
    /// ```
    /// or
    /// ```rust
    /// # let foo = Some(String::new());
    /// # let err_code = "418";
    /// # let err_msg = "I'm a teapot";
    /// foo.expect(format!("Err {}: {}", err_code, err_msg).as_str());
    /// ```
    /// this can instead be written:
    /// ```rust
    /// # let foo = Some(String::new());
    /// # let err_code = "418";
    /// # let err_msg = "I'm a teapot";
    /// foo.unwrap_or_else(|| panic!("Err {}: {}", err_code, err_msg));
    /// ```
    pub EXPECT_FUN_CALL,
    perf,
    "using any `expect` method with a function call"
}

declare_clippy_lint! {
    /// **What it does:** Checks for usage of `.clone()` on a `Copy` type.
    ///
    /// **Why is this bad?** The only reason `Copy` types implement `Clone` is for
    /// generics, not for using the `clone` method on a concrete type.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```rust
    /// 42u64.clone();
    /// ```
    pub CLONE_ON_COPY,
    complexity,
    "using `clone` on a `Copy` type"
}

declare_clippy_lint! {
    /// **What it does:** Checks for usage of `.clone()` on a ref-counted pointer,
    /// (`Rc`, `Arc`, `rc::Weak`, or `sync::Weak`), and suggests calling Clone via unified
    /// function syntax instead (e.g., `Rc::clone(foo)`).
    ///
    /// **Why is this bad?** Calling '.clone()' on an Rc, Arc, or Weak
    /// can obscure the fact that only the pointer is being cloned, not the underlying
    /// data.
    ///
    /// **Example:**
    /// ```rust
    /// # use std::rc::Rc;
    /// let x = Rc::new(1);
    /// x.clone();
    /// ```
    pub CLONE_ON_REF_PTR,
    restriction,
    "using 'clone' on a ref-counted pointer"
}

declare_clippy_lint! {
    /// **What it does:** Checks for usage of `.clone()` on an `&&T`.
    ///
    /// **Why is this bad?** Cloning an `&&T` copies the inner `&T`, instead of
    /// cloning the underlying `T`.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```rust
    /// fn main() {
    ///     let x = vec![1];
    ///     let y = &&x;
    ///     let z = y.clone();
    ///     println!("{:p} {:p}", *y, z); // prints out the same pointer
    /// }
    /// ```
    pub CLONE_DOUBLE_REF,
    correctness,
    "using `clone` on `&&T`"
}

declare_clippy_lint! {
    /// **What it does:** Checks for usage of `.to_string()` on an `&&T` where
    /// `T` implements `ToString` directly (like `&&str` or `&&String`).
    ///
    /// **Why is this bad?** This bypasses the specialized implementation of
    /// `ToString` and instead goes through the more expensive string formatting
    /// facilities.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```rust
    /// // Generic implementation for `T: Display` is used (slow)
    /// ["foo", "bar"].iter().map(|s| s.to_string());
    ///
    /// // OK, the specialized impl is used
    /// ["foo", "bar"].iter().map(|&s| s.to_string());
    /// ```
    pub INEFFICIENT_TO_STRING,
    perf,
    "using `to_string` on `&&T` where `T: ToString`"
}

declare_clippy_lint! {
    /// **What it does:** Checks for `new` not returning `Self`.
    ///
    /// **Why is this bad?** As a convention, `new` methods are used to make a new
    /// instance of a type.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```ignore
    /// impl Foo {
    ///     fn new(..) -> NotAFoo {
    ///     }
    /// }
    /// ```
    pub NEW_RET_NO_SELF,
    style,
    "not returning `Self` in a `new` method"
}

declare_clippy_lint! {
    /// **What it does:** Checks for string methods that receive a single-character
    /// `str` as an argument, e.g., `_.split("x")`.
    ///
    /// **Why is this bad?** Performing these methods using a `char` is faster than
    /// using a `str`.
    ///
    /// **Known problems:** Does not catch multi-byte unicode characters.
    ///
    /// **Example:**
    /// `_.split("x")` could be `_.split('x')`
    pub SINGLE_CHAR_PATTERN,
    perf,
    "using a single-character str where a char could be used, e.g., `_.split(\"x\")`"
}

declare_clippy_lint! {
    /// **What it does:** Checks for getting the inner pointer of a temporary
    /// `CString`.
    ///
    /// **Why is this bad?** The inner pointer of a `CString` is only valid as long
    /// as the `CString` is alive.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```rust,ignore
    /// let c_str = CString::new("foo").unwrap().as_ptr();
    /// unsafe {
    ///     call_some_ffi_func(c_str);
    /// }
    /// ```
    /// Here `c_str` point to a freed address. The correct use would be:
    /// ```rust,ignore
    /// let c_str = CString::new("foo").unwrap();
    /// unsafe {
    ///     call_some_ffi_func(c_str.as_ptr());
    /// }
    /// ```
    pub TEMPORARY_CSTRING_AS_PTR,
    correctness,
    "getting the inner pointer of a temporary `CString`"
}

declare_clippy_lint! {
    /// **What it does:** Checks for use of `.iter().nth()` (and the related
    /// `.iter_mut().nth()`) on standard library types with O(1) element access.
    ///
    /// **Why is this bad?** `.get()` and `.get_mut()` are more efficient and more
    /// readable.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```rust
    /// let some_vec = vec![0, 1, 2, 3];
    /// let bad_vec = some_vec.iter().nth(3);
    /// let bad_slice = &some_vec[..].iter().nth(3);
    /// ```
    /// The correct use would be:
    /// ```rust
    /// let some_vec = vec![0, 1, 2, 3];
    /// let bad_vec = some_vec.get(3);
    /// let bad_slice = &some_vec[..].get(3);
    /// ```
    pub ITER_NTH,
    perf,
    "using `.iter().nth()` on a standard library type with O(1) element access"
}

declare_clippy_lint! {
    /// **What it does:** Checks for use of `.skip(x).next()` on iterators.
    ///
    /// **Why is this bad?** `.nth(x)` is cleaner
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```rust
    /// let some_vec = vec![0, 1, 2, 3];
    /// let bad_vec = some_vec.iter().skip(3).next();
    /// let bad_slice = &some_vec[..].iter().skip(3).next();
    /// ```
    /// The correct use would be:
    /// ```rust
    /// let some_vec = vec![0, 1, 2, 3];
    /// let bad_vec = some_vec.iter().nth(3);
    /// let bad_slice = &some_vec[..].iter().nth(3);
    /// ```
    pub ITER_SKIP_NEXT,
    style,
    "using `.skip(x).next()` on an iterator"
}

declare_clippy_lint! {
    /// **What it does:** Checks for use of `.get().unwrap()` (or
    /// `.get_mut().unwrap`) on a standard library type which implements `Index`
    ///
    /// **Why is this bad?** Using the Index trait (`[]`) is more clear and more
    /// concise.
    ///
    /// **Known problems:** Not a replacement for error handling: Using either
    /// `.unwrap()` or the Index trait (`[]`) carries the risk of causing a `panic`
    /// if the value being accessed is `None`. If the use of `.get().unwrap()` is a
    /// temporary placeholder for dealing with the `Option` type, then this does
    /// not mitigate the need for error handling. If there is a chance that `.get()`
    /// will be `None` in your program, then it is advisable that the `None` case
    /// is handled in a future refactor instead of using `.unwrap()` or the Index
    /// trait.
    ///
    /// **Example:**
    /// ```rust
    /// let mut some_vec = vec![0, 1, 2, 3];
    /// let last = some_vec.get(3).unwrap();
    /// *some_vec.get_mut(0).unwrap() = 1;
    /// ```
    /// The correct use would be:
    /// ```rust
    /// let mut some_vec = vec![0, 1, 2, 3];
    /// let last = some_vec[3];
    /// some_vec[0] = 1;
    /// ```
    pub GET_UNWRAP,
    restriction,
    "using `.get().unwrap()` or `.get_mut().unwrap()` when using `[]` would work instead"
}

declare_clippy_lint! {
    /// **What it does:** Checks for the use of `.extend(s.chars())` where s is a
    /// `&str` or `String`.
    ///
    /// **Why is this bad?** `.push_str(s)` is clearer
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```rust
    /// let abc = "abc";
    /// let def = String::from("def");
    /// let mut s = String::new();
    /// s.extend(abc.chars());
    /// s.extend(def.chars());
    /// ```
    /// The correct use would be:
    /// ```rust
    /// let abc = "abc";
    /// let def = String::from("def");
    /// let mut s = String::new();
    /// s.push_str(abc);
    /// s.push_str(&def);
    /// ```
    pub STRING_EXTEND_CHARS,
    style,
    "using `x.extend(s.chars())` where s is a `&str` or `String`"
}

declare_clippy_lint! {
    /// **What it does:** Checks for the use of `.cloned().collect()` on slice to
    /// create a `Vec`.
    ///
    /// **Why is this bad?** `.to_vec()` is clearer
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```rust
    /// let s = [1, 2, 3, 4, 5];
    /// let s2: Vec<isize> = s[..].iter().cloned().collect();
    /// ```
    /// The better use would be:
    /// ```rust
    /// let s = [1, 2, 3, 4, 5];
    /// let s2: Vec<isize> = s.to_vec();
    /// ```
    pub ITER_CLONED_COLLECT,
    style,
    "using `.cloned().collect()` on slice to create a `Vec`"
}

declare_clippy_lint! {
    /// **What it does:** Checks for usage of `.chars().last()` or
    /// `.chars().next_back()` on a `str` to check if it ends with a given char.
    ///
    /// **Why is this bad?** Readability, this can be written more concisely as
    /// `_.ends_with(_)`.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```ignore
    /// name.chars().last() == Some('_') || name.chars().next_back() == Some('-')
    /// ```
    pub CHARS_LAST_CMP,
    style,
    "using `.chars().last()` or `.chars().next_back()` to check if a string ends with a char"
}

declare_clippy_lint! {
    /// **What it does:** Checks for usage of `.as_ref()` or `.as_mut()` where the
    /// types before and after the call are the same.
    ///
    /// **Why is this bad?** The call is unnecessary.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```rust
    /// # fn do_stuff(x: &[i32]) {}
    /// let x: &[i32] = &[1, 2, 3, 4, 5];
    /// do_stuff(x.as_ref());
    /// ```
    /// The correct use would be:
    /// ```rust
    /// # fn do_stuff(x: &[i32]) {}
    /// let x: &[i32] = &[1, 2, 3, 4, 5];
    /// do_stuff(x);
    /// ```
    pub USELESS_ASREF,
    complexity,
    "using `as_ref` where the types before and after the call are the same"
}

declare_clippy_lint! {
    /// **What it does:** Checks for using `fold` when a more succinct alternative exists.
    /// Specifically, this checks for `fold`s which could be replaced by `any`, `all`,
    /// `sum` or `product`.
    ///
    /// **Why is this bad?** Readability.
    ///
    /// **Known problems:** False positive in pattern guards. Will be resolved once
    /// non-lexical lifetimes are stable.
    ///
    /// **Example:**
    /// ```rust
    /// let _ = (0..3).fold(false, |acc, x| acc || x > 2);
    /// ```
    /// This could be written as:
    /// ```rust
    /// let _ = (0..3).any(|x| x > 2);
    /// ```
    pub UNNECESSARY_FOLD,
    style,
    "using `fold` when a more succinct alternative exists"
}

declare_clippy_lint! {
    /// **What it does:** Checks for `filter_map` calls which could be replaced by `filter` or `map`.
    /// More specifically it checks if the closure provided is only performing one of the
    /// filter or map operations and suggests the appropriate option.
    ///
    /// **Why is this bad?** Complexity. The intent is also clearer if only a single
    /// operation is being performed.
    ///
    /// **Known problems:** None
    ///
    /// **Example:**
    /// ```rust
    /// let _ = (0..3).filter_map(|x| if x > 2 { Some(x) } else { None });
    /// ```
    /// As there is no transformation of the argument this could be written as:
    /// ```rust
    /// let _ = (0..3).filter(|&x| x > 2);
    /// ```
    ///
    /// ```rust
    /// let _ = (0..4).filter_map(i32::checked_abs);
    /// ```
    /// As there is no conditional check on the argument this could be written as:
    /// ```rust
    /// let _ = (0..4).map(i32::checked_abs);
    /// ```
    pub UNNECESSARY_FILTER_MAP,
    complexity,
    "using `filter_map` when a more succinct alternative exists"
}

declare_clippy_lint! {
    /// **What it does:** Checks for `into_iter` calls on references which should be replaced by `iter`
    /// or `iter_mut`.
    ///
    /// **Why is this bad?** Readability. Calling `into_iter` on a reference will not move out its
    /// content into the resulting iterator, which is confusing. It is better just call `iter` or
    /// `iter_mut` directly.
    ///
    /// **Known problems:** None
    ///
    /// **Example:**
    ///
    /// ```rust
    /// let _ = (&vec![3, 4, 5]).into_iter();
    /// ```
    pub INTO_ITER_ON_REF,
    style,
    "using `.into_iter()` on a reference"
}

declare_clippy_lint! {
    /// **What it does:** Checks for calls to `map` followed by a `count`.
    ///
    /// **Why is this bad?** It looks suspicious. Maybe `map` was confused with `filter`.
    /// If the `map` call is intentional, this should be rewritten.
    ///
    /// **Known problems:** None
    ///
    /// **Example:**
    ///
    /// ```rust
    /// let _ = (0..3).map(|x| x + 2).count();
    /// ```
    pub SUSPICIOUS_MAP,
    complexity,
    "suspicious usage of map"
}

declare_clippy_lint! {
    /// **What it does:** Checks for `MaybeUninit::uninit().assume_init()`.
    ///
    /// **Why is this bad?** For most types, this is undefined behavior.
    ///
    /// **Known problems:** For now, we accept empty tuples and tuples / arrays
    /// of `MaybeUninit`. There may be other types that allow uninitialized
    /// data, but those are not yet rigorously defined.
    ///
    /// **Example:**
    ///
    /// ```rust
    /// // Beware the UB
    /// use std::mem::MaybeUninit;
    ///
    /// let _: usize = unsafe { MaybeUninit::uninit().assume_init() };
    /// ```
    ///
    /// Note that the following is OK:
    ///
    /// ```rust
    /// use std::mem::MaybeUninit;
    ///
    /// let _: [MaybeUninit<bool>; 5] = unsafe {
    ///     MaybeUninit::uninit().assume_init()
    /// };
    /// ```
    pub UNINIT_ASSUMED_INIT,
    correctness,
    "`MaybeUninit::uninit().assume_init()`"
}

declare_clippy_lint! {
    /// **What it does:** Checks for `.checked_add/sub(x).unwrap_or(MAX/MIN)`.
    ///
    /// **Why is this bad?** These can be written simply with `saturating_add/sub` methods.
    ///
    /// **Example:**
    ///
    /// ```rust
    /// # let y: u32 = 0;
    /// # let x: u32 = 100;
    /// let add = x.checked_add(y).unwrap_or(u32::max_value());
    /// let sub = x.checked_sub(y).unwrap_or(u32::min_value());
    /// ```
    ///
    /// can be written using dedicated methods for saturating addition/subtraction as:
    ///
    /// ```rust
    /// # let y: u32 = 0;
    /// # let x: u32 = 100;
    /// let add = x.saturating_add(y);
    /// let sub = x.saturating_sub(y);
    /// ```
    pub MANUAL_SATURATING_ARITHMETIC,
    style,
    "`.chcked_add/sub(x).unwrap_or(MAX/MIN)`"
}

declare_lint_pass!(Methods => [
    OPTION_UNWRAP_USED,
    RESULT_UNWRAP_USED,
    OPTION_EXPECT_USED,
    RESULT_EXPECT_USED,
    SHOULD_IMPLEMENT_TRAIT,
    WRONG_SELF_CONVENTION,
    WRONG_PUB_SELF_CONVENTION,
    OK_EXPECT,
    OPTION_MAP_UNWRAP_OR,
    OPTION_MAP_UNWRAP_OR_ELSE,
    RESULT_MAP_UNWRAP_OR_ELSE,
    OPTION_MAP_OR_NONE,
    OPTION_AND_THEN_SOME,
    OR_FUN_CALL,
    EXPECT_FUN_CALL,
    CHARS_NEXT_CMP,
    CHARS_LAST_CMP,
    CLONE_ON_COPY,
    CLONE_ON_REF_PTR,
    CLONE_DOUBLE_REF,
    INEFFICIENT_TO_STRING,
    NEW_RET_NO_SELF,
    SINGLE_CHAR_PATTERN,
    SEARCH_IS_SOME,
    TEMPORARY_CSTRING_AS_PTR,
    FILTER_NEXT,
    FILTER_MAP,
    FILTER_MAP_NEXT,
    FLAT_MAP_IDENTITY,
    FIND_MAP,
    MAP_FLATTEN,
    ITER_NTH,
    ITER_SKIP_NEXT,
    GET_UNWRAP,
    STRING_EXTEND_CHARS,
    ITER_CLONED_COLLECT,
    USELESS_ASREF,
    UNNECESSARY_FOLD,
    UNNECESSARY_FILTER_MAP,
    INTO_ITER_ON_REF,
    SUSPICIOUS_MAP,
    UNINIT_ASSUMED_INIT,
    MANUAL_SATURATING_ARITHMETIC,
]);

impl<'a, 'tcx> LateLintPass<'a, 'tcx> for Methods {
    #[allow(clippy::cognitive_complexity)]
    fn check_expr(&mut self, cx: &LateContext<'a, 'tcx>, expr: &'tcx hir::Expr) {
        if in_macro(expr.span) {
            return;
        }

        let (method_names, arg_lists, method_spans) = method_calls(expr, 2);
        let method_names: Vec<SymbolStr> = method_names.iter().map(|s| s.as_str()).collect();
        let method_names: Vec<&str> = method_names.iter().map(|s| &**s).collect();

        match method_names.as_slice() {
            ["unwrap", "get"] => lint_get_unwrap(cx, expr, arg_lists[1], false),
            ["unwrap", "get_mut"] => lint_get_unwrap(cx, expr, arg_lists[1], true),
            ["unwrap", ..] => lint_unwrap(cx, expr, arg_lists[0]),
            ["expect", "ok"] => lint_ok_expect(cx, expr, arg_lists[1]),
            ["expect", ..] => lint_expect(cx, expr, arg_lists[0]),
            ["unwrap_or", "map"] => option_map_unwrap_or::lint(cx, expr, arg_lists[1], arg_lists[0]),
            ["unwrap_or_else", "map"] => lint_map_unwrap_or_else(cx, expr, arg_lists[1], arg_lists[0]),
            ["map_or", ..] => lint_map_or_none(cx, expr, arg_lists[0]),
            ["and_then", ..] => lint_option_and_then_some(cx, expr, arg_lists[0]),
            ["next", "filter"] => lint_filter_next(cx, expr, arg_lists[1]),
            ["map", "filter"] => lint_filter_map(cx, expr, arg_lists[1], arg_lists[0]),
            ["map", "filter_map"] => lint_filter_map_map(cx, expr, arg_lists[1], arg_lists[0]),
            ["next", "filter_map"] => lint_filter_map_next(cx, expr, arg_lists[1]),
            ["map", "find"] => lint_find_map(cx, expr, arg_lists[1], arg_lists[0]),
            ["flat_map", "filter"] => lint_filter_flat_map(cx, expr, arg_lists[1], arg_lists[0]),
            ["flat_map", "filter_map"] => lint_filter_map_flat_map(cx, expr, arg_lists[1], arg_lists[0]),
            ["flat_map", ..] => lint_flat_map_identity(cx, expr, arg_lists[0], method_spans[0]),
            ["flatten", "map"] => lint_map_flatten(cx, expr, arg_lists[1]),
            ["is_some", "find"] => lint_search_is_some(cx, expr, "find", arg_lists[1], arg_lists[0], method_spans[1]),
            ["is_some", "position"] => {
                lint_search_is_some(cx, expr, "position", arg_lists[1], arg_lists[0], method_spans[1])
            },
            ["is_some", "rposition"] => {
                lint_search_is_some(cx, expr, "rposition", arg_lists[1], arg_lists[0], method_spans[1])
            },
            ["extend", ..] => lint_extend(cx, expr, arg_lists[0]),
            ["as_ptr", "unwrap"] | ["as_ptr", "expect"] => {
                lint_cstring_as_ptr(cx, expr, &arg_lists[1][0], &arg_lists[0][0])
            },
            ["nth", "iter"] => lint_iter_nth(cx, expr, arg_lists[1], false),
            ["nth", "iter_mut"] => lint_iter_nth(cx, expr, arg_lists[1], true),
            ["next", "skip"] => lint_iter_skip_next(cx, expr),
            ["collect", "cloned"] => lint_iter_cloned_collect(cx, expr, arg_lists[1]),
            ["as_ref"] => lint_asref(cx, expr, "as_ref", arg_lists[0]),
            ["as_mut"] => lint_asref(cx, expr, "as_mut", arg_lists[0]),
            ["fold", ..] => lint_unnecessary_fold(cx, expr, arg_lists[0], method_spans[0]),
            ["filter_map", ..] => unnecessary_filter_map::lint(cx, expr, arg_lists[0]),
            ["count", "map"] => lint_suspicious_map(cx, expr),
            ["assume_init"] => lint_maybe_uninit(cx, &arg_lists[0][0], expr),
            ["unwrap_or", arith @ "checked_add"]
            | ["unwrap_or", arith @ "checked_sub"]
            | ["unwrap_or", arith @ "checked_mul"] => {
                manual_saturating_arithmetic::lint(cx, expr, &arg_lists, &arith["checked_".len()..])
            },
            _ => {},
        }

        match expr.kind {
            hir::ExprKind::MethodCall(ref method_call, ref method_span, ref args) => {
                lint_or_fun_call(cx, expr, *method_span, &method_call.ident.as_str(), args);
                lint_expect_fun_call(cx, expr, *method_span, &method_call.ident.as_str(), args);

                let self_ty = cx.tables.expr_ty_adjusted(&args[0]);
                if args.len() == 1 && method_call.ident.name == sym!(clone) {
                    lint_clone_on_copy(cx, expr, &args[0], self_ty);
                    lint_clone_on_ref_ptr(cx, expr, &args[0]);
                }
                if args.len() == 1 && method_call.ident.name == sym!(to_string) {
                    inefficient_to_string::lint(cx, expr, &args[0], self_ty);
                }

                match self_ty.kind {
                    ty::Ref(_, ty, _) if ty.kind == ty::Str => {
                        for &(method, pos) in &PATTERN_METHODS {
                            if method_call.ident.name.as_str() == method && args.len() > pos {
                                lint_single_char_pattern(cx, expr, &args[pos]);
                            }
                        }
                    },
                    ty::Ref(..) if method_call.ident.name == sym!(into_iter) => {
                        lint_into_iter(cx, expr, self_ty, *method_span);
                    },
                    _ => (),
                }
            },
            hir::ExprKind::Binary(op, ref lhs, ref rhs)
                if op.node == hir::BinOpKind::Eq || op.node == hir::BinOpKind::Ne =>
            {
                let mut info = BinaryExprInfo {
                    expr,
                    chain: lhs,
                    other: rhs,
                    eq: op.node == hir::BinOpKind::Eq,
                };
                lint_binary_expr_with_method_call(cx, &mut info);
            }
            _ => (),
        }
    }

    fn check_impl_item(&mut self, cx: &LateContext<'a, 'tcx>, impl_item: &'tcx hir::ImplItem) {
        if in_external_macro(cx.sess(), impl_item.span) {
            return;
        }
        let name = impl_item.ident.name.as_str();
        let parent = cx.tcx.hir().get_parent_item(impl_item.hir_id);
        let item = cx.tcx.hir().expect_item(parent);
        let def_id = cx.tcx.hir().local_def_id(item.hir_id);
        let ty = cx.tcx.type_of(def_id);
        if_chain! {
            if let hir::ImplItemKind::Method(ref sig, id) = impl_item.kind;
            if let Some(first_arg) = iter_input_pats(&sig.decl, cx.tcx.hir().body(id)).next();
            if let hir::ItemKind::Impl(_, _, _, _, None, _, _) = item.kind;

            let method_def_id = cx.tcx.hir().local_def_id(impl_item.hir_id);
            let method_sig = cx.tcx.fn_sig(method_def_id);
            let method_sig = cx.tcx.erase_late_bound_regions(&method_sig);

            let first_arg_ty = &method_sig.inputs().iter().next();

            // check conventions w.r.t. conversion method names and predicates
            if let Some(first_arg_ty) = first_arg_ty;

            then {
                if cx.access_levels.is_exported(impl_item.hir_id) {
                // check missing trait implementations
                    for &(method_name, n_args, self_kind, out_type, trait_name) in &TRAIT_METHODS {
                        if name == method_name &&
                        sig.decl.inputs.len() == n_args &&
                        out_type.matches(cx, &sig.decl.output) &&
                        self_kind.matches(cx, ty, first_arg_ty) {
                            span_lint(cx, SHOULD_IMPLEMENT_TRAIT, impl_item.span, &format!(
                                "defining a method called `{}` on this type; consider implementing \
                                the `{}` trait or choosing a less ambiguous name", name, trait_name));
                        }
                    }
                }

                if let Some((ref conv, self_kinds)) = &CONVENTIONS
                    .iter()
                    .find(|(ref conv, _)| conv.check(&name))
                {
                    if !self_kinds.iter().any(|k| k.matches(cx, ty, first_arg_ty)) {
                        let lint = if item.vis.node.is_pub() {
                            WRONG_PUB_SELF_CONVENTION
                        } else {
                            WRONG_SELF_CONVENTION
                        };

                        span_lint(
                            cx,
                            lint,
                            first_arg.pat.span,
                            &format!(
                               "methods called `{}` usually take {}; consider choosing a less \
                                 ambiguous name",
                                conv,
                                &self_kinds
                                    .iter()
                                    .map(|k| k.description())
                                    .collect::<Vec<_>>()
                                    .join(" or ")
                            ),
                        );
                    }
                }
            }
        }

        if let hir::ImplItemKind::Method(_, _) = impl_item.kind {
            let ret_ty = return_ty(cx, impl_item.hir_id);

            // walk the return type and check for Self (this does not check associated types)
            if ret_ty.walk().any(|inner_type| same_tys(cx, ty, inner_type)) {
                return;
            }

            // if return type is impl trait, check the associated types
            if let ty::Opaque(def_id, _) = ret_ty.kind {
                // one of the associated types must be Self
                for predicate in cx.tcx.predicates_of(def_id).predicates {
                    match predicate {
                        (Predicate::Projection(poly_projection_predicate), _) => {
                            let binder = poly_projection_predicate.ty();
                            let associated_type = binder.skip_binder();

                            // walk the associated type and check for Self
                            for inner_type in associated_type.walk() {
                                if same_tys(cx, ty, inner_type) {
                                    return;
                                }
                            }
                        },
                        (_, _) => {},
                    }
                }
            }

            if name == "new" && !same_tys(cx, ret_ty, ty) {
                span_lint(
                    cx,
                    NEW_RET_NO_SELF,
                    impl_item.span,
                    "methods called `new` usually return `Self`",
                );
            }
        }
    }
}

/// Checks for the `OR_FUN_CALL` lint.
#[allow(clippy::too_many_lines)]
fn lint_or_fun_call<'a, 'tcx>(
    cx: &LateContext<'a, 'tcx>,
    expr: &hir::Expr,
    method_span: Span,
    name: &str,
    args: &'tcx [hir::Expr],
) {
    // Searches an expression for method calls or function calls that aren't ctors
    struct FunCallFinder<'a, 'tcx> {
        cx: &'a LateContext<'a, 'tcx>,
        found: bool,
    }

    impl<'a, 'tcx> intravisit::Visitor<'tcx> for FunCallFinder<'a, 'tcx> {
        fn visit_expr(&mut self, expr: &'tcx hir::Expr) {
            let call_found = match &expr.kind {
                // ignore enum and struct constructors
                hir::ExprKind::Call(..) => !is_ctor_or_promotable_const_function(self.cx, expr),
                hir::ExprKind::MethodCall(..) => true,
                _ => false,
            };

            if call_found {
                self.found |= true;
            }

            if !self.found {
                intravisit::walk_expr(self, expr);
            }
        }

        fn nested_visit_map<'this>(&'this mut self) -> intravisit::NestedVisitorMap<'this, 'tcx> {
            intravisit::NestedVisitorMap::None
        }
    }

    /// Checks for `unwrap_or(T::new())` or `unwrap_or(T::default())`.
    fn check_unwrap_or_default(
        cx: &LateContext<'_, '_>,
        name: &str,
        fun: &hir::Expr,
        self_expr: &hir::Expr,
        arg: &hir::Expr,
        or_has_args: bool,
        span: Span,
    ) -> bool {
        if_chain! {
            if !or_has_args;
            if name == "unwrap_or";
            if let hir::ExprKind::Path(ref qpath) = fun.kind;
            let path = &*last_path_segment(qpath).ident.as_str();
            if ["default", "new"].contains(&path);
            let arg_ty = cx.tables.expr_ty(arg);
            if let Some(default_trait_id) = get_trait_def_id(cx, &paths::DEFAULT_TRAIT);
            if implements_trait(cx, arg_ty, default_trait_id, &[]);

            then {
                let mut applicability = Applicability::MachineApplicable;
                span_lint_and_sugg(
                    cx,
                    OR_FUN_CALL,
                    span,
                    &format!("use of `{}` followed by a call to `{}`", name, path),
                    "try this",
                    format!(
                        "{}.unwrap_or_default()",
                        snippet_with_applicability(cx, self_expr.span, "_", &mut applicability)
                    ),
                    applicability,
                );

                true
            } else {
                false
            }
        }
    }

    /// Checks for `*or(foo())`.
    #[allow(clippy::too_many_arguments)]
    fn check_general_case<'a, 'tcx>(
        cx: &LateContext<'a, 'tcx>,
        name: &str,
        method_span: Span,
        fun_span: Span,
        self_expr: &hir::Expr,
        arg: &'tcx hir::Expr,
        or_has_args: bool,
        span: Span,
    ) {
        // (path, fn_has_argument, methods, suffix)
        let know_types: &[(&[_], _, &[_], _)] = &[
            (&paths::BTREEMAP_ENTRY, false, &["or_insert"], "with"),
            (&paths::HASHMAP_ENTRY, false, &["or_insert"], "with"),
            (&paths::OPTION, false, &["map_or", "ok_or", "or", "unwrap_or"], "else"),
            (&paths::RESULT, true, &["or", "unwrap_or"], "else"),
        ];

        if_chain! {
            if know_types.iter().any(|k| k.2.contains(&name));

            let mut finder = FunCallFinder { cx: &cx, found: false };
            if { finder.visit_expr(&arg); finder.found };
            if !contains_return(&arg);

            let self_ty = cx.tables.expr_ty(self_expr);

            if let Some(&(_, fn_has_arguments, poss, suffix)) =
                   know_types.iter().find(|&&i| match_type(cx, self_ty, i.0));

            if poss.contains(&name);

            then {
                let sugg: Cow<'_, _> = match (fn_has_arguments, !or_has_args) {
                    (true, _) => format!("|_| {}", snippet_with_macro_callsite(cx, arg.span, "..")).into(),
                    (false, false) => format!("|| {}", snippet_with_macro_callsite(cx, arg.span, "..")).into(),
                    (false, true) => snippet_with_macro_callsite(cx, fun_span, ".."),
                };
                let span_replace_word = method_span.with_hi(span.hi());
                span_lint_and_sugg(
                    cx,
                    OR_FUN_CALL,
                    span_replace_word,
                    &format!("use of `{}` followed by a function call", name),
                    "try this",
                    format!("{}_{}({})", name, suffix, sugg),
                    Applicability::HasPlaceholders,
                );
            }
        }
    }

    if args.len() == 2 {
        match args[1].kind {
            hir::ExprKind::Call(ref fun, ref or_args) => {
                let or_has_args = !or_args.is_empty();
                if !check_unwrap_or_default(cx, name, fun, &args[0], &args[1], or_has_args, expr.span) {
                    check_general_case(
                        cx,
                        name,
                        method_span,
                        fun.span,
                        &args[0],
                        &args[1],
                        or_has_args,
                        expr.span,
                    );
                }
            },
            hir::ExprKind::MethodCall(_, span, ref or_args) => check_general_case(
                cx,
                name,
                method_span,
                span,
                &args[0],
                &args[1],
                !or_args.is_empty(),
                expr.span,
            ),
            _ => {},
        }
    }
}

/// Checks for the `EXPECT_FUN_CALL` lint.
#[allow(clippy::too_many_lines)]
fn lint_expect_fun_call(cx: &LateContext<'_, '_>, expr: &hir::Expr, method_span: Span, name: &str, args: &[hir::Expr]) {
    // Strip `&`, `as_ref()` and `as_str()` off `arg` until we're left with either a `String` or
    // `&str`
    fn get_arg_root<'a>(cx: &LateContext<'_, '_>, arg: &'a hir::Expr) -> &'a hir::Expr {
        let mut arg_root = arg;
        loop {
            arg_root = match &arg_root.kind {
                hir::ExprKind::AddrOf(_, expr) => expr,
                hir::ExprKind::MethodCall(method_name, _, call_args) => {
                    if call_args.len() == 1
                        && (method_name.ident.name == sym!(as_str) || method_name.ident.name == sym!(as_ref))
                        && {
                            let arg_type = cx.tables.expr_ty(&call_args[0]);
                            let base_type = walk_ptrs_ty(arg_type);
                            base_type.kind == ty::Str || match_type(cx, base_type, &paths::STRING)
                        }
                    {
                        &call_args[0]
                    } else {
                        break;
                    }
                },
                _ => break,
            };
        }
        arg_root
    }

    // Only `&'static str` or `String` can be used directly in the `panic!`. Other types should be
    // converted to string.
    fn requires_to_string(cx: &LateContext<'_, '_>, arg: &hir::Expr) -> bool {
        let arg_ty = cx.tables.expr_ty(arg);
        if match_type(cx, arg_ty, &paths::STRING) {
            return false;
        }
        if let ty::Ref(ty::ReStatic, ty, ..) = arg_ty.kind {
            if ty.kind == ty::Str {
                return false;
            }
        };
        true
    }

    fn generate_format_arg_snippet(
        cx: &LateContext<'_, '_>,
        a: &hir::Expr,
        applicability: &mut Applicability,
    ) -> Vec<String> {
        if_chain! {
            if let hir::ExprKind::AddrOf(_, ref format_arg) = a.kind;
            if let hir::ExprKind::Match(ref format_arg_expr, _, _) = format_arg.kind;
            if let hir::ExprKind::Tup(ref format_arg_expr_tup) = format_arg_expr.kind;

            then {
                format_arg_expr_tup
                    .iter()
                    .map(|a| snippet_with_applicability(cx, a.span, "..", applicability).into_owned())
                    .collect()
            } else {
                unreachable!()
            }
        }
    }

    fn is_call(node: &hir::ExprKind) -> bool {
        match node {
            hir::ExprKind::AddrOf(_, expr) => {
                is_call(&expr.kind)
            },
            hir::ExprKind::Call(..)
            | hir::ExprKind::MethodCall(..)
            // These variants are debatable or require further examination
            | hir::ExprKind::Match(..)
            | hir::ExprKind::Block{ .. } => true,
            _ => false,
        }
    }

    if args.len() != 2 || name != "expect" || !is_call(&args[1].kind) {
        return;
    }

    let receiver_type = cx.tables.expr_ty(&args[0]);
    let closure_args = if match_type(cx, receiver_type, &paths::OPTION) {
        "||"
    } else if match_type(cx, receiver_type, &paths::RESULT) {
        "|_|"
    } else {
        return;
    };

    let arg_root = get_arg_root(cx, &args[1]);

    let span_replace_word = method_span.with_hi(expr.span.hi());

    let mut applicability = Applicability::MachineApplicable;

    //Special handling for `format!` as arg_root
    if let hir::ExprKind::Call(ref inner_fun, ref inner_args) = arg_root.kind {
        if is_expn_of(inner_fun.span, "format").is_some() && inner_args.len() == 1 {
            if let hir::ExprKind::Call(_, format_args) = &inner_args[0].kind {
                let fmt_spec = &format_args[0];
                let fmt_args = &format_args[1];

                let mut args = vec![snippet(cx, fmt_spec.span, "..").into_owned()];

                args.extend(generate_format_arg_snippet(cx, fmt_args, &mut applicability));

                let sugg = args.join(", ");

                span_lint_and_sugg(
                    cx,
                    EXPECT_FUN_CALL,
                    span_replace_word,
                    &format!("use of `{}` followed by a function call", name),
                    "try this",
                    format!("unwrap_or_else({} panic!({}))", closure_args, sugg),
                    applicability,
                );

                return;
            }
        }
    }

    let mut arg_root_snippet: Cow<'_, _> = snippet_with_applicability(cx, arg_root.span, "..", &mut applicability);
    if requires_to_string(cx, arg_root) {
        arg_root_snippet.to_mut().push_str(".to_string()");
    }

    span_lint_and_sugg(
        cx,
        EXPECT_FUN_CALL,
        span_replace_word,
        &format!("use of `{}` followed by a function call", name),
        "try this",
        format!("unwrap_or_else({} {{ panic!({}) }})", closure_args, arg_root_snippet),
        applicability,
    );
}

/// Checks for the `CLONE_ON_COPY` lint.
fn lint_clone_on_copy(cx: &LateContext<'_, '_>, expr: &hir::Expr, arg: &hir::Expr, arg_ty: Ty<'_>) {
    let ty = cx.tables.expr_ty(expr);
    if let ty::Ref(_, inner, _) = arg_ty.kind {
        if let ty::Ref(_, innermost, _) = inner.kind {
            span_lint_and_then(
                cx,
                CLONE_DOUBLE_REF,
                expr.span,
                "using `clone` on a double-reference; \
                 this will copy the reference instead of cloning the inner type",
                |db| {
                    if let Some(snip) = sugg::Sugg::hir_opt(cx, arg) {
                        let mut ty = innermost;
                        let mut n = 0;
                        while let ty::Ref(_, inner, _) = ty.kind {
                            ty = inner;
                            n += 1;
                        }
                        let refs: String = iter::repeat('&').take(n + 1).collect();
                        let derefs: String = iter::repeat('*').take(n).collect();
                        let explicit = format!("{}{}::clone({})", refs, ty, snip);
                        db.span_suggestion(
                            expr.span,
                            "try dereferencing it",
                            format!("{}({}{}).clone()", refs, derefs, snip.deref()),
                            Applicability::MaybeIncorrect,
                        );
                        db.span_suggestion(
                            expr.span,
                            "or try being explicit about what type to clone",
                            explicit,
                            Applicability::MaybeIncorrect,
                        );
                    }
                },
            );
            return; // don't report clone_on_copy
        }
    }

    if is_copy(cx, ty) {
        let snip;
        if let Some(snippet) = sugg::Sugg::hir_opt(cx, arg) {
            let parent = cx.tcx.hir().get_parent_node(expr.hir_id);
            match &cx.tcx.hir().get(parent) {
                hir::Node::Expr(parent) => match parent.kind {
                    // &*x is a nop, &x.clone() is not
                    hir::ExprKind::AddrOf(..) |
                    // (*x).func() is useless, x.clone().func() can work in case func borrows mutably
                    hir::ExprKind::MethodCall(..) => return,
                    _ => {},
                },
                hir::Node::Stmt(stmt) => {
                    if let hir::StmtKind::Local(ref loc) = stmt.kind {
                        if let hir::PatKind::Ref(..) = loc.pat.kind {
                            // let ref y = *x borrows x, let ref y = x.clone() does not
                            return;
                        }
                    }
                },
                _ => {},
            }

            // x.clone() might have dereferenced x, possibly through Deref impls
            if cx.tables.expr_ty(arg) == ty {
                snip = Some(("try removing the `clone` call", format!("{}", snippet)));
            } else {
                let deref_count = cx
                    .tables
                    .expr_adjustments(arg)
                    .iter()
                    .filter(|adj| {
                        if let ty::adjustment::Adjust::Deref(_) = adj.kind {
                            true
                        } else {
                            false
                        }
                    })
                    .count();
                let derefs: String = iter::repeat('*').take(deref_count).collect();
                snip = Some(("try dereferencing it", format!("{}{}", derefs, snippet)));
            }
        } else {
            snip = None;
        }
        span_lint_and_then(cx, CLONE_ON_COPY, expr.span, "using `clone` on a `Copy` type", |db| {
            if let Some((text, snip)) = snip {
                db.span_suggestion(expr.span, text, snip, Applicability::Unspecified);
            }
        });
    }
}

fn lint_clone_on_ref_ptr(cx: &LateContext<'_, '_>, expr: &hir::Expr, arg: &hir::Expr) {
    let obj_ty = walk_ptrs_ty(cx.tables.expr_ty(arg));

    if let ty::Adt(_, subst) = obj_ty.kind {
        let caller_type = if match_type(cx, obj_ty, &paths::RC) {
            "Rc"
        } else if match_type(cx, obj_ty, &paths::ARC) {
            "Arc"
        } else if match_type(cx, obj_ty, &paths::WEAK_RC) || match_type(cx, obj_ty, &paths::WEAK_ARC) {
            "Weak"
        } else {
            return;
        };

        span_lint_and_sugg(
            cx,
            CLONE_ON_REF_PTR,
            expr.span,
            "using '.clone()' on a ref-counted pointer",
            "try this",
            format!(
                "{}::<{}>::clone(&{})",
                caller_type,
                subst.type_at(0),
                snippet(cx, arg.span, "_")
            ),
            Applicability::Unspecified, // Sometimes unnecessary ::<_> after Rc/Arc/Weak
        );
    }
}

fn lint_string_extend(cx: &LateContext<'_, '_>, expr: &hir::Expr, args: &[hir::Expr]) {
    let arg = &args[1];
    if let Some(arglists) = method_chain_args(arg, &["chars"]) {
        let target = &arglists[0][0];
        let self_ty = walk_ptrs_ty(cx.tables.expr_ty(target));
        let ref_str = if self_ty.kind == ty::Str {
            ""
        } else if match_type(cx, self_ty, &paths::STRING) {
            "&"
        } else {
            return;
        };

        let mut applicability = Applicability::MachineApplicable;
        span_lint_and_sugg(
            cx,
            STRING_EXTEND_CHARS,
            expr.span,
            "calling `.extend(_.chars())`",
            "try this",
            format!(
                "{}.push_str({}{})",
                snippet_with_applicability(cx, args[0].span, "_", &mut applicability),
                ref_str,
                snippet_with_applicability(cx, target.span, "_", &mut applicability)
            ),
            applicability,
        );
    }
}

fn lint_extend(cx: &LateContext<'_, '_>, expr: &hir::Expr, args: &[hir::Expr]) {
    let obj_ty = walk_ptrs_ty(cx.tables.expr_ty(&args[0]));
    if match_type(cx, obj_ty, &paths::STRING) {
        lint_string_extend(cx, expr, args);
    }
}

fn lint_cstring_as_ptr(cx: &LateContext<'_, '_>, expr: &hir::Expr, source: &hir::Expr, unwrap: &hir::Expr) {
    if_chain! {
        let source_type = cx.tables.expr_ty(source);
        if let ty::Adt(def, substs) = source_type.kind;
        if match_def_path(cx, def.did, &paths::RESULT);
        if match_type(cx, substs.type_at(0), &paths::CSTRING);
        then {
            span_lint_and_then(
                cx,
                TEMPORARY_CSTRING_AS_PTR,
                expr.span,
                "you are getting the inner pointer of a temporary `CString`",
                |db| {
                    db.note("that pointer will be invalid outside this expression");
                    db.span_help(unwrap.span, "assign the `CString` to a variable to extend its lifetime");
                });
        }
    }
}

fn lint_iter_cloned_collect<'a, 'tcx>(cx: &LateContext<'a, 'tcx>, expr: &hir::Expr, iter_args: &'tcx [hir::Expr]) {
    if_chain! {
        if is_type_diagnostic_item(cx, cx.tables.expr_ty(expr), Symbol::intern("vec_type"));
        if let Some(slice) = derefs_to_slice(cx, &iter_args[0], cx.tables.expr_ty(&iter_args[0]));
        if let Some(to_replace) = expr.span.trim_start(slice.span.source_callsite());

        then {
            span_lint_and_sugg(
                cx,
                ITER_CLONED_COLLECT,
                to_replace,
                "called `iter().cloned().collect()` on a slice to create a `Vec`. Calling `to_vec()` is both faster and \
                 more readable",
                "try",
                ".to_vec()".to_string(),
                Applicability::MachineApplicable,
            );
        }
    }
}

fn lint_unnecessary_fold(cx: &LateContext<'_, '_>, expr: &hir::Expr, fold_args: &[hir::Expr], fold_span: Span) {
    fn check_fold_with_op(
        cx: &LateContext<'_, '_>,
        expr: &hir::Expr,
        fold_args: &[hir::Expr],
        fold_span: Span,
        op: hir::BinOpKind,
        replacement_method_name: &str,
        replacement_has_args: bool,
    ) {
        if_chain! {
            // Extract the body of the closure passed to fold
            if let hir::ExprKind::Closure(_, _, body_id, _, _) = fold_args[2].kind;
            let closure_body = cx.tcx.hir().body(body_id);
            let closure_expr = remove_blocks(&closure_body.value);

            // Check if the closure body is of the form `acc <op> some_expr(x)`
            if let hir::ExprKind::Binary(ref bin_op, ref left_expr, ref right_expr) = closure_expr.kind;
            if bin_op.node == op;

            // Extract the names of the two arguments to the closure
            if let Some(first_arg_ident) = get_arg_name(&closure_body.params[0].pat);
            if let Some(second_arg_ident) = get_arg_name(&closure_body.params[1].pat);

            if match_var(&*left_expr, first_arg_ident);
            if replacement_has_args || match_var(&*right_expr, second_arg_ident);

            then {
                let mut applicability = Applicability::MachineApplicable;
                let sugg = if replacement_has_args {
                    format!(
                        "{replacement}(|{s}| {r})",
                        replacement = replacement_method_name,
                        s = second_arg_ident,
                        r = snippet_with_applicability(cx, right_expr.span, "EXPR", &mut applicability),
                    )
                } else {
                    format!(
                        "{replacement}()",
                        replacement = replacement_method_name,
                    )
                };

                span_lint_and_sugg(
                    cx,
                    UNNECESSARY_FOLD,
                    fold_span.with_hi(expr.span.hi()),
                    // TODO #2371 don't suggest e.g., .any(|x| f(x)) if we can suggest .any(f)
                    "this `.fold` can be written more succinctly using another method",
                    "try",
                    sugg,
                    applicability,
                );
            }
        }
    }

    // Check that this is a call to Iterator::fold rather than just some function called fold
    if !match_trait_method(cx, expr, &paths::ITERATOR) {
        return;
    }

    assert!(
        fold_args.len() == 3,
        "Expected fold_args to have three entries - the receiver, the initial value and the closure"
    );

    // Check if the first argument to .fold is a suitable literal
    if let hir::ExprKind::Lit(ref lit) = fold_args[1].kind {
        match lit.node {
            ast::LitKind::Bool(false) => {
                check_fold_with_op(cx, expr, fold_args, fold_span, hir::BinOpKind::Or, "any", true)
            },
            ast::LitKind::Bool(true) => {
                check_fold_with_op(cx, expr, fold_args, fold_span, hir::BinOpKind::And, "all", true)
            },
            ast::LitKind::Int(0, _) => {
                check_fold_with_op(cx, expr, fold_args, fold_span, hir::BinOpKind::Add, "sum", false)
            },
            ast::LitKind::Int(1, _) => {
                check_fold_with_op(cx, expr, fold_args, fold_span, hir::BinOpKind::Mul, "product", false)
            },
            _ => (),
        }
    }
}

fn lint_iter_nth<'a, 'tcx>(cx: &LateContext<'a, 'tcx>, expr: &hir::Expr, iter_args: &'tcx [hir::Expr], is_mut: bool) {
    let mut_str = if is_mut { "_mut" } else { "" };
    let caller_type = if derefs_to_slice(cx, &iter_args[0], cx.tables.expr_ty(&iter_args[0])).is_some() {
        "slice"
    } else if is_type_diagnostic_item(cx, cx.tables.expr_ty(&iter_args[0]), Symbol::intern("vec_type")) {
        "Vec"
    } else if match_type(cx, cx.tables.expr_ty(&iter_args[0]), &paths::VEC_DEQUE) {
        "VecDeque"
    } else {
        return; // caller is not a type that we want to lint
    };

    span_lint(
        cx,
        ITER_NTH,
        expr.span,
        &format!(
            "called `.iter{0}().nth()` on a {1}. Calling `.get{0}()` is both faster and more readable",
            mut_str, caller_type
        ),
    );
}

fn lint_get_unwrap<'a, 'tcx>(cx: &LateContext<'a, 'tcx>, expr: &hir::Expr, get_args: &'tcx [hir::Expr], is_mut: bool) {
    // Note: we don't want to lint `get_mut().unwrap` for `HashMap` or `BTreeMap`,
    // because they do not implement `IndexMut`
    let mut applicability = Applicability::MachineApplicable;
    let expr_ty = cx.tables.expr_ty(&get_args[0]);
    let get_args_str = if get_args.len() > 1 {
        snippet_with_applicability(cx, get_args[1].span, "_", &mut applicability)
    } else {
        return; // not linting on a .get().unwrap() chain or variant
    };
    let mut needs_ref;
    let caller_type = if derefs_to_slice(cx, &get_args[0], expr_ty).is_some() {
        needs_ref = get_args_str.parse::<usize>().is_ok();
        "slice"
    } else if is_type_diagnostic_item(cx, expr_ty, Symbol::intern("vec_type")) {
        needs_ref = get_args_str.parse::<usize>().is_ok();
        "Vec"
    } else if match_type(cx, expr_ty, &paths::VEC_DEQUE) {
        needs_ref = get_args_str.parse::<usize>().is_ok();
        "VecDeque"
    } else if !is_mut && match_type(cx, expr_ty, &paths::HASHMAP) {
        needs_ref = true;
        "HashMap"
    } else if !is_mut && match_type(cx, expr_ty, &paths::BTREEMAP) {
        needs_ref = true;
        "BTreeMap"
    } else {
        return; // caller is not a type that we want to lint
    };

    let mut span = expr.span;

    // Handle the case where the result is immediately dereferenced
    // by not requiring ref and pulling the dereference into the
    // suggestion.
    if_chain! {
        if needs_ref;
        if let Some(parent) = get_parent_expr(cx, expr);
        if let hir::ExprKind::Unary(hir::UnOp::UnDeref, _) = parent.kind;
        then {
            needs_ref = false;
            span = parent.span;
        }
    }

    let mut_str = if is_mut { "_mut" } else { "" };
    let borrow_str = if !needs_ref {
        ""
    } else if is_mut {
        "&mut "
    } else {
        "&"
    };

    span_lint_and_sugg(
        cx,
        GET_UNWRAP,
        span,
        &format!(
            "called `.get{0}().unwrap()` on a {1}. Using `[]` is more clear and more concise",
            mut_str, caller_type
        ),
        "try this",
        format!(
            "{}{}[{}]",
            borrow_str,
            snippet_with_applicability(cx, get_args[0].span, "_", &mut applicability),
            get_args_str
        ),
        applicability,
    );
}

fn lint_iter_skip_next(cx: &LateContext<'_, '_>, expr: &hir::Expr) {
    // lint if caller of skip is an Iterator
    if match_trait_method(cx, expr, &paths::ITERATOR) {
        span_lint(
            cx,
            ITER_SKIP_NEXT,
            expr.span,
            "called `skip(x).next()` on an iterator. This is more succinctly expressed by calling `nth(x)`",
        );
    }
}

fn derefs_to_slice<'a, 'tcx>(
    cx: &LateContext<'a, 'tcx>,
    expr: &'tcx hir::Expr,
    ty: Ty<'tcx>,
) -> Option<&'tcx hir::Expr> {
    fn may_slice<'a>(cx: &LateContext<'_, 'a>, ty: Ty<'a>) -> bool {
        match ty.kind {
            ty::Slice(_) => true,
            ty::Adt(def, _) if def.is_box() => may_slice(cx, ty.boxed_ty()),
            ty::Adt(..) => is_type_diagnostic_item(cx, ty, Symbol::intern("vec_type")),
            ty::Array(_, size) => size.eval_usize(cx.tcx, cx.param_env) < 32,
            ty::Ref(_, inner, _) => may_slice(cx, inner),
            _ => false,
        }
    }

    if let hir::ExprKind::MethodCall(ref path, _, ref args) = expr.kind {
        if path.ident.name == sym!(iter) && may_slice(cx, cx.tables.expr_ty(&args[0])) {
            Some(&args[0])
        } else {
            None
        }
    } else {
        match ty.kind {
            ty::Slice(_) => Some(expr),
            ty::Adt(def, _) if def.is_box() && may_slice(cx, ty.boxed_ty()) => Some(expr),
            ty::Ref(_, inner, _) => {
                if may_slice(cx, inner) {
                    Some(expr)
                } else {
                    None
                }
            },
            _ => None,
        }
    }
}

/// lint use of `unwrap()` for `Option`s and `Result`s
fn lint_unwrap(cx: &LateContext<'_, '_>, expr: &hir::Expr, unwrap_args: &[hir::Expr]) {
    let obj_ty = walk_ptrs_ty(cx.tables.expr_ty(&unwrap_args[0]));

    let mess = if match_type(cx, obj_ty, &paths::OPTION) {
        Some((OPTION_UNWRAP_USED, "an Option", "None"))
    } else if match_type(cx, obj_ty, &paths::RESULT) {
        Some((RESULT_UNWRAP_USED, "a Result", "Err"))
    } else {
        None
    };

    if let Some((lint, kind, none_value)) = mess {
        span_lint(
            cx,
            lint,
            expr.span,
            &format!(
                "used unwrap() on {} value. If you don't want to handle the {} case gracefully, consider \
                 using expect() to provide a better panic \
                 message",
                kind, none_value
            ),
        );
    }
}

/// lint use of `expect()` for `Option`s and `Result`s
fn lint_expect(cx: &LateContext<'_, '_>, expr: &hir::Expr, expect_args: &[hir::Expr]) {
    let obj_ty = walk_ptrs_ty(cx.tables.expr_ty(&expect_args[0]));

    let mess = if match_type(cx, obj_ty, &paths::OPTION) {
        Some((OPTION_EXPECT_USED, "an Option", "None"))
    } else if match_type(cx, obj_ty, &paths::RESULT) {
        Some((RESULT_EXPECT_USED, "a Result", "Err"))
    } else {
        None
    };

    if let Some((lint, kind, none_value)) = mess {
        span_lint(
            cx,
            lint,
            expr.span,
            &format!(
                "used expect() on {} value. If this value is an {} it will panic",
                kind, none_value
            ),
        );
    }
}

/// lint use of `ok().expect()` for `Result`s
fn lint_ok_expect(cx: &LateContext<'_, '_>, expr: &hir::Expr, ok_args: &[hir::Expr]) {
    if_chain! {
        // lint if the caller of `ok()` is a `Result`
        if match_type(cx, cx.tables.expr_ty(&ok_args[0]), &paths::RESULT);
        let result_type = cx.tables.expr_ty(&ok_args[0]);
        if let Some(error_type) = get_error_type(cx, result_type);
        if has_debug_impl(error_type, cx);

        then {
            span_lint(
                cx,
                OK_EXPECT,
                expr.span,
                "called `ok().expect()` on a Result value. You can call `expect` directly on the `Result`",
            );
        }
    }
}

/// lint use of `map().flatten()` for `Iterators`
fn lint_map_flatten<'a, 'tcx>(cx: &LateContext<'a, 'tcx>, expr: &'tcx hir::Expr, map_args: &'tcx [hir::Expr]) {
    // lint if caller of `.map().flatten()` is an Iterator
    if match_trait_method(cx, expr, &paths::ITERATOR) {
        let msg = "called `map(..).flatten()` on an `Iterator`. \
                   This is more succinctly expressed by calling `.flat_map(..)`";
        let self_snippet = snippet(cx, map_args[0].span, "..");
        let func_snippet = snippet(cx, map_args[1].span, "..");
        let hint = format!("{0}.flat_map({1})", self_snippet, func_snippet);
        span_lint_and_then(cx, MAP_FLATTEN, expr.span, msg, |db| {
            db.span_suggestion(
                expr.span,
                "try using flat_map instead",
                hint,
                Applicability::MachineApplicable,
            );
        });
    }
}

/// lint use of `map().unwrap_or_else()` for `Option`s and `Result`s
fn lint_map_unwrap_or_else<'a, 'tcx>(
    cx: &LateContext<'a, 'tcx>,
    expr: &'tcx hir::Expr,
    map_args: &'tcx [hir::Expr],
    unwrap_args: &'tcx [hir::Expr],
) {
    // lint if the caller of `map()` is an `Option`
    let is_option = match_type(cx, cx.tables.expr_ty(&map_args[0]), &paths::OPTION);
    let is_result = match_type(cx, cx.tables.expr_ty(&map_args[0]), &paths::RESULT);

    if is_option || is_result {
        // Don't make a suggestion that may fail to compile due to mutably borrowing
        // the same variable twice.
        let map_mutated_vars = mutated_variables(&map_args[0], cx);
        let unwrap_mutated_vars = mutated_variables(&unwrap_args[1], cx);
        if let (Some(map_mutated_vars), Some(unwrap_mutated_vars)) = (map_mutated_vars, unwrap_mutated_vars) {
            if map_mutated_vars.intersection(&unwrap_mutated_vars).next().is_some() {
                return;
            }
        } else {
            return;
        }

        // lint message
        let msg = if is_option {
            "called `map(f).unwrap_or_else(g)` on an Option value. This can be done more directly by calling \
             `map_or_else(g, f)` instead"
        } else {
            "called `map(f).unwrap_or_else(g)` on a Result value. This can be done more directly by calling \
             `ok().map_or_else(g, f)` instead"
        };
        // get snippets for args to map() and unwrap_or_else()
        let map_snippet = snippet(cx, map_args[1].span, "..");
        let unwrap_snippet = snippet(cx, unwrap_args[1].span, "..");
        // lint, with note if neither arg is > 1 line and both map() and
        // unwrap_or_else() have the same span
        let multiline = map_snippet.lines().count() > 1 || unwrap_snippet.lines().count() > 1;
        let same_span = map_args[1].span.ctxt() == unwrap_args[1].span.ctxt();
        if same_span && !multiline {
            span_note_and_lint(
                cx,
                if is_option {
                    OPTION_MAP_UNWRAP_OR_ELSE
                } else {
                    RESULT_MAP_UNWRAP_OR_ELSE
                },
                expr.span,
                msg,
                expr.span,
                &format!(
                    "replace `map({0}).unwrap_or_else({1})` with `{2}map_or_else({1}, {0})`",
                    map_snippet,
                    unwrap_snippet,
                    if is_result { "ok()." } else { "" }
                ),
            );
        } else if same_span && multiline {
            span_lint(
                cx,
                if is_option {
                    OPTION_MAP_UNWRAP_OR_ELSE
                } else {
                    RESULT_MAP_UNWRAP_OR_ELSE
                },
                expr.span,
                msg,
            );
        };
    }
}

/// lint use of `_.map_or(None, _)` for `Option`s
fn lint_map_or_none<'a, 'tcx>(cx: &LateContext<'a, 'tcx>, expr: &'tcx hir::Expr, map_or_args: &'tcx [hir::Expr]) {
    if match_type(cx, cx.tables.expr_ty(&map_or_args[0]), &paths::OPTION) {
        // check if the first non-self argument to map_or() is None
        let map_or_arg_is_none = if let hir::ExprKind::Path(ref qpath) = map_or_args[1].kind {
            match_qpath(qpath, &paths::OPTION_NONE)
        } else {
            false
        };

        if map_or_arg_is_none {
            // lint message
            let msg = "called `map_or(None, f)` on an Option value. This can be done more directly by calling \
                       `and_then(f)` instead";
            let map_or_self_snippet = snippet(cx, map_or_args[0].span, "..");
            let map_or_func_snippet = snippet(cx, map_or_args[2].span, "..");
            let hint = format!("{0}.and_then({1})", map_or_self_snippet, map_or_func_snippet);
            span_lint_and_then(cx, OPTION_MAP_OR_NONE, expr.span, msg, |db| {
                db.span_suggestion(
                    expr.span,
                    "try using and_then instead",
                    hint,
                    Applicability::MachineApplicable, // snippet
                );
            });
        }
    }
}

/// Lint use of `_.and_then(|x| Some(y))` for `Option`s
fn lint_option_and_then_some(cx: &LateContext<'_, '_>, expr: &hir::Expr, args: &[hir::Expr]) {
    const LINT_MSG: &str = "using `Option.and_then(|x| Some(y))`, which is more succinctly expressed as `map(|x| y)`";
    const NO_OP_MSG: &str = "using `Option.and_then(Some)`, which is a no-op";

    let ty = cx.tables.expr_ty(&args[0]);
    if !match_type(cx, ty, &paths::OPTION) {
        return;
    }

    match args[1].kind {
        hir::ExprKind::Closure(_, _, body_id, closure_args_span, _) => {
            let closure_body = cx.tcx.hir().body(body_id);
            let closure_expr = remove_blocks(&closure_body.value);
            if_chain! {
                if let hir::ExprKind::Call(ref some_expr, ref some_args) = closure_expr.kind;
                if let hir::ExprKind::Path(ref qpath) = some_expr.kind;
                if match_qpath(qpath, &paths::OPTION_SOME);
                if some_args.len() == 1;
                then {
                    let inner_expr = &some_args[0];

                    if contains_return(inner_expr) {
                        return;
                    }

                    let some_inner_snip = if inner_expr.span.from_expansion() {
                        snippet_with_macro_callsite(cx, inner_expr.span, "_")
                    } else {
                        snippet(cx, inner_expr.span, "_")
                    };

                    let closure_args_snip = snippet(cx, closure_args_span, "..");
                    let option_snip = snippet(cx, args[0].span, "..");
                    let note = format!("{}.map({} {})", option_snip, closure_args_snip, some_inner_snip);
                    span_lint_and_sugg(
                        cx,
                        OPTION_AND_THEN_SOME,
                        expr.span,
                        LINT_MSG,
                        "try this",
                        note,
                        Applicability::MachineApplicable,
                    );
                }
            }
        },
        // `_.and_then(Some)` case, which is no-op.
        hir::ExprKind::Path(ref qpath) => {
            if match_qpath(qpath, &paths::OPTION_SOME) {
                let option_snip = snippet(cx, args[0].span, "..");
                let note = format!("{}", option_snip);
                span_lint_and_sugg(
                    cx,
                    OPTION_AND_THEN_SOME,
                    expr.span,
                    NO_OP_MSG,
                    "use the expression directly",
                    note,
                    Applicability::MachineApplicable,
                );
            }
        },
        _ => {},
    }
}

/// lint use of `filter().next()` for `Iterators`
fn lint_filter_next<'a, 'tcx>(cx: &LateContext<'a, 'tcx>, expr: &'tcx hir::Expr, filter_args: &'tcx [hir::Expr]) {
    // lint if caller of `.filter().next()` is an Iterator
    if match_trait_method(cx, expr, &paths::ITERATOR) {
        let msg = "called `filter(p).next()` on an `Iterator`. This is more succinctly expressed by calling \
                   `.find(p)` instead.";
        let filter_snippet = snippet(cx, filter_args[1].span, "..");
        if filter_snippet.lines().count() <= 1 {
            // add note if not multi-line
            span_note_and_lint(
                cx,
                FILTER_NEXT,
                expr.span,
                msg,
                expr.span,
                &format!("replace `filter({0}).next()` with `find({0})`", filter_snippet),
            );
        } else {
            span_lint(cx, FILTER_NEXT, expr.span, msg);
        }
    }
}

/// lint use of `filter().map()` for `Iterators`
fn lint_filter_map<'a, 'tcx>(
    cx: &LateContext<'a, 'tcx>,
    expr: &'tcx hir::Expr,
    _filter_args: &'tcx [hir::Expr],
    _map_args: &'tcx [hir::Expr],
) {
    // lint if caller of `.filter().map()` is an Iterator
    if match_trait_method(cx, expr, &paths::ITERATOR) {
        let msg = "called `filter(p).map(q)` on an `Iterator`. \
                   This is more succinctly expressed by calling `.filter_map(..)` instead.";
        span_lint(cx, FILTER_MAP, expr.span, msg);
    }
}

/// lint use of `filter_map().next()` for `Iterators`
fn lint_filter_map_next<'a, 'tcx>(cx: &LateContext<'a, 'tcx>, expr: &'tcx hir::Expr, filter_args: &'tcx [hir::Expr]) {
    if match_trait_method(cx, expr, &paths::ITERATOR) {
        let msg = "called `filter_map(p).next()` on an `Iterator`. This is more succinctly expressed by calling \
                   `.find_map(p)` instead.";
        let filter_snippet = snippet(cx, filter_args[1].span, "..");
        if filter_snippet.lines().count() <= 1 {
            span_note_and_lint(
                cx,
                FILTER_MAP_NEXT,
                expr.span,
                msg,
                expr.span,
                &format!("replace `filter_map({0}).next()` with `find_map({0})`", filter_snippet),
            );
        } else {
            span_lint(cx, FILTER_MAP_NEXT, expr.span, msg);
        }
    }
}

/// lint use of `find().map()` for `Iterators`
fn lint_find_map<'a, 'tcx>(
    cx: &LateContext<'a, 'tcx>,
    expr: &'tcx hir::Expr,
    _find_args: &'tcx [hir::Expr],
    map_args: &'tcx [hir::Expr],
) {
    // lint if caller of `.filter().map()` is an Iterator
    if match_trait_method(cx, &map_args[0], &paths::ITERATOR) {
        let msg = "called `find(p).map(q)` on an `Iterator`. \
                   This is more succinctly expressed by calling `.find_map(..)` instead.";
        span_lint(cx, FIND_MAP, expr.span, msg);
    }
}

/// lint use of `filter().map()` for `Iterators`
fn lint_filter_map_map<'a, 'tcx>(
    cx: &LateContext<'a, 'tcx>,
    expr: &'tcx hir::Expr,
    _filter_args: &'tcx [hir::Expr],
    _map_args: &'tcx [hir::Expr],
) {
    // lint if caller of `.filter().map()` is an Iterator
    if match_trait_method(cx, expr, &paths::ITERATOR) {
        let msg = "called `filter_map(p).map(q)` on an `Iterator`. \
                   This is more succinctly expressed by only calling `.filter_map(..)` instead.";
        span_lint(cx, FILTER_MAP, expr.span, msg);
    }
}

/// lint use of `filter().flat_map()` for `Iterators`
fn lint_filter_flat_map<'a, 'tcx>(
    cx: &LateContext<'a, 'tcx>,
    expr: &'tcx hir::Expr,
    _filter_args: &'tcx [hir::Expr],
    _map_args: &'tcx [hir::Expr],
) {
    // lint if caller of `.filter().flat_map()` is an Iterator
    if match_trait_method(cx, expr, &paths::ITERATOR) {
        let msg = "called `filter(p).flat_map(q)` on an `Iterator`. \
                   This is more succinctly expressed by calling `.flat_map(..)` \
                   and filtering by returning an empty Iterator.";
        span_lint(cx, FILTER_MAP, expr.span, msg);
    }
}

/// lint use of `filter_map().flat_map()` for `Iterators`
fn lint_filter_map_flat_map<'a, 'tcx>(
    cx: &LateContext<'a, 'tcx>,
    expr: &'tcx hir::Expr,
    _filter_args: &'tcx [hir::Expr],
    _map_args: &'tcx [hir::Expr],
) {
    // lint if caller of `.filter_map().flat_map()` is an Iterator
    if match_trait_method(cx, expr, &paths::ITERATOR) {
        let msg = "called `filter_map(p).flat_map(q)` on an `Iterator`. \
                   This is more succinctly expressed by calling `.flat_map(..)` \
                   and filtering by returning an empty Iterator.";
        span_lint(cx, FILTER_MAP, expr.span, msg);
    }
}

/// lint use of `flat_map` for `Iterators` where `flatten` would be sufficient
fn lint_flat_map_identity<'a, 'tcx>(
    cx: &LateContext<'a, 'tcx>,
    expr: &'tcx hir::Expr,
    flat_map_args: &'tcx [hir::Expr],
    flat_map_span: Span,
) {
    if match_trait_method(cx, expr, &paths::ITERATOR) {
        let arg_node = &flat_map_args[1].kind;

        let apply_lint = |message: &str| {
            span_lint_and_sugg(
                cx,
                FLAT_MAP_IDENTITY,
                flat_map_span.with_hi(expr.span.hi()),
                message,
                "try",
                "flatten()".to_string(),
                Applicability::MachineApplicable,
            );
        };

        if_chain! {
            if let hir::ExprKind::Closure(_, _, body_id, _, _) = arg_node;
            let body = cx.tcx.hir().body(*body_id);

            if let hir::PatKind::Binding(_, _, binding_ident, _) = body.params[0].pat.kind;
            if let hir::ExprKind::Path(hir::QPath::Resolved(_, ref path)) = body.value.kind;

            if path.segments.len() == 1;
            if path.segments[0].ident.as_str() == binding_ident.as_str();

            then {
                apply_lint("called `flat_map(|x| x)` on an `Iterator`");
            }
        }

        if_chain! {
            if let hir::ExprKind::Path(ref qpath) = arg_node;

            if match_qpath(qpath, &paths::STD_CONVERT_IDENTITY);

            then {
                apply_lint("called `flat_map(std::convert::identity)` on an `Iterator`");
            }
        }
    }
}

/// lint searching an Iterator followed by `is_some()`
fn lint_search_is_some<'a, 'tcx>(
    cx: &LateContext<'a, 'tcx>,
    expr: &'tcx hir::Expr,
    search_method: &str,
    search_args: &'tcx [hir::Expr],
    is_some_args: &'tcx [hir::Expr],
    method_span: Span,
) {
    // lint if caller of search is an Iterator
    if match_trait_method(cx, &is_some_args[0], &paths::ITERATOR) {
        let msg = format!(
            "called `is_some()` after searching an `Iterator` with {}. This is more succinctly \
             expressed by calling `any()`.",
            search_method
        );
        let search_snippet = snippet(cx, search_args[1].span, "..");
        if search_snippet.lines().count() <= 1 {
            // suggest `any(|x| ..)` instead of `any(|&x| ..)` for `find(|&x| ..).is_some()`
            // suggest `any(|..| *..)` instead of `any(|..| **..)` for `find(|..| **..).is_some()`
            let any_search_snippet = if_chain! {
                if search_method == "find";
                if let hir::ExprKind::Closure(_, _, body_id, ..) = search_args[1].kind;
                let closure_body = cx.tcx.hir().body(body_id);
                if let Some(closure_arg) = closure_body.params.get(0);
                then {
                    if let hir::PatKind::Ref(..) = closure_arg.pat.kind {
                        Some(search_snippet.replacen('&', "", 1))
                    } else if let Some(name) = get_arg_name(&closure_arg.pat) {
                        Some(search_snippet.replace(&format!("*{}", name), &name.as_str()))
                    } else {
                        None
                    }
                } else {
                    None
                }
            };
            // add note if not multi-line
            span_lint_and_sugg(
                cx,
                SEARCH_IS_SOME,
                method_span.with_hi(expr.span.hi()),
                &msg,
                "try this",
                format!(
                    "any({})",
                    any_search_snippet.as_ref().map_or(&*search_snippet, String::as_str)
                ),
                Applicability::MachineApplicable,
            );
        } else {
            span_lint(cx, SEARCH_IS_SOME, expr.span, &msg);
        }
    }
}

/// Used for `lint_binary_expr_with_method_call`.
#[derive(Copy, Clone)]
struct BinaryExprInfo<'a> {
    expr: &'a hir::Expr,
    chain: &'a hir::Expr,
    other: &'a hir::Expr,
    eq: bool,
}

/// Checks for the `CHARS_NEXT_CMP` and `CHARS_LAST_CMP` lints.
fn lint_binary_expr_with_method_call(cx: &LateContext<'_, '_>, info: &mut BinaryExprInfo<'_>) {
    macro_rules! lint_with_both_lhs_and_rhs {
        ($func:ident, $cx:expr, $info:ident) => {
            if !$func($cx, $info) {
                ::std::mem::swap(&mut $info.chain, &mut $info.other);
                if $func($cx, $info) {
                    return;
                }
            }
        };
    }

    lint_with_both_lhs_and_rhs!(lint_chars_next_cmp, cx, info);
    lint_with_both_lhs_and_rhs!(lint_chars_last_cmp, cx, info);
    lint_with_both_lhs_and_rhs!(lint_chars_next_cmp_with_unwrap, cx, info);
    lint_with_both_lhs_and_rhs!(lint_chars_last_cmp_with_unwrap, cx, info);
}

/// Wrapper fn for `CHARS_NEXT_CMP` and `CHARS_LAST_CMP` lints.
fn lint_chars_cmp(
    cx: &LateContext<'_, '_>,
    info: &BinaryExprInfo<'_>,
    chain_methods: &[&str],
    lint: &'static Lint,
    suggest: &str,
) -> bool {
    if_chain! {
        if let Some(args) = method_chain_args(info.chain, chain_methods);
        if let hir::ExprKind::Call(ref fun, ref arg_char) = info.other.kind;
        if arg_char.len() == 1;
        if let hir::ExprKind::Path(ref qpath) = fun.kind;
        if let Some(segment) = single_segment_path(qpath);
        if segment.ident.name == sym!(Some);
        then {
            let mut applicability = Applicability::MachineApplicable;
            let self_ty = walk_ptrs_ty(cx.tables.expr_ty_adjusted(&args[0][0]));

            if self_ty.kind != ty::Str {
                return false;
            }

            span_lint_and_sugg(
                cx,
                lint,
                info.expr.span,
                &format!("you should use the `{}` method", suggest),
                "like this",
                format!("{}{}.{}({})",
                        if info.eq { "" } else { "!" },
                        snippet_with_applicability(cx, args[0][0].span, "_", &mut applicability),
                        suggest,
                        snippet_with_applicability(cx, arg_char[0].span, "_", &mut applicability)),
                applicability,
            );

            return true;
        }
    }

    false
}

/// Checks for the `CHARS_NEXT_CMP` lint.
fn lint_chars_next_cmp<'a, 'tcx>(cx: &LateContext<'a, 'tcx>, info: &BinaryExprInfo<'_>) -> bool {
    lint_chars_cmp(cx, info, &["chars", "next"], CHARS_NEXT_CMP, "starts_with")
}

/// Checks for the `CHARS_LAST_CMP` lint.
fn lint_chars_last_cmp<'a, 'tcx>(cx: &LateContext<'a, 'tcx>, info: &BinaryExprInfo<'_>) -> bool {
    if lint_chars_cmp(cx, info, &["chars", "last"], CHARS_LAST_CMP, "ends_with") {
        true
    } else {
        lint_chars_cmp(cx, info, &["chars", "next_back"], CHARS_LAST_CMP, "ends_with")
    }
}

/// Wrapper fn for `CHARS_NEXT_CMP` and `CHARS_LAST_CMP` lints with `unwrap()`.
fn lint_chars_cmp_with_unwrap<'a, 'tcx>(
    cx: &LateContext<'a, 'tcx>,
    info: &BinaryExprInfo<'_>,
    chain_methods: &[&str],
    lint: &'static Lint,
    suggest: &str,
) -> bool {
    if_chain! {
        if let Some(args) = method_chain_args(info.chain, chain_methods);
        if let hir::ExprKind::Lit(ref lit) = info.other.kind;
        if let ast::LitKind::Char(c) = lit.node;
        then {
            let mut applicability = Applicability::MachineApplicable;
            span_lint_and_sugg(
                cx,
                lint,
                info.expr.span,
                &format!("you should use the `{}` method", suggest),
                "like this",
                format!("{}{}.{}('{}')",
                        if info.eq { "" } else { "!" },
                        snippet_with_applicability(cx, args[0][0].span, "_", &mut applicability),
                        suggest,
                        c),
                applicability,
            );

            true
        } else {
            false
        }
    }
}

/// Checks for the `CHARS_NEXT_CMP` lint with `unwrap()`.
fn lint_chars_next_cmp_with_unwrap<'a, 'tcx>(cx: &LateContext<'a, 'tcx>, info: &BinaryExprInfo<'_>) -> bool {
    lint_chars_cmp_with_unwrap(cx, info, &["chars", "next", "unwrap"], CHARS_NEXT_CMP, "starts_with")
}

/// Checks for the `CHARS_LAST_CMP` lint with `unwrap()`.
fn lint_chars_last_cmp_with_unwrap<'a, 'tcx>(cx: &LateContext<'a, 'tcx>, info: &BinaryExprInfo<'_>) -> bool {
    if lint_chars_cmp_with_unwrap(cx, info, &["chars", "last", "unwrap"], CHARS_LAST_CMP, "ends_with") {
        true
    } else {
        lint_chars_cmp_with_unwrap(cx, info, &["chars", "next_back", "unwrap"], CHARS_LAST_CMP, "ends_with")
    }
}

/// lint for length-1 `str`s for methods in `PATTERN_METHODS`
fn lint_single_char_pattern<'a, 'tcx>(cx: &LateContext<'a, 'tcx>, _expr: &'tcx hir::Expr, arg: &'tcx hir::Expr) {
    if_chain! {
        if let hir::ExprKind::Lit(lit) = &arg.kind;
        if let ast::LitKind::Str(r, style) = lit.node;
        if r.as_str().len() == 1;
        then {
            let mut applicability = Applicability::MachineApplicable;
            let snip = snippet_with_applicability(cx, arg.span, "..", &mut applicability);
            let ch = if let ast::StrStyle::Raw(nhash) = style {
                let nhash = nhash as usize;
                // for raw string: r##"a"##
                &snip[(nhash + 2)..(snip.len() - 1 - nhash)]
            } else {
                // for regular string: "a"
                &snip[1..(snip.len() - 1)]
            };
            let hint = format!("'{}'", if ch == "'" { "\\'" } else { ch });
            span_lint_and_sugg(
                cx,
                SINGLE_CHAR_PATTERN,
                arg.span,
                "single-character string constant used as pattern",
                "try using a char instead",
                hint,
                applicability,
            );
        }
    }
}

/// Checks for the `USELESS_ASREF` lint.
fn lint_asref(cx: &LateContext<'_, '_>, expr: &hir::Expr, call_name: &str, as_ref_args: &[hir::Expr]) {
    // when we get here, we've already checked that the call name is "as_ref" or "as_mut"
    // check if the call is to the actual `AsRef` or `AsMut` trait
    if match_trait_method(cx, expr, &paths::ASREF_TRAIT) || match_trait_method(cx, expr, &paths::ASMUT_TRAIT) {
        // check if the type after `as_ref` or `as_mut` is the same as before
        let recvr = &as_ref_args[0];
        let rcv_ty = cx.tables.expr_ty(recvr);
        let res_ty = cx.tables.expr_ty(expr);
        let (base_res_ty, res_depth) = walk_ptrs_ty_depth(res_ty);
        let (base_rcv_ty, rcv_depth) = walk_ptrs_ty_depth(rcv_ty);
        if base_rcv_ty == base_res_ty && rcv_depth >= res_depth {
            // allow the `as_ref` or `as_mut` if it is followed by another method call
            if_chain! {
                if let Some(parent) = get_parent_expr(cx, expr);
                if let hir::ExprKind::MethodCall(_, ref span, _) = parent.kind;
                if span != &expr.span;
                then {
                    return;
                }
            }

            let mut applicability = Applicability::MachineApplicable;
            span_lint_and_sugg(
                cx,
                USELESS_ASREF,
                expr.span,
                &format!("this call to `{}` does nothing", call_name),
                "try this",
                snippet_with_applicability(cx, recvr.span, "_", &mut applicability).to_string(),
                applicability,
            );
        }
    }
}

fn ty_has_iter_method(cx: &LateContext<'_, '_>, self_ref_ty: Ty<'_>) -> Option<(&'static str, &'static str)> {
    has_iter_method(cx, self_ref_ty).map(|ty_name| {
        let mutbl = match self_ref_ty.kind {
            ty::Ref(_, _, mutbl) => mutbl,
            _ => unreachable!(),
        };
        let method_name = match mutbl {
            hir::Mutability::Immutable => "iter",
            hir::Mutability::Mutable => "iter_mut",
        };
        (ty_name, method_name)
    })
}

fn lint_into_iter(cx: &LateContext<'_, '_>, expr: &hir::Expr, self_ref_ty: Ty<'_>, method_span: Span) {
    if !match_trait_method(cx, expr, &paths::INTO_ITERATOR) {
        return;
    }
    if let Some((kind, method_name)) = ty_has_iter_method(cx, self_ref_ty) {
        span_lint_and_sugg(
            cx,
            INTO_ITER_ON_REF,
            method_span,
            &format!(
                "this .into_iter() call is equivalent to .{}() and will not move the {}",
                method_name, kind,
            ),
            "call directly",
            method_name.to_string(),
            Applicability::MachineApplicable,
        );
    }
}

/// lint for `MaybeUninit::uninit().assume_init()` (we already have the latter)
fn lint_maybe_uninit(cx: &LateContext<'_, '_>, expr: &hir::Expr, outer: &hir::Expr) {
    if_chain! {
        if let hir::ExprKind::Call(ref callee, ref args) = expr.kind;
        if args.is_empty();
        if let hir::ExprKind::Path(ref path) = callee.kind;
        if match_qpath(path, &paths::MEM_MAYBEUNINIT_UNINIT);
        if !is_maybe_uninit_ty_valid(cx, cx.tables.expr_ty_adjusted(outer));
        then {
            span_lint(
                cx,
                UNINIT_ASSUMED_INIT,
                outer.span,
                "this call for this type may be undefined behavior"
            );
        }
    }
}

fn is_maybe_uninit_ty_valid(cx: &LateContext<'_, '_>, ty: Ty<'_>) -> bool {
    match ty.kind {
        ty::Array(ref component, _) => is_maybe_uninit_ty_valid(cx, component),
        ty::Tuple(ref types) => types.types().all(|ty| is_maybe_uninit_ty_valid(cx, ty)),
        ty::Adt(ref adt, _) => {
            // needs to be a MaybeUninit
            match_def_path(cx, adt.did, &paths::MEM_MAYBEUNINIT)
        },
        _ => false,
    }
}

fn lint_suspicious_map(cx: &LateContext<'_, '_>, expr: &hir::Expr) {
    span_help_and_lint(
        cx,
        SUSPICIOUS_MAP,
        expr.span,
        "this call to `map()` won't have an effect on the call to `count()`",
        "make sure you did not confuse `map` with `filter`",
    );
}

/// Given a `Result<T, E>` type, return its error type (`E`).
fn get_error_type<'a>(cx: &LateContext<'_, '_>, ty: Ty<'a>) -> Option<Ty<'a>> {
    match ty.kind {
        ty::Adt(_, substs) if match_type(cx, ty, &paths::RESULT) => substs.types().nth(1),
        _ => None,
    }
}

/// This checks whether a given type is known to implement Debug.
fn has_debug_impl<'a, 'b>(ty: Ty<'a>, cx: &LateContext<'b, 'a>) -> bool {
    cx.tcx
        .get_diagnostic_item(sym::debug_trait)
        .map_or(false, |debug| implements_trait(cx, ty, debug, &[]))
}

enum Convention {
    Eq(&'static str),
    StartsWith(&'static str),
}

#[rustfmt::skip]
const CONVENTIONS: [(Convention, &[SelfKind]); 7] = [
    (Convention::Eq("new"), &[SelfKind::No]),
    (Convention::StartsWith("as_"), &[SelfKind::Ref, SelfKind::RefMut]),
    (Convention::StartsWith("from_"), &[SelfKind::No]),
    (Convention::StartsWith("into_"), &[SelfKind::Value]),
    (Convention::StartsWith("is_"), &[SelfKind::Ref, SelfKind::No]),
    (Convention::Eq("to_mut"), &[SelfKind::RefMut]),
    (Convention::StartsWith("to_"), &[SelfKind::Ref]),
];

#[rustfmt::skip]
const TRAIT_METHODS: [(&str, usize, SelfKind, OutType, &str); 30] = [
    ("add", 2, SelfKind::Value, OutType::Any, "std::ops::Add"),
    ("as_mut", 1, SelfKind::RefMut, OutType::Ref, "std::convert::AsMut"),
    ("as_ref", 1, SelfKind::Ref, OutType::Ref, "std::convert::AsRef"),
    ("bitand", 2, SelfKind::Value, OutType::Any, "std::ops::BitAnd"),
    ("bitor", 2, SelfKind::Value, OutType::Any, "std::ops::BitOr"),
    ("bitxor", 2, SelfKind::Value, OutType::Any, "std::ops::BitXor"),
    ("borrow", 1, SelfKind::Ref, OutType::Ref, "std::borrow::Borrow"),
    ("borrow_mut", 1, SelfKind::RefMut, OutType::Ref, "std::borrow::BorrowMut"),
    ("clone", 1, SelfKind::Ref, OutType::Any, "std::clone::Clone"),
    ("cmp", 2, SelfKind::Ref, OutType::Any, "std::cmp::Ord"),
    ("default", 0, SelfKind::No, OutType::Any, "std::default::Default"),
    ("deref", 1, SelfKind::Ref, OutType::Ref, "std::ops::Deref"),
    ("deref_mut", 1, SelfKind::RefMut, OutType::Ref, "std::ops::DerefMut"),
    ("div", 2, SelfKind::Value, OutType::Any, "std::ops::Div"),
    ("drop", 1, SelfKind::RefMut, OutType::Unit, "std::ops::Drop"),
    ("eq", 2, SelfKind::Ref, OutType::Bool, "std::cmp::PartialEq"),
    ("from_iter", 1, SelfKind::No, OutType::Any, "std::iter::FromIterator"),
    ("from_str", 1, SelfKind::No, OutType::Any, "std::str::FromStr"),
    ("hash", 2, SelfKind::Ref, OutType::Unit, "std::hash::Hash"),
    ("index", 2, SelfKind::Ref, OutType::Ref, "std::ops::Index"),
    ("index_mut", 2, SelfKind::RefMut, OutType::Ref, "std::ops::IndexMut"),
    ("into_iter", 1, SelfKind::Value, OutType::Any, "std::iter::IntoIterator"),
    ("mul", 2, SelfKind::Value, OutType::Any, "std::ops::Mul"),
    ("neg", 1, SelfKind::Value, OutType::Any, "std::ops::Neg"),
    ("next", 1, SelfKind::RefMut, OutType::Any, "std::iter::Iterator"),
    ("not", 1, SelfKind::Value, OutType::Any, "std::ops::Not"),
    ("rem", 2, SelfKind::Value, OutType::Any, "std::ops::Rem"),
    ("shl", 2, SelfKind::Value, OutType::Any, "std::ops::Shl"),
    ("shr", 2, SelfKind::Value, OutType::Any, "std::ops::Shr"),
    ("sub", 2, SelfKind::Value, OutType::Any, "std::ops::Sub"),
];

#[rustfmt::skip]
const PATTERN_METHODS: [(&str, usize); 17] = [
    ("contains", 1),
    ("starts_with", 1),
    ("ends_with", 1),
    ("find", 1),
    ("rfind", 1),
    ("split", 1),
    ("rsplit", 1),
    ("split_terminator", 1),
    ("rsplit_terminator", 1),
    ("splitn", 2),
    ("rsplitn", 2),
    ("matches", 1),
    ("rmatches", 1),
    ("match_indices", 1),
    ("rmatch_indices", 1),
    ("trim_start_matches", 1),
    ("trim_end_matches", 1),
];

#[derive(Clone, Copy, PartialEq, Debug)]
enum SelfKind {
    Value,
    Ref,
    RefMut,
    No,
}

impl SelfKind {
    fn matches<'a>(self, cx: &LateContext<'_, 'a>, parent_ty: Ty<'a>, ty: Ty<'a>) -> bool {
        fn matches_value(parent_ty: Ty<'_>, ty: Ty<'_>) -> bool {
            if ty == parent_ty {
                true
            } else if ty.is_box() {
                ty.boxed_ty() == parent_ty
            } else if ty.is_rc() || ty.is_arc() {
                if let ty::Adt(_, substs) = ty.kind {
                    substs.types().next().map_or(false, |t| t == parent_ty)
                } else {
                    false
                }
            } else {
                false
            }
        }

        fn matches_ref<'a>(
            cx: &LateContext<'_, 'a>,
            mutability: hir::Mutability,
            parent_ty: Ty<'a>,
            ty: Ty<'a>,
        ) -> bool {
            if let ty::Ref(_, t, m) = ty.kind {
                return m == mutability && t == parent_ty;
            }

            let trait_path = match mutability {
                hir::Mutability::Immutable => &paths::ASREF_TRAIT,
                hir::Mutability::Mutable => &paths::ASMUT_TRAIT,
            };

            let trait_def_id = match get_trait_def_id(cx, trait_path) {
                Some(did) => did,
                None => return false,
            };
            implements_trait(cx, ty, trait_def_id, &[parent_ty.into()])
        }

        match self {
            Self::Value => matches_value(parent_ty, ty),
            Self::Ref => {
                matches_ref(cx, hir::Mutability::Immutable, parent_ty, ty) || ty == parent_ty && is_copy(cx, ty)
            },
            Self::RefMut => matches_ref(cx, hir::Mutability::Mutable, parent_ty, ty),
            Self::No => ty != parent_ty,
        }
    }

    #[must_use]
    fn description(self) -> &'static str {
        match self {
            Self::Value => "self by value",
            Self::Ref => "self by reference",
            Self::RefMut => "self by mutable reference",
            Self::No => "no self",
        }
    }
}

impl Convention {
    #[must_use]
    fn check(&self, other: &str) -> bool {
        match *self {
            Self::Eq(this) => this == other,
            Self::StartsWith(this) => other.starts_with(this) && this != other,
        }
    }
}

impl fmt::Display for Convention {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match *self {
            Self::Eq(this) => this.fmt(f),
            Self::StartsWith(this) => this.fmt(f).and_then(|_| '*'.fmt(f)),
        }
    }
}

#[derive(Clone, Copy)]
enum OutType {
    Unit,
    Bool,
    Any,
    Ref,
}

impl OutType {
    fn matches(self, cx: &LateContext<'_, '_>, ty: &hir::FunctionRetTy) -> bool {
        let is_unit = |ty: &hir::Ty| SpanlessEq::new(cx).eq_ty_kind(&ty.kind, &hir::TyKind::Tup(vec![].into()));
        match (self, ty) {
            (Self::Unit, &hir::DefaultReturn(_)) => true,
            (Self::Unit, &hir::Return(ref ty)) if is_unit(ty) => true,
            (Self::Bool, &hir::Return(ref ty)) if is_bool(ty) => true,
            (Self::Any, &hir::Return(ref ty)) if !is_unit(ty) => true,
            (Self::Ref, &hir::Return(ref ty)) => matches!(ty.kind, hir::TyKind::Rptr(_, _)),
            _ => false,
        }
    }
}

fn is_bool(ty: &hir::Ty) -> bool {
    if let hir::TyKind::Path(ref p) = ty.kind {
        match_qpath(p, &["bool"])
    } else {
        false
    }
}

// Returns `true` if `expr` contains a return expression
fn contains_return(expr: &hir::Expr) -> bool {
    struct RetCallFinder {
        found: bool,
    }

    impl<'tcx> intravisit::Visitor<'tcx> for RetCallFinder {
        fn visit_expr(&mut self, expr: &'tcx hir::Expr) {
            if self.found {
                return;
            }
            if let hir::ExprKind::Ret(..) = &expr.kind {
                self.found = true;
            } else {
                intravisit::walk_expr(self, expr);
            }
        }

        fn nested_visit_map<'this>(&'this mut self) -> intravisit::NestedVisitorMap<'this, 'tcx> {
            intravisit::NestedVisitorMap::None
        }
    }

    let mut visitor = RetCallFinder { found: false };
    visitor.visit_expr(expr);
    visitor.found
}
