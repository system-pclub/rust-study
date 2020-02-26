# Change Log

All notable changes to this project will be documented in this file.
See [Changelog Update](doc/changelog_update.md) if you want to update this
document.

## Unreleased / In Rust Beta or Nightly

[3aea860...master](https://github.com/rust-lang/rust-clippy/compare/3aea860...master)

## Rust 1.39

Current Beta

## Rust 1.38

Current stable, released 2019-09-26

[e3cb40e...3aea860](https://github.com/rust-lang/rust-clippy/compare/e3cb40e...3aea860)

* New Lints:
  * [`main_recursion`] [#4203](https://github.com/rust-lang/rust-clippy/pull/4203)
  * [`inherent_to_string`] [#4259](https://github.com/rust-lang/rust-clippy/pull/4259)
  * [`inherent_to_string_shadow_display`] [#4259](https://github.com/rust-lang/rust-clippy/pull/4259)
  * [`type_repetition_in_bounds`] [#3766](https://github.com/rust-lang/rust-clippy/pull/3766)
  * [`try_err`] [#4222](https://github.com/rust-lang/rust-clippy/pull/4222)
* Move `{unnnecessary,panicking}_unwrap` out of nursery [#4307](https://github.com/rust-lang/rust-clippy/pull/4307)
* Extend the `use_self` lint to suggest uses of `Self::Variant` [#4308](https://github.com/rust-lang/rust-clippy/pull/4308)
* Improve suggestion for needless return [#4262](https://github.com/rust-lang/rust-clippy/pull/4262)
* Add auto-fixable suggestion for `let_unit` [#4337](https://github.com/rust-lang/rust-clippy/pull/4337)
* Fix false positive in `pub_enum_variant_names` and `enum_variant_names` [#4345](https://github.com/rust-lang/rust-clippy/pull/4345)
* Fix false positive in `cast_ptr_alignment` [#4257](https://github.com/rust-lang/rust-clippy/pull/4257)
* Fix false positive in `string_lit_as_bytes` [#4233](https://github.com/rust-lang/rust-clippy/pull/4233)
* Fix false positive in `needless_lifetimes` [#4266](https://github.com/rust-lang/rust-clippy/pull/4266)
* Fix false positive in `float_cmp` [#4275](https://github.com/rust-lang/rust-clippy/pull/4275)
* Fix false positives in `needless_return` [#4274](https://github.com/rust-lang/rust-clippy/pull/4274)
* Fix false negative in `match_same_arms` [#4246](https://github.com/rust-lang/rust-clippy/pull/4246)
* Fix incorrect suggestion for `needless_bool` [#4335](https://github.com/rust-lang/rust-clippy/pull/4335)
* Improve suggestion for `cast_ptr_alignment` [#4257](https://github.com/rust-lang/rust-clippy/pull/4257)
* Improve suggestion for `single_char_literal` [#4361](https://github.com/rust-lang/rust-clippy/pull/4361)
* Improve suggestion for `len_zero` [#4314](https://github.com/rust-lang/rust-clippy/pull/4314)
* Fix ICE in `implicit_hasher` [#4268](https://github.com/rust-lang/rust-clippy/pull/4268)
* Fix allow bug in `trivially_copy_pass_by_ref` [#4250](https://github.com/rust-lang/rust-clippy/pull/4250)

## Rust 1.37

Released 2019-08-15

[082cfa7...e3cb40e](https://github.com/rust-lang/rust-clippy/compare/082cfa7...e3cb40e)

* New Lints:
  * [`checked_conversions`] [#4088](https://github.com/rust-lang/rust-clippy/pull/4088)
  * [`get_last_with_len`] [#3832](https://github.com/rust-lang/rust-clippy/pull/3832)
  * [`integer_division`] [#4195](https://github.com/rust-lang/rust-clippy/pull/4195)
* Renamed Lint: `const_static_lifetime` is now called [`redundant_static_lifetimes`].
  The lint now covers statics in addition to consts [#4162](https://github.com/rust-lang/rust-clippy/pull/4162)
* [`match_same_arms`] now warns for all identical arms, instead of only the first one [#4102](https://github.com/rust-lang/rust-clippy/pull/4102)
* [`needless_return`] now works with void functions [#4220](https://github.com/rust-lang/rust-clippy/pull/4220)
* Fix false positive in [`redundant_closure`] [#4190](https://github.com/rust-lang/rust-clippy/pull/4190)
* Fix false positive in [`useless_attribute`] [#4107](https://github.com/rust-lang/rust-clippy/pull/4107)
* Fix incorrect suggestion for [`float_cmp`] [#4214](https://github.com/rust-lang/rust-clippy/pull/4214)
* Add suggestions for [`print_with_newline`] and [`write_with_newline`] [#4136](https://github.com/rust-lang/rust-clippy/pull/4136)
* Improve suggestions for [`option_map_unwrap_or_else`] and [`result_map_unwrap_or_else`] [#4164](https://github.com/rust-lang/rust-clippy/pull/4164)
* Improve suggestions for [`non_ascii_literal`] [#4119](https://github.com/rust-lang/rust-clippy/pull/4119)
* Improve diagnostics for [`let_and_return`] [#4137](https://github.com/rust-lang/rust-clippy/pull/4137)
* Improve diagnostics for [`trivially_copy_pass_by_ref`] [#4071](https://github.com/rust-lang/rust-clippy/pull/4071)
* Add macro check for [`unreadable_literal`] [#4099](https://github.com/rust-lang/rust-clippy/pull/4099)

## Rust 1.36

Released 2019-07-04

[eb9f9b1...082cfa7](https://github.com/rust-lang/rust-clippy/compare/eb9f9b1...082cfa7)

 * New lints: [`find_map`], [`filter_map_next`] [#4039](https://github.com/rust-lang/rust-clippy/pull/4039)
 * New lint: [`path_buf_push_overwrite`] [#3954](https://github.com/rust-lang/rust-clippy/pull/3954)
 * Move `path_buf_push_overwrite` to the nursery [#4013](https://github.com/rust-lang/rust-clippy/pull/4013)
 * Split [`redundant_closure`] into [`redundant_closure`] and [`redundant_closure_for_method_calls`] [#4110](https://github.com/rust-lang/rust-clippy/pull/4101)
 * Allow allowing of [`toplevel_ref_arg`] lint [#4007](https://github.com/rust-lang/rust-clippy/pull/4007)
 * Fix false negative in [`or_fun_call`] pertaining to nested constructors [#4084](https://github.com/rust-lang/rust-clippy/pull/4084)
 * Fix false positive in [`or_fn_call`] pertaining to enum variant constructors [#4018](https://github.com/rust-lang/rust-clippy/pull/4018)
 * Fix false positive in [`useless_let_if_seq`] pertaining to interior mutability [#4035](https://github.com/rust-lang/rust-clippy/pull/4035)
 * Fix false positive in [`redundant_closure`] pertaining to non-function types [#4008](https://github.com/rust-lang/rust-clippy/pull/4008)
 * Fix false positive in [`let_and_return`] pertaining to attributes on `let`s [#4024](https://github.com/rust-lang/rust-clippy/pull/4024)
 * Fix false positive in [`module_name_repetitions`] lint pertaining to attributes [#4006](https://github.com/rust-lang/rust-clippy/pull/4006)
 * Fix false positive on [`assertions_on_constants`] pertaining to `debug_assert!` [#3989](https://github.com/rust-lang/rust-clippy/pull/3989)
 * Improve suggestion in [`map_clone`] to suggest `.copied()` where applicable  [#3970](https://github.com/rust-lang/rust-clippy/pull/3970) [#4043](https://github.com/rust-lang/rust-clippy/pull/4043)
 * Improve suggestion for [`search_is_some`] [#4049](https://github.com/rust-lang/rust-clippy/pull/4049)
 * Improve suggestion applicability for [`naive_bytecount`] [#3984](https://github.com/rust-lang/rust-clippy/pull/3984)
 * Improve suggestion applicability for [`while_let_loop`] [#3975](https://github.com/rust-lang/rust-clippy/pull/3975)
 * Improve diagnostics for [`too_many_arguments`] [#4053](https://github.com/rust-lang/rust-clippy/pull/4053)
 * Improve diagnostics for [`cast_lossless`] [#4021](https://github.com/rust-lang/rust-clippy/pull/4021)
 * Deal with macro checks in desugarings better [#4082](https://github.com/rust-lang/rust-clippy/pull/4082)
 * Add macro check for [`unnecessary_cast`]  [#4026](https://github.com/rust-lang/rust-clippy/pull/4026)
 * Remove [`approx_constant`]'s documentation's "Known problems" section. [#4027](https://github.com/rust-lang/rust-clippy/pull/4027)
 * Fix ICE in [`suspicious_else_formatting`] [#3960](https://github.com/rust-lang/rust-clippy/pull/3960)
 * Fix ICE in [`decimal_literal_representation`] [#3931](https://github.com/rust-lang/rust-clippy/pull/3931)


## Rust 1.35

Released 2019-05-20

[1fac380..37f5c1e](https://github.com/rust-lang/rust-clippy/compare/1fac380...37f5c1e)

 * New lint: [`drop_bounds`] to detect `T: Drop` bounds
 * Split [`redundant_closure`] into [`redundant_closure`] and [`redundant_closure_for_method_calls`] [#4110](https://github.com/rust-lang/rust-clippy/pull/4101)
 * Rename `cyclomatic_complexity` to [`cognitive_complexity`], start work on making lint more practical for Rust code
 * Move [`get_unwrap`] to the restriction category
 * Improve suggestions for [`iter_cloned_collect`]
 * Improve suggestions for [`cast_lossless`] to suggest suffixed literals
 * Fix false positives in [`print_with_newline`] and [`write_with_newline`] pertaining to raw strings
 * Fix false positive in [`needless_range_loop`] pertaining to structs without a `.iter()`
 * Fix false positive in [`bool_comparison`] pertaining to non-bool types
 * Fix false positive in [`redundant_closure`] pertaining to differences in borrows
 * Fix false positive in [`option_map_unwrap_or`] on non-copy types
 * Fix false positives in [`missing_const_for_fn`] pertaining to macros and trait method impls
 * Fix false positive in [`needless_pass_by_value`] pertaining to procedural macros
 * Fix false positive in [`needless_continue`] pertaining to loop labels
 * Fix false positive for [`boxed_local`] pertaining to arguments moved into closures
 * Fix false positive for [`use_self`] in nested functions
 * Fix suggestion for [`expect_fun_call`] (https://github.com/rust-lang/rust-clippy/pull/3846)
 * Fix suggestion for [`explicit_counter_loop`] to deal with parenthesizing range variables
 * Fix suggestion for [`single_char_pattern`] to correctly escape single quotes
 * Avoid triggering [`redundant_closure`] in macros
 * ICE fixes: [#3805](https://github.com/rust-lang/rust-clippy/pull/3805), [#3772](https://github.com/rust-lang/rust-clippy/pull/3772), [#3741](https://github.com/rust-lang/rust-clippy/pull/3741)

## Rust 1.34

Released 2019-04-10

[1b89724...1fac380](https://github.com/rust-lang/rust-clippy/compare/1b89724...1fac380)

* New lint: [`assertions_on_constants`] to detect for example `assert!(true)`
* New lint: [`dbg_macro`] to detect uses of the `dbg!` macro
* New lint: [`missing_const_for_fn`] that can suggest functions to be made `const`
* New lint: [`too_many_lines`] to detect functions with excessive LOC. It can be
  configured using the `too-many-lines-threshold` configuration.
* New lint: [`wildcard_enum_match_arm`] to check for wildcard enum matches using `_`
* Expand `redundant_closure` to also work for methods (not only functions)
* Fix ICEs in `vec_box`, `needless_pass_by_value` and `implicit_hasher`
* Fix false positive in `cast_sign_loss`
* Fix false positive in `integer_arithmetic`
* Fix false positive in `unit_arg`
* Fix false positives in `implicit_return`
* Add suggestion to `explicit_write`
* Improve suggestions for `question_mark` lint
* Fix incorrect suggestion for `cast_lossless`
* Fix incorrect suggestion for `expect_fun_call`
* Fix incorrect suggestion for `needless_bool`
* Fix incorrect suggestion for `needless_range_loop`
* Fix incorrect suggestion for `use_self`
* Fix incorrect suggestion for `while_let_on_iterator`
* Clippy is now slightly easier to invoke in non-cargo contexts. See
  [#3665][pull3665] for more details.
* We now have [improved documentation][adding_lints] on how to add new lints

## Rust 1.33

Released 2019-02-26

[b2601be...1b89724](https://github.com/rust-lang/rust-clippy/compare/b2601be...1b89724)

* New lints: [`implicit_return`], [`vec_box`], [`cast_ref_to_mut`]
* The `rust-clippy` repository is now part of the `rust-lang` org.
* Rename `stutter` to `module_name_repetitions`
* Merge `new_without_default_derive` into `new_without_default` lint
* Move `large_digit_groups` from `style` group to `pedantic`
* Expand `bool_comparison` to check for `<`, `<=`, `>`, `>=`, and `!=`
  comparisons against booleans
* Expand `no_effect` to detect writes to constants such as `A_CONST.field = 2`
* Expand `redundant_clone` to work on struct fields
* Expand `suspicious_else_formatting` to detect `if .. {..} {..}`
* Expand `use_self` to work on tuple structs and also in local macros
* Fix ICE in `result_map_unit_fn` and `option_map_unit_fn`
* Fix false positives in `implicit_return`
* Fix false positives in `use_self`
* Fix false negative in `clone_on_copy`
* Fix false positive in `doc_markdown`
* Fix false positive in `empty_loop`
* Fix false positive in `if_same_then_else`
* Fix false positive in `infinite_iter`
* Fix false positive in `question_mark`
* Fix false positive in `useless_asref`
* Fix false positive in `wildcard_dependencies`
* Fix false positive in `write_with_newline`
* Add suggestion to `explicit_write`
* Improve suggestions for `question_mark` lint
* Fix incorrect suggestion for `get_unwrap`

## Rust 1.32

Released 2019-01-17

[2e26fdc2...b2601be](https://github.com/rust-lang/rust-clippy/compare/2e26fdc2...b2601be)

* New lints: [`slow_vector_initialization`], [`mem_discriminant_non_enum`],
  [`redundant_clone`], [`wildcard_dependencies`],
  [`into_iter_on_ref`], [`into_iter_on_array`], [`deprecated_cfg_attr`],
  [`mem_discriminant_non_enum`], [`cargo_common_metadata`]
* Add support for `u128` and `i128` to integer related lints
* Add float support to `mistyped_literal_suffixes`
* Fix false positives in `use_self`
* Fix false positives in `missing_comma`
* Fix false positives in `new_ret_no_self`
* Fix false positives in `possible_missing_comma`
* Fix false positive in `integer_arithmetic` in constant items
* Fix false positive in `needless_borrow`
* Fix false positive in `out_of_bounds_indexing`
* Fix false positive in `new_without_default_derive`
* Fix false positive in `string_lit_as_bytes`
* Fix false negative in `out_of_bounds_indexing`
* Fix false negative in `use_self`. It will now also check existential types
* Fix incorrect suggestion for `redundant_closure_call`
* Fix various suggestions that contained expanded macros
* Fix `bool_comparison` triggering 3 times on on on the same code
* Expand `trivially_copy_pass_by_ref` to work on trait methods
* Improve suggestion for `needless_range_loop`
* Move `needless_pass_by_value` from `pedantic` group to `style`

## Rust 1.31

Released 2018-12-06

[125907ad..2e26fdc2](https://github.com/rust-lang/rust-clippy/compare/125907ad..2e26fdc2)

* Clippy has been relicensed under a dual MIT / Apache license.
  See [#3093](https://github.com/rust-lang/rust-clippy/issues/3093) for more
  information.
* With Rust 1.31, Clippy is no longer available via crates.io. The recommended
  installation method is via `rustup component add clippy`.
* New lints: [`redundant_pattern_matching`], [`unnecessary_filter_map`],
  [`unused_unit`], [`map_flatten`], [`mem_replace_option_with_none`]
* Fix ICE in `if_let_redundant_pattern_matching`
* Fix ICE in `needless_pass_by_value` when encountering a generic function
  argument with a lifetime parameter
* Fix ICE in `needless_range_loop`
* Fix ICE in `single_char_pattern` when encountering a constant value
* Fix false positive in `assign_op_pattern`
* Fix false positive in `boxed_local` on trait implementations
* Fix false positive in `cmp_owned`
* Fix false positive in `collapsible_if` when conditionals have comments
* Fix false positive in `double_parens`
* Fix false positive in `excessive_precision`
* Fix false positive in `explicit_counter_loop`
* Fix false positive in `fn_to_numeric_cast_with_truncation`
* Fix false positive in `map_clone`
* Fix false positive in `new_ret_no_self`
* Fix false positive in `new_without_default` when `new` is unsafe
* Fix false positive in `type_complexity` when using extern types
* Fix false positive in `useless_format`
* Fix false positive in `wrong_self_convention`
* Fix incorrect suggestion for `excessive_precision`
* Fix incorrect suggestion for `expect_fun_call`
* Fix incorrect suggestion for `get_unwrap`
* Fix incorrect suggestion for `useless_format`
* `fn_to_numeric_cast_with_truncation` lint can be disabled again
* Improve suggestions for `manual_memcpy`
* Improve help message for `needless_lifetimes`

## Rust 1.30

Released 2018-10-25

[14207503...125907ad](https://github.com/rust-lang/rust-clippy/compare/14207503...125907ad)

* Deprecate `assign_ops` lint
* New lints: [`mistyped_literal_suffixes`], [`ptr_offset_with_cast`],
  [`needless_collect`], [`copy_iterator`]
* `cargo clippy -V` now includes the Clippy commit hash of the Rust
  Clippy component
* Fix ICE in `implicit_hasher`
* Fix ICE when encountering `println!("{}" a);`
* Fix ICE when encountering a macro call in match statements
* Fix false positive in `default_trait_access`
* Fix false positive in `trivially_copy_pass_by_ref`
* Fix false positive in `similar_names`
* Fix false positive in `redundant_field_name`
* Fix false positive in `expect_fun_call`
* Fix false negative in `identity_conversion`
* Fix false negative in `explicit_counter_loop`
* Fix `range_plus_one` suggestion and false negative
* `print_with_newline` / `write_with_newline`: don't warn about string with several `\n`s in them
* Fix `useless_attribute` to also whitelist `unused_extern_crates`
* Fix incorrect suggestion for `single_char_pattern`
* Improve suggestion for `identity_conversion` lint
* Move `explicit_iter_loop` and `explicit_into_iter_loop` from `style` group to `pedantic`
* Move `range_plus_one` and `range_minus_one` from `nursery` group to `complexity`
* Move `shadow_unrelated` from `restriction` group to `pedantic`
* Move `indexing_slicing` from `pedantic` group to `restriction`

## Rust 1.29

Released 2018-09-13

[v0.0.212...14207503](https://github.com/rust-lang/rust-clippy/compare/v0.0.212...14207503)

* :tada: :tada: **Rust 1.29 is the first stable Rust that includes a bundled Clippy** :tada:
  :tada:
  You can now run `rustup component add clippy-preview` and then `cargo
  clippy` to run Clippy. This should put an end to the continuous nightly
  upgrades for Clippy users.
* Clippy now follows the Rust versioning scheme instead of its own
* Fix ICE when encountering a `while let (..) = x.iter()` construct
* Fix false positives in `use_self`
* Fix false positive in `trivially_copy_pass_by_ref`
* Fix false positive in `useless_attribute` lint
* Fix false positive in `print_literal`
* Fix `use_self` regressions
* Improve lint message for `neg_cmp_op_on_partial_ord`
* Improve suggestion highlight for `single_char_pattern`
* Improve suggestions for various print/write macro lints
* Improve website header

## 0.0.212 (2018-07-10)
* Rustup to *rustc 1.29.0-nightly (e06c87544 2018-07-06)*

## 0.0.211
* Rustup to *rustc 1.28.0-nightly (e3bf634e0 2018-06-28)*

## 0.0.210
* Rustup to *rustc 1.28.0-nightly (01cc982e9 2018-06-24)*

## 0.0.209
* Rustup to *rustc 1.28.0-nightly (523097979 2018-06-18)*

## 0.0.208
* Rustup to *rustc 1.28.0-nightly (86a8f1a63 2018-06-17)*

## 0.0.207
* Rustup to *rustc 1.28.0-nightly (2a0062974 2018-06-09)*

## 0.0.206
* Rustup to *rustc 1.28.0-nightly (5bf68db6e 2018-05-28)*

## 0.0.205
* Rustup to *rustc 1.28.0-nightly (990d8aa74 2018-05-25)*
* Rename `unused_lifetimes` to `extra_unused_lifetimes` because of naming conflict with new rustc lint

## 0.0.204
* Rustup to *rustc 1.28.0-nightly (71e87be38 2018-05-22)*

## 0.0.203
* Rustup to *rustc 1.28.0-nightly (a3085756e 2018-05-19)*
* Clippy attributes are now of the form `clippy::cyclomatic_complexity` instead of `clippy(cyclomatic_complexity)`

## 0.0.202
* Rustup to *rustc 1.28.0-nightly (952f344cd 2018-05-18)*

## 0.0.201
* Rustup to *rustc 1.27.0-nightly (2f2a11dfc 2018-05-16)*

## 0.0.200
* Rustup to *rustc 1.27.0-nightly (9fae15374 2018-05-13)*

## 0.0.199
* Rustup to *rustc 1.27.0-nightly (ff2ac35db 2018-05-12)*

## 0.0.198
* Rustup to *rustc 1.27.0-nightly (acd3871ba 2018-05-10)*

## 0.0.197
* Rustup to *rustc 1.27.0-nightly (428ea5f6b 2018-05-06)*

## 0.0.196
* Rustup to *rustc 1.27.0-nightly (e82261dfb 2018-05-03)*

## 0.0.195
* Rustup to *rustc 1.27.0-nightly (ac3c2288f 2018-04-18)*

## 0.0.194
* Rustup to *rustc 1.27.0-nightly (bd40cbbe1 2018-04-14)*
* New lints: [`cast_ptr_alignment`], [`transmute_ptr_to_ptr`], [`write_literal`], [`write_with_newline`], [`writeln_empty_string`]

## 0.0.193
* Rustup to *rustc 1.27.0-nightly (eeea94c11 2018-04-06)*

## 0.0.192
* Rustup to *rustc 1.27.0-nightly (fb44b4c0e 2018-04-04)*
* New lint: [`print_literal`]

## 0.0.191
* Rustup to *rustc 1.26.0-nightly (ae544ee1c 2018-03-29)*
* Lint audit; categorize lints as style, correctness, complexity, pedantic, nursery, restriction.

## 0.0.190
* Fix a bunch of intermittent cargo bugs

## 0.0.189
* Rustup to *rustc 1.26.0-nightly (5508b2714 2018-03-18)*

## 0.0.188
* Rustup to *rustc 1.26.0-nightly (392645394 2018-03-15)*
* New lint: [`while_immutable_condition`]

## 0.0.187
* Rustup to *rustc 1.26.0-nightly (322d7f7b9 2018-02-25)*
* New lints: [`redundant_field_names`], [`suspicious_arithmetic_impl`], [`suspicious_op_assign_impl`]

## 0.0.186
* Rustup to *rustc 1.25.0-nightly (0c6091fbd 2018-02-04)*
* Various false positive fixes

## 0.0.185
* Rustup to *rustc 1.25.0-nightly (56733bc9f 2018-02-01)*
* New lint: [`question_mark`]

## 0.0.184
* Rustup to *rustc 1.25.0-nightly (90eb44a58 2018-01-29)*
* New lints: [`double_comparisons`], [`empty_line_after_outer_attr`]

## 0.0.183
* Rustup to *rustc 1.25.0-nightly (21882aad7 2018-01-28)*
* New lint: [`misaligned_transmute`]

## 0.0.182
* Rustup to *rustc 1.25.0-nightly (a0dcecff9 2018-01-24)*
* New lint: [`decimal_literal_representation`]

## 0.0.181
* Rustup to *rustc 1.25.0-nightly (97520ccb1 2018-01-21)*
* New lints: [`else_if_without_else`], [`option_option`], [`unit_arg`], [`unnecessary_fold`]
* Removed [`unit_expr`]
* Various false positive fixes for [`needless_pass_by_value`]

## 0.0.180
* Rustup to *rustc 1.25.0-nightly (3f92e8d89 2018-01-14)*

## 0.0.179
* Rustup to *rustc 1.25.0-nightly (61452e506 2018-01-09)*

## 0.0.178
* Rustup to *rustc 1.25.0-nightly (ee220daca 2018-01-07)*

## 0.0.177
* Rustup to *rustc 1.24.0-nightly (250b49205 2017-12-21)*
* New lint: [`match_as_ref`]

## 0.0.176
* Rustup to *rustc 1.24.0-nightly (0077d128d 2017-12-14)*

## 0.0.175
* Rustup to *rustc 1.24.0-nightly (bb42071f6 2017-12-01)*

## 0.0.174
* Rustup to *rustc 1.23.0-nightly (63739ab7b 2017-11-21)*

## 0.0.173
* Rustup to *rustc 1.23.0-nightly (33374fa9d 2017-11-20)*

## 0.0.172
* Rustup to *rustc 1.23.0-nightly (d0f8e2913 2017-11-16)*

## 0.0.171
* Rustup to *rustc 1.23.0-nightly (ff0f5de3b 2017-11-14)*

## 0.0.170
* Rustup to *rustc 1.23.0-nightly (d6b06c63a 2017-11-09)*

## 0.0.169
* Rustup to *rustc 1.23.0-nightly (3b82e4c74 2017-11-05)*
* New lints: [`just_underscores_and_digits`], [`result_map_unwrap_or_else`], [`transmute_bytes_to_str`]

## 0.0.168
* Rustup to *rustc 1.23.0-nightly (f0fe716db 2017-10-30)*

## 0.0.167
* Rustup to *rustc 1.23.0-nightly (90ef3372e 2017-10-29)*
* New lints: [`const_static_lifetime`], [`erasing_op`], [`fallible_impl_from`], [`println_empty_string`], [`useless_asref`]

## 0.0.166
* Rustup to *rustc 1.22.0-nightly (b7960878b 2017-10-18)*
* New lints: [`explicit_write`], [`identity_conversion`], [`implicit_hasher`], [`invalid_ref`], [`option_map_or_none`], [`range_minus_one`], [`range_plus_one`], [`transmute_int_to_bool`], [`transmute_int_to_char`], [`transmute_int_to_float`]

## 0.0.165
* Rust upgrade to rustc 1.22.0-nightly (0e6f4cf51 2017-09-27)
* New lint: [`mut_range_bound`]

## 0.0.164
* Update to *rustc 1.22.0-nightly (6c476ce46 2017-09-25)*
* New lint: [`int_plus_one`]

## 0.0.163
* Update to *rustc 1.22.0-nightly (14039a42a 2017-09-22)*

## 0.0.162
* Update to *rustc 1.22.0-nightly (0701b37d9 2017-09-18)*
* New lint: [`chars_last_cmp`]
* Improved suggestions for [`needless_borrow`], [`ptr_arg`],

## 0.0.161
* Update to *rustc 1.22.0-nightly (539f2083d 2017-09-13)*

## 0.0.160
* Update to *rustc 1.22.0-nightly (dd08c3070 2017-09-12)*

## 0.0.159
* Update to *rustc 1.22.0-nightly (eba374fb2 2017-09-11)*
* New lint: [`clone_on_ref_ptr`]

## 0.0.158
* New lint: [`manual_memcpy`]
* [`cast_lossless`] no longer has redundant parentheses in its suggestions
* Update to *rustc 1.22.0-nightly (dead08cb3 2017-09-08)*

## 0.0.157 - 2017-09-04
* Update to *rustc 1.22.0-nightly (981ce7d8d 2017-09-03)*
* New lint: [`unit_expr`]

## 0.0.156 - 2017-09-03
* Update to *rustc 1.22.0-nightly (744dd6c1d 2017-09-02)*

## 0.0.155
* Update to *rustc 1.21.0-nightly (c11f689d2 2017-08-29)*
* New lint: [`infinite_iter`], [`maybe_infinite_iter`], [`cast_lossless`]

## 0.0.154
* Update to *rustc 1.21.0-nightly (2c0558f63 2017-08-24)*
* Fix [`use_self`] triggering inside derives
* Add support for linting an entire workspace with `cargo clippy --all`
* New lint: [`naive_bytecount`]

## 0.0.153
* Update to *rustc 1.21.0-nightly (8c303ed87 2017-08-20)*
* New lint: [`use_self`]

## 0.0.152
* Update to *rustc 1.21.0-nightly (df511d554 2017-08-14)*

## 0.0.151
* Update to *rustc 1.21.0-nightly (13d94d5fa 2017-08-10)*

## 0.0.150
* Update to *rustc 1.21.0-nightly (215e0b10e 2017-08-08)*

## 0.0.148
* Update to *rustc 1.21.0-nightly (37c7d0ebb 2017-07-31)*
* New lints: [`unreadable_literal`], [`inconsistent_digit_grouping`], [`large_digit_groups`]

## 0.0.147
* Update to *rustc 1.21.0-nightly (aac223f4f 2017-07-30)*

## 0.0.146
* Update to *rustc 1.21.0-nightly (52a330969 2017-07-27)*
* Fixes false positives in `inline_always`
* Fixes false negatives in `panic_params`

## 0.0.145
* Update to *rustc 1.20.0-nightly (afe145d22 2017-07-23)*

## 0.0.144
* Update to *rustc 1.20.0-nightly (086eaa78e 2017-07-15)*

## 0.0.143
* Update to *rustc 1.20.0-nightly (d84693b93 2017-07-09)*
* Fix `cargo clippy` crashing on `dylib` projects
* Fix false positives around `nested_while_let` and `never_loop`

## 0.0.142
* Update to *rustc 1.20.0-nightly (067971139 2017-07-02)*

## 0.0.141
* Rewrite of the `doc_markdown` lint.
* Deprecated [`range_step_by_zero`]
* New lint: [`iterator_step_by_zero`]
* New lint: [`needless_borrowed_reference`]
* Update to *rustc 1.20.0-nightly (69c65d296 2017-06-28)*

## 0.0.140 - 2017-06-16
* Update to *rustc 1.19.0-nightly (258ae6dd9 2017-06-15)*

## 0.0.139 — 2017-06-10
* Update to *rustc 1.19.0-nightly (4bf5c99af 2017-06-10)*
* Fix bugs with for loop desugaring
* Check for [`AsRef`]/[`AsMut`] arguments in [`wrong_self_convention`]

## 0.0.138 — 2017-06-05
* Update to *rustc 1.19.0-nightly (0418fa9d3 2017-06-04)*

## 0.0.137 — 2017-06-05
* Update to *rustc 1.19.0-nightly (6684d176c 2017-06-03)*

## 0.0.136 — 2017—05—26
* Update to *rustc 1.19.0-nightly (557967766 2017-05-26)*

## 0.0.135 — 2017—05—24
* Update to *rustc 1.19.0-nightly (5b13bff52 2017-05-23)*

## 0.0.134 — 2017—05—19
* Update to *rustc 1.19.0-nightly (0ed1ec9f9 2017-05-18)*

## 0.0.133 — 2017—05—14
* Update to *rustc 1.19.0-nightly (826d8f385 2017-05-13)*

## 0.0.132 — 2017—05—05
* Fix various bugs and some ices

## 0.0.131 — 2017—05—04
* Update to *rustc 1.19.0-nightly (2d4ed8e0c 2017-05-03)*

## 0.0.130 — 2017—05—03
* Update to *rustc 1.19.0-nightly (6a5fc9eec 2017-05-02)*

## 0.0.129 — 2017-05-01
* Update to *rustc 1.19.0-nightly (06fb4d256 2017-04-30)*

## 0.0.128 — 2017-04-28
* Update to *rustc 1.18.0-nightly (94e884b63 2017-04-27)*

## 0.0.127 — 2017-04-27
* Update to *rustc 1.18.0-nightly (036983201 2017-04-26)*
* New lint: [`needless_continue`]

## 0.0.126 — 2017-04-24
* Update to *rustc 1.18.0-nightly (2bd4b5c6d 2017-04-23)*

## 0.0.125 — 2017-04-19
* Update to *rustc 1.18.0-nightly (9f2abadca 2017-04-18)*

## 0.0.124 — 2017-04-16
* Update to *rustc 1.18.0-nightly (d5cf1cb64 2017-04-15)*

## 0.0.123 — 2017-04-07
* Fix various false positives

## 0.0.122 — 2017-04-07
* Rustup to *rustc 1.18.0-nightly (91ae22a01 2017-04-05)*
* New lint: [`op_ref`]

## 0.0.121 — 2017-03-21
* Rustup to *rustc 1.17.0-nightly (134c4a0f0 2017-03-20)*

## 0.0.120 — 2017-03-17
* Rustup to *rustc 1.17.0-nightly (0aeb9c129 2017-03-15)*

## 0.0.119 — 2017-03-13
* Rustup to *rustc 1.17.0-nightly (824c9ebbd 2017-03-12)*

## 0.0.118 — 2017-03-05
* Rustup to *rustc 1.17.0-nightly (b1e31766d 2017-03-03)*

## 0.0.117 — 2017-03-01
* Rustup to *rustc 1.17.0-nightly (be760566c 2017-02-28)*

## 0.0.116 — 2017-02-28
* Fix `cargo clippy` on 64 bit windows systems

## 0.0.115 — 2017-02-27
* Rustup to *rustc 1.17.0-nightly (60a0edc6c 2017-02-26)*
* New lints: [`zero_ptr`], [`never_loop`], [`mut_from_ref`]

## 0.0.114 — 2017-02-08
* Rustup to *rustc 1.17.0-nightly (c49d10207 2017-02-07)*
* Tests are now ui tests (testing the exact output of rustc)

## 0.0.113 — 2017-02-04
* Rustup to *rustc 1.16.0-nightly (eedaa94e3 2017-02-02)*
* New lint: [`large_enum_variant`]
* `explicit_into_iter_loop` provides suggestions

## 0.0.112 — 2017-01-27
* Rustup to *rustc 1.16.0-nightly (df8debf6d 2017-01-25)*

## 0.0.111 — 2017-01-21
* Rustup to *rustc 1.16.0-nightly (a52da95ce 2017-01-20)*

## 0.0.110 — 2017-01-20
* Add badges and categories to `Cargo.toml`

## 0.0.109 — 2017-01-19
* Update to *rustc 1.16.0-nightly (c07a6ae77 2017-01-17)*

## 0.0.108 — 2017-01-12
* Update to *rustc 1.16.0-nightly (2782e8f8f 2017-01-12)*

## 0.0.107 — 2017-01-11
* Update regex dependency
* Fix FP when matching `&&mut` by `&ref`
* Reintroduce `for (_, x) in &mut hash_map` -> `for x in hash_map.values_mut()`
* New lints: [`unused_io_amount`], [`forget_ref`], [`short_circuit_statement`]

## 0.0.106 — 2017-01-04
* Fix FP introduced by rustup in [`wrong_self_convention`]

## 0.0.105 — 2017-01-04
* Update to *rustc 1.16.0-nightly (468227129 2017-01-03)*
* New lints: [`deref_addrof`], [`double_parens`], [`pub_enum_variant_names`]
* Fix suggestion in [`new_without_default`]
* FP fix in [`absurd_extreme_comparisons`]

## 0.0.104 — 2016-12-15
* Update to *rustc 1.15.0-nightly (8f02c429a 2016-12-15)*

## 0.0.103 — 2016-11-25
* Update to *rustc 1.15.0-nightly (d5814b03e 2016-11-23)*

## 0.0.102 — 2016-11-24
* Update to *rustc 1.15.0-nightly (3bf2be9ce 2016-11-22)*

## 0.0.101 — 2016-11-23
* Update to *rustc 1.15.0-nightly (7b3eeea22 2016-11-21)*
* New lint: [`string_extend_chars`]

## 0.0.100 — 2016-11-20
* Update to *rustc 1.15.0-nightly (ac635aa95 2016-11-18)*

## 0.0.99 — 2016-11-18
* Update to rustc 1.15.0-nightly (0ed951993 2016-11-14)
* New lint: [`get_unwrap`]

## 0.0.98 — 2016-11-08
* Fixes an issue due to a change in how cargo handles `--sysroot`, which broke `cargo clippy`

## 0.0.97 — 2016-11-03
* For convenience, `cargo clippy` defines a `cargo-clippy` feature. This was
  previously added for a short time under the name `clippy` but removed for
  compatibility.
* `cargo clippy --help` is more helping (and less helpful :smile:)
* Rustup to *rustc 1.14.0-nightly (5665bdf3e 2016-11-02)*
* New lints: [`if_let_redundant_pattern_matching`], [`partialeq_ne_impl`]

## 0.0.96 — 2016-10-22
* Rustup to *rustc 1.14.0-nightly (f09420685 2016-10-20)*
* New lint: [`iter_skip_next`]

## 0.0.95 — 2016-10-06
* Rustup to *rustc 1.14.0-nightly (3210fd5c2 2016-10-05)*

## 0.0.94 — 2016-10-04
* Fixes bustage on Windows due to forbidden directory name

## 0.0.93 — 2016-10-03
* Rustup to *rustc 1.14.0-nightly (144af3e97 2016-10-02)*
* [`option_map_unwrap_or`] and [`option_map_unwrap_or_else`] are now
  allowed by default.
* New lint: [`explicit_into_iter_loop`]

## 0.0.92 — 2016-09-30
* Rustup to *rustc 1.14.0-nightly (289f3a4ca 2016-09-29)*

## 0.0.91 — 2016-09-28
* Rustup to *rustc 1.13.0-nightly (d0623cf7b 2016-09-26)*

## 0.0.90 — 2016-09-09
* Rustup to *rustc 1.13.0-nightly (f1f40f850 2016-09-09)*

## 0.0.89 — 2016-09-06
* Rustup to *rustc 1.13.0-nightly (cbe4de78e 2016-09-05)*

## 0.0.88 — 2016-09-04
* Rustup to *rustc 1.13.0-nightly (70598e04f 2016-09-03)*
* The following lints are not new but were only usable through the `clippy`
  lint groups: [`filter_next`], [`for_loop_over_option`],
  [`for_loop_over_result`] and [`match_overlapping_arm`]. You should now be
  able to `#[allow/deny]` them individually and they are available directly
  through [`cargo clippy`].

## 0.0.87 — 2016-08-31
* Rustup to *rustc 1.13.0-nightly (eac41469d 2016-08-30)*
* New lints: [`builtin_type_shadow`]
* Fix FP in [`zero_prefixed_literal`] and `0b`/`0o`

## 0.0.86 — 2016-08-28
* Rustup to *rustc 1.13.0-nightly (a23064af5 2016-08-27)*
* New lints: [`missing_docs_in_private_items`], [`zero_prefixed_literal`]

## 0.0.85 — 2016-08-19
* Fix ICE with [`useless_attribute`]
* [`useless_attribute`] ignores [`unused_imports`] on `use` statements

## 0.0.84 — 2016-08-18
* Rustup to *rustc 1.13.0-nightly (aef6971ca 2016-08-17)*

## 0.0.83 — 2016-08-17
* Rustup to *rustc 1.12.0-nightly (1bf5fa326 2016-08-16)*
* New lints: [`print_with_newline`], [`useless_attribute`]

## 0.0.82 — 2016-08-17
* Rustup to *rustc 1.12.0-nightly (197be89f3 2016-08-15)*
* New lint: [`module_inception`]

## 0.0.81 — 2016-08-14
* Rustup to *rustc 1.12.0-nightly (1deb02ea6 2016-08-12)*
* New lints: [`eval_order_dependence`], [`mixed_case_hex_literals`], [`unseparated_literal_suffix`]
* False positive fix in [`too_many_arguments`]
* Addition of functionality to [`needless_borrow`]
* Suggestions for [`clone_on_copy`]
* Bug fix in [`wrong_self_convention`]
* Doc improvements

## 0.0.80 — 2016-07-31
* Rustup to *rustc 1.12.0-nightly (1225e122f 2016-07-30)*
* New lints: [`misrefactored_assign_op`], [`serde_api_misuse`]

## 0.0.79 — 2016-07-10
* Rustup to *rustc 1.12.0-nightly (f93aaf84c 2016-07-09)*
* Major suggestions refactoring

## 0.0.78 — 2016-07-02
* Rustup to *rustc 1.11.0-nightly (01411937f 2016-07-01)*
* New lints: [`wrong_transmute`], [`double_neg`], [`filter_map`]
* For compatibility, `cargo clippy` does not defines the `clippy` feature
  introduced in 0.0.76 anymore
* [`collapsible_if`] now considers `if let`

## 0.0.77 — 2016-06-21
* Rustup to *rustc 1.11.0-nightly (5522e678b 2016-06-20)*
* New lints: [`stutter`] and [`iter_nth`]

## 0.0.76 — 2016-06-10
* Rustup to *rustc 1.11.0-nightly (7d2f75a95 2016-06-09)*
* `cargo clippy` now automatically defines the `clippy` feature
* New lint: [`not_unsafe_ptr_arg_deref`]

## 0.0.75 — 2016-06-08
* Rustup to *rustc 1.11.0-nightly (763f9234b 2016-06-06)*

## 0.0.74 — 2016-06-07
* Fix bug with `cargo-clippy` JSON parsing
* Add the `CLIPPY_DISABLE_DOCS_LINKS` environment variable to deactivate the
  “for further information visit *lint-link*” message.

## 0.0.73 — 2016-06-05
* Fix false positives in [`useless_let_if_seq`]

## 0.0.72 — 2016-06-04
* Fix false positives in [`useless_let_if_seq`]

## 0.0.71 — 2016-05-31
* Rustup to *rustc 1.11.0-nightly (a967611d8 2016-05-30)*
* New lint: [`useless_let_if_seq`]

## 0.0.70 — 2016-05-28
* Rustup to *rustc 1.10.0-nightly (7bddce693 2016-05-27)*
* [`invalid_regex`] and [`trivial_regex`] can now warn on `RegexSet::new`,
  `RegexBuilder::new` and byte regexes

## 0.0.69 — 2016-05-20
* Rustup to *rustc 1.10.0-nightly (476fe6eef 2016-05-21)*
* [`used_underscore_binding`] has been made `Allow` temporarily

## 0.0.68 — 2016-05-17
* Rustup to *rustc 1.10.0-nightly (cd6a40017 2016-05-16)*
* New lint: [`unnecessary_operation`]

## 0.0.67 — 2016-05-12
* Rustup to *rustc 1.10.0-nightly (22ac88f1a 2016-05-11)*

## 0.0.66 — 2016-05-11
* New `cargo clippy` subcommand
* New lints: [`assign_op_pattern`], [`assign_ops`], [`needless_borrow`]

## 0.0.65 — 2016-05-08
* Rustup to *rustc 1.10.0-nightly (62e2b2fb7 2016-05-06)*
* New lints: [`float_arithmetic`], [`integer_arithmetic`]

## 0.0.64 — 2016-04-26
* Rustup to *rustc 1.10.0-nightly (645dd013a 2016-04-24)*
* New lints: [`temporary_cstring_as_ptr`], [`unsafe_removed_from_name`], and [`mem_forget`]

## 0.0.63 — 2016-04-08
* Rustup to *rustc 1.9.0-nightly (7979dd608 2016-04-07)*

## 0.0.62 — 2016-04-07
* Rustup to *rustc 1.9.0-nightly (bf5da36f1 2016-04-06)*

## 0.0.61 — 2016-04-03
* Rustup to *rustc 1.9.0-nightly (5ab11d72c 2016-04-02)*
* New lint: [`invalid_upcast_comparisons`]

## 0.0.60 — 2016-04-01
* Rustup to *rustc 1.9.0-nightly (e1195c24b 2016-03-31)*

## 0.0.59 — 2016-03-31
* Rustup to *rustc 1.9.0-nightly (30a3849f2 2016-03-30)*
* New lints: [`logic_bug`], [`nonminimal_bool`]
* Fixed: [`match_same_arms`] now ignores arms with guards
* Improved: [`useless_vec`] now warns on `for … in vec![…]`

## 0.0.58 — 2016-03-27
* Rustup to *rustc 1.9.0-nightly (d5a91e695 2016-03-26)*
* New lint: [`doc_markdown`]

## 0.0.57 — 2016-03-27
* Update to *rustc 1.9.0-nightly (a1e29daf1 2016-03-25)*
* Deprecated lints: [`str_to_string`], [`string_to_string`], [`unstable_as_slice`], [`unstable_as_mut_slice`]
* New lint: [`crosspointer_transmute`]

## 0.0.56 — 2016-03-23
* Update to *rustc 1.9.0-nightly (0dcc413e4 2016-03-22)*
* New lints: [`many_single_char_names`] and [`similar_names`]

## 0.0.55 — 2016-03-21
* Update to *rustc 1.9.0-nightly (02310fd31 2016-03-19)*

## 0.0.54 — 2016-03-16
* Update to *rustc 1.9.0-nightly (c66d2380a 2016-03-15)*

## 0.0.53 — 2016-03-15
* Add a [configuration file]

## ~~0.0.52~~

## 0.0.51 — 2016-03-13
* Add `str` to types considered by [`len_zero`]
* New lints: [`indexing_slicing`]

## 0.0.50 — 2016-03-11
* Update to *rustc 1.9.0-nightly (c9629d61c 2016-03-10)*

## 0.0.49 — 2016-03-09
* Update to *rustc 1.9.0-nightly (eabfc160f 2016-03-08)*
* New lints: [`overflow_check_conditional`], [`unused_label`], [`new_without_default`]

## 0.0.48 — 2016-03-07
* Fixed: ICE in [`needless_range_loop`] with globals

## 0.0.47 — 2016-03-07
* Update to *rustc 1.9.0-nightly (998a6720b 2016-03-07)*
* New lint: [`redundant_closure_call`]

[`AsMut`]: https://doc.rust-lang.org/std/convert/trait.AsMut.html
[`AsRef`]: https://doc.rust-lang.org/std/convert/trait.AsRef.html
[configuration file]: ./rust-clippy#configuration
[pull3665]: https://github.com/rust-lang/rust-clippy/pull/3665
[adding_lints]: https://github.com/rust-lang/rust-clippy/blob/master/doc/adding_lints.md

<!-- begin autogenerated links to lint list -->
[`absurd_extreme_comparisons`]: https://rust-lang.github.io/rust-clippy/master/index.html#absurd_extreme_comparisons
[`almost_swapped`]: https://rust-lang.github.io/rust-clippy/master/index.html#almost_swapped
[`approx_constant`]: https://rust-lang.github.io/rust-clippy/master/index.html#approx_constant
[`assertions_on_constants`]: https://rust-lang.github.io/rust-clippy/master/index.html#assertions_on_constants
[`assign_op_pattern`]: https://rust-lang.github.io/rust-clippy/master/index.html#assign_op_pattern
[`assign_ops`]: https://rust-lang.github.io/rust-clippy/master/index.html#assign_ops
[`bad_bit_mask`]: https://rust-lang.github.io/rust-clippy/master/index.html#bad_bit_mask
[`blacklisted_name`]: https://rust-lang.github.io/rust-clippy/master/index.html#blacklisted_name
[`block_in_if_condition_expr`]: https://rust-lang.github.io/rust-clippy/master/index.html#block_in_if_condition_expr
[`block_in_if_condition_stmt`]: https://rust-lang.github.io/rust-clippy/master/index.html#block_in_if_condition_stmt
[`bool_comparison`]: https://rust-lang.github.io/rust-clippy/master/index.html#bool_comparison
[`borrow_interior_mutable_const`]: https://rust-lang.github.io/rust-clippy/master/index.html#borrow_interior_mutable_const
[`borrowed_box`]: https://rust-lang.github.io/rust-clippy/master/index.html#borrowed_box
[`box_vec`]: https://rust-lang.github.io/rust-clippy/master/index.html#box_vec
[`boxed_local`]: https://rust-lang.github.io/rust-clippy/master/index.html#boxed_local
[`builtin_type_shadow`]: https://rust-lang.github.io/rust-clippy/master/index.html#builtin_type_shadow
[`cargo_common_metadata`]: https://rust-lang.github.io/rust-clippy/master/index.html#cargo_common_metadata
[`cast_lossless`]: https://rust-lang.github.io/rust-clippy/master/index.html#cast_lossless
[`cast_possible_truncation`]: https://rust-lang.github.io/rust-clippy/master/index.html#cast_possible_truncation
[`cast_possible_wrap`]: https://rust-lang.github.io/rust-clippy/master/index.html#cast_possible_wrap
[`cast_precision_loss`]: https://rust-lang.github.io/rust-clippy/master/index.html#cast_precision_loss
[`cast_ptr_alignment`]: https://rust-lang.github.io/rust-clippy/master/index.html#cast_ptr_alignment
[`cast_ref_to_mut`]: https://rust-lang.github.io/rust-clippy/master/index.html#cast_ref_to_mut
[`cast_sign_loss`]: https://rust-lang.github.io/rust-clippy/master/index.html#cast_sign_loss
[`char_lit_as_u8`]: https://rust-lang.github.io/rust-clippy/master/index.html#char_lit_as_u8
[`chars_last_cmp`]: https://rust-lang.github.io/rust-clippy/master/index.html#chars_last_cmp
[`chars_next_cmp`]: https://rust-lang.github.io/rust-clippy/master/index.html#chars_next_cmp
[`checked_conversions`]: https://rust-lang.github.io/rust-clippy/master/index.html#checked_conversions
[`clone_double_ref`]: https://rust-lang.github.io/rust-clippy/master/index.html#clone_double_ref
[`clone_on_copy`]: https://rust-lang.github.io/rust-clippy/master/index.html#clone_on_copy
[`clone_on_ref_ptr`]: https://rust-lang.github.io/rust-clippy/master/index.html#clone_on_ref_ptr
[`cmp_nan`]: https://rust-lang.github.io/rust-clippy/master/index.html#cmp_nan
[`cmp_null`]: https://rust-lang.github.io/rust-clippy/master/index.html#cmp_null
[`cmp_owned`]: https://rust-lang.github.io/rust-clippy/master/index.html#cmp_owned
[`cognitive_complexity`]: https://rust-lang.github.io/rust-clippy/master/index.html#cognitive_complexity
[`collapsible_if`]: https://rust-lang.github.io/rust-clippy/master/index.html#collapsible_if
[`comparison_chain`]: https://rust-lang.github.io/rust-clippy/master/index.html#comparison_chain
[`copy_iterator`]: https://rust-lang.github.io/rust-clippy/master/index.html#copy_iterator
[`crosspointer_transmute`]: https://rust-lang.github.io/rust-clippy/master/index.html#crosspointer_transmute
[`dbg_macro`]: https://rust-lang.github.io/rust-clippy/master/index.html#dbg_macro
[`debug_assert_with_mut_call`]: https://rust-lang.github.io/rust-clippy/master/index.html#debug_assert_with_mut_call
[`decimal_literal_representation`]: https://rust-lang.github.io/rust-clippy/master/index.html#decimal_literal_representation
[`declare_interior_mutable_const`]: https://rust-lang.github.io/rust-clippy/master/index.html#declare_interior_mutable_const
[`default_trait_access`]: https://rust-lang.github.io/rust-clippy/master/index.html#default_trait_access
[`deprecated_cfg_attr`]: https://rust-lang.github.io/rust-clippy/master/index.html#deprecated_cfg_attr
[`deprecated_semver`]: https://rust-lang.github.io/rust-clippy/master/index.html#deprecated_semver
[`deref_addrof`]: https://rust-lang.github.io/rust-clippy/master/index.html#deref_addrof
[`derive_hash_xor_eq`]: https://rust-lang.github.io/rust-clippy/master/index.html#derive_hash_xor_eq
[`diverging_sub_expression`]: https://rust-lang.github.io/rust-clippy/master/index.html#diverging_sub_expression
[`doc_markdown`]: https://rust-lang.github.io/rust-clippy/master/index.html#doc_markdown
[`double_comparisons`]: https://rust-lang.github.io/rust-clippy/master/index.html#double_comparisons
[`double_must_use`]: https://rust-lang.github.io/rust-clippy/master/index.html#double_must_use
[`double_neg`]: https://rust-lang.github.io/rust-clippy/master/index.html#double_neg
[`double_parens`]: https://rust-lang.github.io/rust-clippy/master/index.html#double_parens
[`drop_bounds`]: https://rust-lang.github.io/rust-clippy/master/index.html#drop_bounds
[`drop_copy`]: https://rust-lang.github.io/rust-clippy/master/index.html#drop_copy
[`drop_ref`]: https://rust-lang.github.io/rust-clippy/master/index.html#drop_ref
[`duplicate_underscore_argument`]: https://rust-lang.github.io/rust-clippy/master/index.html#duplicate_underscore_argument
[`duration_subsec`]: https://rust-lang.github.io/rust-clippy/master/index.html#duration_subsec
[`else_if_without_else`]: https://rust-lang.github.io/rust-clippy/master/index.html#else_if_without_else
[`empty_enum`]: https://rust-lang.github.io/rust-clippy/master/index.html#empty_enum
[`empty_line_after_outer_attr`]: https://rust-lang.github.io/rust-clippy/master/index.html#empty_line_after_outer_attr
[`empty_loop`]: https://rust-lang.github.io/rust-clippy/master/index.html#empty_loop
[`enum_clike_unportable_variant`]: https://rust-lang.github.io/rust-clippy/master/index.html#enum_clike_unportable_variant
[`enum_glob_use`]: https://rust-lang.github.io/rust-clippy/master/index.html#enum_glob_use
[`enum_variant_names`]: https://rust-lang.github.io/rust-clippy/master/index.html#enum_variant_names
[`eq_op`]: https://rust-lang.github.io/rust-clippy/master/index.html#eq_op
[`erasing_op`]: https://rust-lang.github.io/rust-clippy/master/index.html#erasing_op
[`eval_order_dependence`]: https://rust-lang.github.io/rust-clippy/master/index.html#eval_order_dependence
[`excessive_precision`]: https://rust-lang.github.io/rust-clippy/master/index.html#excessive_precision
[`exit`]: https://rust-lang.github.io/rust-clippy/master/index.html#exit
[`expect_fun_call`]: https://rust-lang.github.io/rust-clippy/master/index.html#expect_fun_call
[`expl_impl_clone_on_copy`]: https://rust-lang.github.io/rust-clippy/master/index.html#expl_impl_clone_on_copy
[`explicit_counter_loop`]: https://rust-lang.github.io/rust-clippy/master/index.html#explicit_counter_loop
[`explicit_into_iter_loop`]: https://rust-lang.github.io/rust-clippy/master/index.html#explicit_into_iter_loop
[`explicit_iter_loop`]: https://rust-lang.github.io/rust-clippy/master/index.html#explicit_iter_loop
[`explicit_write`]: https://rust-lang.github.io/rust-clippy/master/index.html#explicit_write
[`extend_from_slice`]: https://rust-lang.github.io/rust-clippy/master/index.html#extend_from_slice
[`extra_unused_lifetimes`]: https://rust-lang.github.io/rust-clippy/master/index.html#extra_unused_lifetimes
[`fallible_impl_from`]: https://rust-lang.github.io/rust-clippy/master/index.html#fallible_impl_from
[`filter_map`]: https://rust-lang.github.io/rust-clippy/master/index.html#filter_map
[`filter_map_next`]: https://rust-lang.github.io/rust-clippy/master/index.html#filter_map_next
[`filter_next`]: https://rust-lang.github.io/rust-clippy/master/index.html#filter_next
[`find_map`]: https://rust-lang.github.io/rust-clippy/master/index.html#find_map
[`flat_map_identity`]: https://rust-lang.github.io/rust-clippy/master/index.html#flat_map_identity
[`float_arithmetic`]: https://rust-lang.github.io/rust-clippy/master/index.html#float_arithmetic
[`float_cmp`]: https://rust-lang.github.io/rust-clippy/master/index.html#float_cmp
[`float_cmp_const`]: https://rust-lang.github.io/rust-clippy/master/index.html#float_cmp_const
[`fn_to_numeric_cast`]: https://rust-lang.github.io/rust-clippy/master/index.html#fn_to_numeric_cast
[`fn_to_numeric_cast_with_truncation`]: https://rust-lang.github.io/rust-clippy/master/index.html#fn_to_numeric_cast_with_truncation
[`for_kv_map`]: https://rust-lang.github.io/rust-clippy/master/index.html#for_kv_map
[`for_loop_over_option`]: https://rust-lang.github.io/rust-clippy/master/index.html#for_loop_over_option
[`for_loop_over_result`]: https://rust-lang.github.io/rust-clippy/master/index.html#for_loop_over_result
[`forget_copy`]: https://rust-lang.github.io/rust-clippy/master/index.html#forget_copy
[`forget_ref`]: https://rust-lang.github.io/rust-clippy/master/index.html#forget_ref
[`get_last_with_len`]: https://rust-lang.github.io/rust-clippy/master/index.html#get_last_with_len
[`get_unwrap`]: https://rust-lang.github.io/rust-clippy/master/index.html#get_unwrap
[`identity_conversion`]: https://rust-lang.github.io/rust-clippy/master/index.html#identity_conversion
[`identity_op`]: https://rust-lang.github.io/rust-clippy/master/index.html#identity_op
[`if_let_redundant_pattern_matching`]: https://rust-lang.github.io/rust-clippy/master/index.html#if_let_redundant_pattern_matching
[`if_let_some_result`]: https://rust-lang.github.io/rust-clippy/master/index.html#if_let_some_result
[`if_not_else`]: https://rust-lang.github.io/rust-clippy/master/index.html#if_not_else
[`if_same_then_else`]: https://rust-lang.github.io/rust-clippy/master/index.html#if_same_then_else
[`ifs_same_cond`]: https://rust-lang.github.io/rust-clippy/master/index.html#ifs_same_cond
[`implicit_hasher`]: https://rust-lang.github.io/rust-clippy/master/index.html#implicit_hasher
[`implicit_return`]: https://rust-lang.github.io/rust-clippy/master/index.html#implicit_return
[`inconsistent_digit_grouping`]: https://rust-lang.github.io/rust-clippy/master/index.html#inconsistent_digit_grouping
[`indexing_slicing`]: https://rust-lang.github.io/rust-clippy/master/index.html#indexing_slicing
[`ineffective_bit_mask`]: https://rust-lang.github.io/rust-clippy/master/index.html#ineffective_bit_mask
[`inefficient_to_string`]: https://rust-lang.github.io/rust-clippy/master/index.html#inefficient_to_string
[`infallible_destructuring_match`]: https://rust-lang.github.io/rust-clippy/master/index.html#infallible_destructuring_match
[`infinite_iter`]: https://rust-lang.github.io/rust-clippy/master/index.html#infinite_iter
[`inherent_to_string`]: https://rust-lang.github.io/rust-clippy/master/index.html#inherent_to_string
[`inherent_to_string_shadow_display`]: https://rust-lang.github.io/rust-clippy/master/index.html#inherent_to_string_shadow_display
[`inline_always`]: https://rust-lang.github.io/rust-clippy/master/index.html#inline_always
[`inline_fn_without_body`]: https://rust-lang.github.io/rust-clippy/master/index.html#inline_fn_without_body
[`int_plus_one`]: https://rust-lang.github.io/rust-clippy/master/index.html#int_plus_one
[`integer_arithmetic`]: https://rust-lang.github.io/rust-clippy/master/index.html#integer_arithmetic
[`integer_division`]: https://rust-lang.github.io/rust-clippy/master/index.html#integer_division
[`into_iter_on_array`]: https://rust-lang.github.io/rust-clippy/master/index.html#into_iter_on_array
[`into_iter_on_ref`]: https://rust-lang.github.io/rust-clippy/master/index.html#into_iter_on_ref
[`invalid_ref`]: https://rust-lang.github.io/rust-clippy/master/index.html#invalid_ref
[`invalid_regex`]: https://rust-lang.github.io/rust-clippy/master/index.html#invalid_regex
[`invalid_upcast_comparisons`]: https://rust-lang.github.io/rust-clippy/master/index.html#invalid_upcast_comparisons
[`items_after_statements`]: https://rust-lang.github.io/rust-clippy/master/index.html#items_after_statements
[`iter_cloned_collect`]: https://rust-lang.github.io/rust-clippy/master/index.html#iter_cloned_collect
[`iter_next_loop`]: https://rust-lang.github.io/rust-clippy/master/index.html#iter_next_loop
[`iter_nth`]: https://rust-lang.github.io/rust-clippy/master/index.html#iter_nth
[`iter_skip_next`]: https://rust-lang.github.io/rust-clippy/master/index.html#iter_skip_next
[`iterator_step_by_zero`]: https://rust-lang.github.io/rust-clippy/master/index.html#iterator_step_by_zero
[`just_underscores_and_digits`]: https://rust-lang.github.io/rust-clippy/master/index.html#just_underscores_and_digits
[`large_digit_groups`]: https://rust-lang.github.io/rust-clippy/master/index.html#large_digit_groups
[`large_enum_variant`]: https://rust-lang.github.io/rust-clippy/master/index.html#large_enum_variant
[`len_without_is_empty`]: https://rust-lang.github.io/rust-clippy/master/index.html#len_without_is_empty
[`len_zero`]: https://rust-lang.github.io/rust-clippy/master/index.html#len_zero
[`let_and_return`]: https://rust-lang.github.io/rust-clippy/master/index.html#let_and_return
[`let_unit_value`]: https://rust-lang.github.io/rust-clippy/master/index.html#let_unit_value
[`linkedlist`]: https://rust-lang.github.io/rust-clippy/master/index.html#linkedlist
[`logic_bug`]: https://rust-lang.github.io/rust-clippy/master/index.html#logic_bug
[`main_recursion`]: https://rust-lang.github.io/rust-clippy/master/index.html#main_recursion
[`manual_memcpy`]: https://rust-lang.github.io/rust-clippy/master/index.html#manual_memcpy
[`manual_mul_add`]: https://rust-lang.github.io/rust-clippy/master/index.html#manual_mul_add
[`manual_saturating_arithmetic`]: https://rust-lang.github.io/rust-clippy/master/index.html#manual_saturating_arithmetic
[`manual_swap`]: https://rust-lang.github.io/rust-clippy/master/index.html#manual_swap
[`many_single_char_names`]: https://rust-lang.github.io/rust-clippy/master/index.html#many_single_char_names
[`map_clone`]: https://rust-lang.github.io/rust-clippy/master/index.html#map_clone
[`map_entry`]: https://rust-lang.github.io/rust-clippy/master/index.html#map_entry
[`map_flatten`]: https://rust-lang.github.io/rust-clippy/master/index.html#map_flatten
[`match_as_ref`]: https://rust-lang.github.io/rust-clippy/master/index.html#match_as_ref
[`match_bool`]: https://rust-lang.github.io/rust-clippy/master/index.html#match_bool
[`match_overlapping_arm`]: https://rust-lang.github.io/rust-clippy/master/index.html#match_overlapping_arm
[`match_ref_pats`]: https://rust-lang.github.io/rust-clippy/master/index.html#match_ref_pats
[`match_same_arms`]: https://rust-lang.github.io/rust-clippy/master/index.html#match_same_arms
[`match_wild_err_arm`]: https://rust-lang.github.io/rust-clippy/master/index.html#match_wild_err_arm
[`maybe_infinite_iter`]: https://rust-lang.github.io/rust-clippy/master/index.html#maybe_infinite_iter
[`mem_discriminant_non_enum`]: https://rust-lang.github.io/rust-clippy/master/index.html#mem_discriminant_non_enum
[`mem_forget`]: https://rust-lang.github.io/rust-clippy/master/index.html#mem_forget
[`mem_replace_option_with_none`]: https://rust-lang.github.io/rust-clippy/master/index.html#mem_replace_option_with_none
[`mem_replace_with_uninit`]: https://rust-lang.github.io/rust-clippy/master/index.html#mem_replace_with_uninit
[`min_max`]: https://rust-lang.github.io/rust-clippy/master/index.html#min_max
[`misaligned_transmute`]: https://rust-lang.github.io/rust-clippy/master/index.html#misaligned_transmute
[`misrefactored_assign_op`]: https://rust-lang.github.io/rust-clippy/master/index.html#misrefactored_assign_op
[`missing_const_for_fn`]: https://rust-lang.github.io/rust-clippy/master/index.html#missing_const_for_fn
[`missing_docs_in_private_items`]: https://rust-lang.github.io/rust-clippy/master/index.html#missing_docs_in_private_items
[`missing_inline_in_public_items`]: https://rust-lang.github.io/rust-clippy/master/index.html#missing_inline_in_public_items
[`missing_safety_doc`]: https://rust-lang.github.io/rust-clippy/master/index.html#missing_safety_doc
[`mistyped_literal_suffixes`]: https://rust-lang.github.io/rust-clippy/master/index.html#mistyped_literal_suffixes
[`mixed_case_hex_literals`]: https://rust-lang.github.io/rust-clippy/master/index.html#mixed_case_hex_literals
[`module_inception`]: https://rust-lang.github.io/rust-clippy/master/index.html#module_inception
[`module_name_repetitions`]: https://rust-lang.github.io/rust-clippy/master/index.html#module_name_repetitions
[`modulo_one`]: https://rust-lang.github.io/rust-clippy/master/index.html#modulo_one
[`multiple_crate_versions`]: https://rust-lang.github.io/rust-clippy/master/index.html#multiple_crate_versions
[`multiple_inherent_impl`]: https://rust-lang.github.io/rust-clippy/master/index.html#multiple_inherent_impl
[`must_use_candidate`]: https://rust-lang.github.io/rust-clippy/master/index.html#must_use_candidate
[`must_use_unit`]: https://rust-lang.github.io/rust-clippy/master/index.html#must_use_unit
[`mut_from_ref`]: https://rust-lang.github.io/rust-clippy/master/index.html#mut_from_ref
[`mut_mut`]: https://rust-lang.github.io/rust-clippy/master/index.html#mut_mut
[`mut_range_bound`]: https://rust-lang.github.io/rust-clippy/master/index.html#mut_range_bound
[`mutex_atomic`]: https://rust-lang.github.io/rust-clippy/master/index.html#mutex_atomic
[`mutex_integer`]: https://rust-lang.github.io/rust-clippy/master/index.html#mutex_integer
[`naive_bytecount`]: https://rust-lang.github.io/rust-clippy/master/index.html#naive_bytecount
[`needless_bool`]: https://rust-lang.github.io/rust-clippy/master/index.html#needless_bool
[`needless_borrow`]: https://rust-lang.github.io/rust-clippy/master/index.html#needless_borrow
[`needless_borrowed_reference`]: https://rust-lang.github.io/rust-clippy/master/index.html#needless_borrowed_reference
[`needless_collect`]: https://rust-lang.github.io/rust-clippy/master/index.html#needless_collect
[`needless_continue`]: https://rust-lang.github.io/rust-clippy/master/index.html#needless_continue
[`needless_doctest_main`]: https://rust-lang.github.io/rust-clippy/master/index.html#needless_doctest_main
[`needless_lifetimes`]: https://rust-lang.github.io/rust-clippy/master/index.html#needless_lifetimes
[`needless_pass_by_value`]: https://rust-lang.github.io/rust-clippy/master/index.html#needless_pass_by_value
[`needless_range_loop`]: https://rust-lang.github.io/rust-clippy/master/index.html#needless_range_loop
[`needless_return`]: https://rust-lang.github.io/rust-clippy/master/index.html#needless_return
[`needless_update`]: https://rust-lang.github.io/rust-clippy/master/index.html#needless_update
[`neg_cmp_op_on_partial_ord`]: https://rust-lang.github.io/rust-clippy/master/index.html#neg_cmp_op_on_partial_ord
[`neg_multiply`]: https://rust-lang.github.io/rust-clippy/master/index.html#neg_multiply
[`never_loop`]: https://rust-lang.github.io/rust-clippy/master/index.html#never_loop
[`new_ret_no_self`]: https://rust-lang.github.io/rust-clippy/master/index.html#new_ret_no_self
[`new_without_default`]: https://rust-lang.github.io/rust-clippy/master/index.html#new_without_default
[`no_effect`]: https://rust-lang.github.io/rust-clippy/master/index.html#no_effect
[`non_ascii_literal`]: https://rust-lang.github.io/rust-clippy/master/index.html#non_ascii_literal
[`nonminimal_bool`]: https://rust-lang.github.io/rust-clippy/master/index.html#nonminimal_bool
[`nonsensical_open_options`]: https://rust-lang.github.io/rust-clippy/master/index.html#nonsensical_open_options
[`not_unsafe_ptr_arg_deref`]: https://rust-lang.github.io/rust-clippy/master/index.html#not_unsafe_ptr_arg_deref
[`ok_expect`]: https://rust-lang.github.io/rust-clippy/master/index.html#ok_expect
[`op_ref`]: https://rust-lang.github.io/rust-clippy/master/index.html#op_ref
[`option_and_then_some`]: https://rust-lang.github.io/rust-clippy/master/index.html#option_and_then_some
[`option_expect_used`]: https://rust-lang.github.io/rust-clippy/master/index.html#option_expect_used
[`option_map_or_none`]: https://rust-lang.github.io/rust-clippy/master/index.html#option_map_or_none
[`option_map_unit_fn`]: https://rust-lang.github.io/rust-clippy/master/index.html#option_map_unit_fn
[`option_map_unwrap_or`]: https://rust-lang.github.io/rust-clippy/master/index.html#option_map_unwrap_or
[`option_map_unwrap_or_else`]: https://rust-lang.github.io/rust-clippy/master/index.html#option_map_unwrap_or_else
[`option_option`]: https://rust-lang.github.io/rust-clippy/master/index.html#option_option
[`option_unwrap_used`]: https://rust-lang.github.io/rust-clippy/master/index.html#option_unwrap_used
[`or_fun_call`]: https://rust-lang.github.io/rust-clippy/master/index.html#or_fun_call
[`out_of_bounds_indexing`]: https://rust-lang.github.io/rust-clippy/master/index.html#out_of_bounds_indexing
[`overflow_check_conditional`]: https://rust-lang.github.io/rust-clippy/master/index.html#overflow_check_conditional
[`panic`]: https://rust-lang.github.io/rust-clippy/master/index.html#panic
[`panic_params`]: https://rust-lang.github.io/rust-clippy/master/index.html#panic_params
[`panicking_unwrap`]: https://rust-lang.github.io/rust-clippy/master/index.html#panicking_unwrap
[`partialeq_ne_impl`]: https://rust-lang.github.io/rust-clippy/master/index.html#partialeq_ne_impl
[`path_buf_push_overwrite`]: https://rust-lang.github.io/rust-clippy/master/index.html#path_buf_push_overwrite
[`possible_missing_comma`]: https://rust-lang.github.io/rust-clippy/master/index.html#possible_missing_comma
[`precedence`]: https://rust-lang.github.io/rust-clippy/master/index.html#precedence
[`print_literal`]: https://rust-lang.github.io/rust-clippy/master/index.html#print_literal
[`print_stdout`]: https://rust-lang.github.io/rust-clippy/master/index.html#print_stdout
[`print_with_newline`]: https://rust-lang.github.io/rust-clippy/master/index.html#print_with_newline
[`println_empty_string`]: https://rust-lang.github.io/rust-clippy/master/index.html#println_empty_string
[`ptr_arg`]: https://rust-lang.github.io/rust-clippy/master/index.html#ptr_arg
[`ptr_offset_with_cast`]: https://rust-lang.github.io/rust-clippy/master/index.html#ptr_offset_with_cast
[`pub_enum_variant_names`]: https://rust-lang.github.io/rust-clippy/master/index.html#pub_enum_variant_names
[`question_mark`]: https://rust-lang.github.io/rust-clippy/master/index.html#question_mark
[`range_minus_one`]: https://rust-lang.github.io/rust-clippy/master/index.html#range_minus_one
[`range_plus_one`]: https://rust-lang.github.io/rust-clippy/master/index.html#range_plus_one
[`range_step_by_zero`]: https://rust-lang.github.io/rust-clippy/master/index.html#range_step_by_zero
[`range_zip_with_len`]: https://rust-lang.github.io/rust-clippy/master/index.html#range_zip_with_len
[`redundant_clone`]: https://rust-lang.github.io/rust-clippy/master/index.html#redundant_clone
[`redundant_closure`]: https://rust-lang.github.io/rust-clippy/master/index.html#redundant_closure
[`redundant_closure_call`]: https://rust-lang.github.io/rust-clippy/master/index.html#redundant_closure_call
[`redundant_closure_for_method_calls`]: https://rust-lang.github.io/rust-clippy/master/index.html#redundant_closure_for_method_calls
[`redundant_field_names`]: https://rust-lang.github.io/rust-clippy/master/index.html#redundant_field_names
[`redundant_pattern`]: https://rust-lang.github.io/rust-clippy/master/index.html#redundant_pattern
[`redundant_pattern_matching`]: https://rust-lang.github.io/rust-clippy/master/index.html#redundant_pattern_matching
[`redundant_static_lifetimes`]: https://rust-lang.github.io/rust-clippy/master/index.html#redundant_static_lifetimes
[`ref_in_deref`]: https://rust-lang.github.io/rust-clippy/master/index.html#ref_in_deref
[`regex_macro`]: https://rust-lang.github.io/rust-clippy/master/index.html#regex_macro
[`replace_consts`]: https://rust-lang.github.io/rust-clippy/master/index.html#replace_consts
[`result_expect_used`]: https://rust-lang.github.io/rust-clippy/master/index.html#result_expect_used
[`result_map_unit_fn`]: https://rust-lang.github.io/rust-clippy/master/index.html#result_map_unit_fn
[`result_map_unwrap_or_else`]: https://rust-lang.github.io/rust-clippy/master/index.html#result_map_unwrap_or_else
[`result_unwrap_used`]: https://rust-lang.github.io/rust-clippy/master/index.html#result_unwrap_used
[`reverse_range_loop`]: https://rust-lang.github.io/rust-clippy/master/index.html#reverse_range_loop
[`search_is_some`]: https://rust-lang.github.io/rust-clippy/master/index.html#search_is_some
[`serde_api_misuse`]: https://rust-lang.github.io/rust-clippy/master/index.html#serde_api_misuse
[`shadow_reuse`]: https://rust-lang.github.io/rust-clippy/master/index.html#shadow_reuse
[`shadow_same`]: https://rust-lang.github.io/rust-clippy/master/index.html#shadow_same
[`shadow_unrelated`]: https://rust-lang.github.io/rust-clippy/master/index.html#shadow_unrelated
[`short_circuit_statement`]: https://rust-lang.github.io/rust-clippy/master/index.html#short_circuit_statement
[`should_assert_eq`]: https://rust-lang.github.io/rust-clippy/master/index.html#should_assert_eq
[`should_implement_trait`]: https://rust-lang.github.io/rust-clippy/master/index.html#should_implement_trait
[`similar_names`]: https://rust-lang.github.io/rust-clippy/master/index.html#similar_names
[`single_char_pattern`]: https://rust-lang.github.io/rust-clippy/master/index.html#single_char_pattern
[`single_match`]: https://rust-lang.github.io/rust-clippy/master/index.html#single_match
[`single_match_else`]: https://rust-lang.github.io/rust-clippy/master/index.html#single_match_else
[`slow_vector_initialization`]: https://rust-lang.github.io/rust-clippy/master/index.html#slow_vector_initialization
[`str_to_string`]: https://rust-lang.github.io/rust-clippy/master/index.html#str_to_string
[`string_add`]: https://rust-lang.github.io/rust-clippy/master/index.html#string_add
[`string_add_assign`]: https://rust-lang.github.io/rust-clippy/master/index.html#string_add_assign
[`string_extend_chars`]: https://rust-lang.github.io/rust-clippy/master/index.html#string_extend_chars
[`string_lit_as_bytes`]: https://rust-lang.github.io/rust-clippy/master/index.html#string_lit_as_bytes
[`string_to_string`]: https://rust-lang.github.io/rust-clippy/master/index.html#string_to_string
[`suspicious_arithmetic_impl`]: https://rust-lang.github.io/rust-clippy/master/index.html#suspicious_arithmetic_impl
[`suspicious_assignment_formatting`]: https://rust-lang.github.io/rust-clippy/master/index.html#suspicious_assignment_formatting
[`suspicious_else_formatting`]: https://rust-lang.github.io/rust-clippy/master/index.html#suspicious_else_formatting
[`suspicious_map`]: https://rust-lang.github.io/rust-clippy/master/index.html#suspicious_map
[`suspicious_op_assign_impl`]: https://rust-lang.github.io/rust-clippy/master/index.html#suspicious_op_assign_impl
[`suspicious_unary_op_formatting`]: https://rust-lang.github.io/rust-clippy/master/index.html#suspicious_unary_op_formatting
[`temporary_assignment`]: https://rust-lang.github.io/rust-clippy/master/index.html#temporary_assignment
[`temporary_cstring_as_ptr`]: https://rust-lang.github.io/rust-clippy/master/index.html#temporary_cstring_as_ptr
[`to_digit_is_some`]: https://rust-lang.github.io/rust-clippy/master/index.html#to_digit_is_some
[`todo`]: https://rust-lang.github.io/rust-clippy/master/index.html#todo
[`too_many_arguments`]: https://rust-lang.github.io/rust-clippy/master/index.html#too_many_arguments
[`too_many_lines`]: https://rust-lang.github.io/rust-clippy/master/index.html#too_many_lines
[`toplevel_ref_arg`]: https://rust-lang.github.io/rust-clippy/master/index.html#toplevel_ref_arg
[`transmute_bytes_to_str`]: https://rust-lang.github.io/rust-clippy/master/index.html#transmute_bytes_to_str
[`transmute_int_to_bool`]: https://rust-lang.github.io/rust-clippy/master/index.html#transmute_int_to_bool
[`transmute_int_to_char`]: https://rust-lang.github.io/rust-clippy/master/index.html#transmute_int_to_char
[`transmute_int_to_float`]: https://rust-lang.github.io/rust-clippy/master/index.html#transmute_int_to_float
[`transmute_ptr_to_ptr`]: https://rust-lang.github.io/rust-clippy/master/index.html#transmute_ptr_to_ptr
[`transmute_ptr_to_ref`]: https://rust-lang.github.io/rust-clippy/master/index.html#transmute_ptr_to_ref
[`transmuting_null`]: https://rust-lang.github.io/rust-clippy/master/index.html#transmuting_null
[`trivial_regex`]: https://rust-lang.github.io/rust-clippy/master/index.html#trivial_regex
[`trivially_copy_pass_by_ref`]: https://rust-lang.github.io/rust-clippy/master/index.html#trivially_copy_pass_by_ref
[`try_err`]: https://rust-lang.github.io/rust-clippy/master/index.html#try_err
[`type_complexity`]: https://rust-lang.github.io/rust-clippy/master/index.html#type_complexity
[`type_repetition_in_bounds`]: https://rust-lang.github.io/rust-clippy/master/index.html#type_repetition_in_bounds
[`unicode_not_nfc`]: https://rust-lang.github.io/rust-clippy/master/index.html#unicode_not_nfc
[`unimplemented`]: https://rust-lang.github.io/rust-clippy/master/index.html#unimplemented
[`uninit_assumed_init`]: https://rust-lang.github.io/rust-clippy/master/index.html#uninit_assumed_init
[`unit_arg`]: https://rust-lang.github.io/rust-clippy/master/index.html#unit_arg
[`unit_cmp`]: https://rust-lang.github.io/rust-clippy/master/index.html#unit_cmp
[`unknown_clippy_lints`]: https://rust-lang.github.io/rust-clippy/master/index.html#unknown_clippy_lints
[`unnecessary_cast`]: https://rust-lang.github.io/rust-clippy/master/index.html#unnecessary_cast
[`unnecessary_filter_map`]: https://rust-lang.github.io/rust-clippy/master/index.html#unnecessary_filter_map
[`unnecessary_fold`]: https://rust-lang.github.io/rust-clippy/master/index.html#unnecessary_fold
[`unnecessary_mut_passed`]: https://rust-lang.github.io/rust-clippy/master/index.html#unnecessary_mut_passed
[`unnecessary_operation`]: https://rust-lang.github.io/rust-clippy/master/index.html#unnecessary_operation
[`unnecessary_unwrap`]: https://rust-lang.github.io/rust-clippy/master/index.html#unnecessary_unwrap
[`unneeded_field_pattern`]: https://rust-lang.github.io/rust-clippy/master/index.html#unneeded_field_pattern
[`unneeded_wildcard_pattern`]: https://rust-lang.github.io/rust-clippy/master/index.html#unneeded_wildcard_pattern
[`unreachable`]: https://rust-lang.github.io/rust-clippy/master/index.html#unreachable
[`unreadable_literal`]: https://rust-lang.github.io/rust-clippy/master/index.html#unreadable_literal
[`unsafe_removed_from_name`]: https://rust-lang.github.io/rust-clippy/master/index.html#unsafe_removed_from_name
[`unsafe_vector_initialization`]: https://rust-lang.github.io/rust-clippy/master/index.html#unsafe_vector_initialization
[`unseparated_literal_suffix`]: https://rust-lang.github.io/rust-clippy/master/index.html#unseparated_literal_suffix
[`unsound_collection_transmute`]: https://rust-lang.github.io/rust-clippy/master/index.html#unsound_collection_transmute
[`unstable_as_mut_slice`]: https://rust-lang.github.io/rust-clippy/master/index.html#unstable_as_mut_slice
[`unstable_as_slice`]: https://rust-lang.github.io/rust-clippy/master/index.html#unstable_as_slice
[`unused_collect`]: https://rust-lang.github.io/rust-clippy/master/index.html#unused_collect
[`unused_io_amount`]: https://rust-lang.github.io/rust-clippy/master/index.html#unused_io_amount
[`unused_label`]: https://rust-lang.github.io/rust-clippy/master/index.html#unused_label
[`unused_self`]: https://rust-lang.github.io/rust-clippy/master/index.html#unused_self
[`unused_unit`]: https://rust-lang.github.io/rust-clippy/master/index.html#unused_unit
[`use_debug`]: https://rust-lang.github.io/rust-clippy/master/index.html#use_debug
[`use_self`]: https://rust-lang.github.io/rust-clippy/master/index.html#use_self
[`used_underscore_binding`]: https://rust-lang.github.io/rust-clippy/master/index.html#used_underscore_binding
[`useless_asref`]: https://rust-lang.github.io/rust-clippy/master/index.html#useless_asref
[`useless_attribute`]: https://rust-lang.github.io/rust-clippy/master/index.html#useless_attribute
[`useless_format`]: https://rust-lang.github.io/rust-clippy/master/index.html#useless_format
[`useless_let_if_seq`]: https://rust-lang.github.io/rust-clippy/master/index.html#useless_let_if_seq
[`useless_transmute`]: https://rust-lang.github.io/rust-clippy/master/index.html#useless_transmute
[`useless_vec`]: https://rust-lang.github.io/rust-clippy/master/index.html#useless_vec
[`vec_box`]: https://rust-lang.github.io/rust-clippy/master/index.html#vec_box
[`verbose_bit_mask`]: https://rust-lang.github.io/rust-clippy/master/index.html#verbose_bit_mask
[`while_immutable_condition`]: https://rust-lang.github.io/rust-clippy/master/index.html#while_immutable_condition
[`while_let_loop`]: https://rust-lang.github.io/rust-clippy/master/index.html#while_let_loop
[`while_let_on_iterator`]: https://rust-lang.github.io/rust-clippy/master/index.html#while_let_on_iterator
[`wildcard_dependencies`]: https://rust-lang.github.io/rust-clippy/master/index.html#wildcard_dependencies
[`wildcard_enum_match_arm`]: https://rust-lang.github.io/rust-clippy/master/index.html#wildcard_enum_match_arm
[`write_literal`]: https://rust-lang.github.io/rust-clippy/master/index.html#write_literal
[`write_with_newline`]: https://rust-lang.github.io/rust-clippy/master/index.html#write_with_newline
[`writeln_empty_string`]: https://rust-lang.github.io/rust-clippy/master/index.html#writeln_empty_string
[`wrong_pub_self_convention`]: https://rust-lang.github.io/rust-clippy/master/index.html#wrong_pub_self_convention
[`wrong_self_convention`]: https://rust-lang.github.io/rust-clippy/master/index.html#wrong_self_convention
[`wrong_transmute`]: https://rust-lang.github.io/rust-clippy/master/index.html#wrong_transmute
[`zero_divided_by_zero`]: https://rust-lang.github.io/rust-clippy/master/index.html#zero_divided_by_zero
[`zero_prefixed_literal`]: https://rust-lang.github.io/rust-clippy/master/index.html#zero_prefixed_literal
[`zero_ptr`]: https://rust-lang.github.io/rust-clippy/master/index.html#zero_ptr
[`zero_width_space`]: https://rust-lang.github.io/rust-clippy/master/index.html#zero_width_space
<!-- end autogenerated links to lint list -->
