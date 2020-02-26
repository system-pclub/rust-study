# Contributing to Clippy

Hello fellow Rustacean! Great to see your interest in compiler internals and lints!

**First**: if you're unsure or afraid of _anything_, just ask or submit the issue or pull request anyway. You won't be yelled at for giving it your best effort. The worst that can happen is that you'll be politely asked to change something. We appreciate any sort of contributions, and don't want a wall of rules to get in the way of that.

Clippy welcomes contributions from everyone. There are many ways to contribute to Clippy and the following document explains how
you can contribute and how to get started.
If you have any questions about contributing or need help with anything, feel free to ask questions on issues or
visit the `#clippy` IRC channel on `irc.mozilla.org` or meet us in `#clippy` on [Discord](https://discord.gg/rust-lang).

All contributors are expected to follow the [Rust Code of Conduct](http://www.rust-lang.org/conduct.html).

* [Getting started](#getting-started)
  * [Finding something to fix/improve](#finding-something-to-fiximprove)
* [Writing code](#writing-code)
* [How Clippy works](#how-clippy-works)
* [Fixing nightly build failures](#fixing-build-failures-caused-by-rust)
* [Issue and PR Triage](#issue-and-pr-triage)
* [Bors and Homu](#bors-and-homu)
* [Contributions](#contributions)

## Getting started

High level approach:

1. Find something to fix/improve
2. Change code (likely some file in `clippy_lints/src/`)
3. Run `cargo test` in the root directory and wiggle code until it passes
4. Open a PR (also can be done between 2. and 3. if you run into problems)

### Finding something to fix/improve

All issues on Clippy are mentored, if you want help with a bug just ask @Manishearth, @llogiq, @mcarton or @oli-obk.

Some issues are easier than others. The [`good first issue`](https://github.com/rust-lang/rust-clippy/labels/good%20first%20issue)
label can be used to find the easy issues. If you want to work on an issue, please leave a comment
so that we can assign it to you!

There are also some abandoned PRs, marked with
[`S-inactive-closed`](https://github.com/rust-lang/rust-clippy/pulls?q=is%3Aclosed+label%3AS-inactive-closed).
Pretty often these PRs are nearly completed and just need some extra steps
(formatting, addressing review comments, ...) to be merged. If you want to
complete such a PR, please leave a comment in the PR and open a new one based
on it.

Issues marked [`T-AST`](https://github.com/rust-lang/rust-clippy/labels/T-AST) involve simple
matching of the syntax tree structure, and are generally easier than
[`T-middle`](https://github.com/rust-lang/rust-clippy/labels/T-middle) issues, which involve types
and resolved paths.

[`T-AST`](https://github.com/rust-lang/rust-clippy/labels/T-AST) issues will generally need you to match against a predefined syntax structure. To figure out
how this syntax structure is encoded in the AST, it is recommended to run `rustc -Z ast-json` on an
example of the structure and compare with the
[nodes in the AST docs](https://doc.rust-lang.org/nightly/nightly-rustc/syntax/ast). Usually
the lint will end up to be a nested series of matches and ifs,
[like so](https://github.com/rust-lang/rust-clippy/blob/de5ccdfab68a5e37689f3c950ed1532ba9d652a0/src/misc.rs#L34).

[`E-medium`](https://github.com/rust-lang/rust-clippy/labels/E-medium) issues are generally
pretty easy too, though it's recommended you work on an E-easy issue first. They are mostly classified
as `E-medium`, since they might be somewhat involved code wise, but not difficult per-se.

[`T-middle`](https://github.com/rust-lang/rust-clippy/labels/T-middle) issues can
be more involved and require verifying types. The
[`ty`](https://doc.rust-lang.org/nightly/nightly-rustc/rustc/ty) module contains a
lot of methods that are useful, though one of the most useful would be `expr_ty` (gives the type of
an AST expression). `match_def_path()` in Clippy's `utils` module can also be useful.

## Writing code

Have a look at the [docs for writing lints](doc/adding_lints.md) for more details. [Llogiq's blog post on lints](https://llogiq.github.io/2015/06/04/workflows.html) is also a nice primer
to lint-writing, though it does get into advanced stuff and may be a bit
outdated.

If you want to add a new lint or change existing ones apart from bugfixing, it's
also a good idea to give the [stability guarantees][rfc_stability] and
[lint categories][rfc_lint_cats] sections of the [Clippy 1.0 RFC][clippy_rfc] a
quick read.

## How Clippy works

Clippy is a [rustc compiler plugin][compiler_plugin]. The main entry point is at [`src/lib.rs`][main_entry]. In there, the lint registration is delegated to the [`clippy_lints`][lint_crate] crate.

[`clippy_lints/src/lib.rs`][lint_crate_entry] imports all the different lint modules and registers them with the rustc plugin registry. For example, the [`else_if_without_else`][else_if_without_else] lint is registered like this:

```rust
// ./clippy_lints/src/lib.rs

// ...
pub mod else_if_without_else;
// ...

pub fn register_plugins(reg: &mut rustc_driver::plugin::Registry) {
    // ...
    reg.register_early_lint_pass(box else_if_without_else::ElseIfWithoutElse);
    // ...

    reg.register_lint_group("clippy::restriction", vec![
        // ...
        else_if_without_else::ELSE_IF_WITHOUT_ELSE,
        // ...
    ]);
}
```

The [`plugin::PluginRegistry`][plugin_registry] provides two methods to register lints: [register_early_lint_pass][reg_early_lint_pass] and [register_late_lint_pass][reg_late_lint_pass].
Both take an object that implements an [`EarlyLintPass`][early_lint_pass] or [`LateLintPass`][late_lint_pass] respectively. This is done in every single lint.
It's worth noting that the majority of `clippy_lints/src/lib.rs` is autogenerated by `util/dev update_lints` and you don't have to add anything by hand. When you are writing your own lint, you can use that script to save you some time.

```rust
// ./clippy_lints/src/else_if_without_else.rs

use rustc::lint::{EarlyLintPass, LintArray, LintPass};

// ...

pub struct ElseIfWithoutElse;

// ...

impl EarlyLintPass for ElseIfWithoutElse {
    // ... the functions needed, to make the lint work
}
```

The difference between `EarlyLintPass` and `LateLintPass` is that the methods of the `EarlyLintPass` trait only provide AST information. The methods of the `LateLintPass` trait are executed after type checking and contain type information via the `LateContext` parameter.

That's why the `else_if_without_else` example uses the `register_early_lint_pass` function. Because the [actual lint logic][else_if_without_else] does not depend on any type information.

## Fixing build failures caused by Rust

Clippy will sometimes fail to build from source because building it depends on unstable internal Rust features. Most of the times we have to adapt to the changes and only very rarely there's an actual bug in Rust. Fixing build failures caused by Rust updates, can be a good way to learn about Rust internals.

In order to find out why Clippy does not work properly with a new Rust commit, you can use the [rust-toolstate commit history][toolstate_commit_history].
You will then have to look for the last commit that contains `test-pass -> build-fail` or `test-pass` -> `test-fail` for the `clippy-driver` component. [Here][toolstate_commit] is an example.

The commit message contains a link to the PR. The PRs are usually small enough to discover the breaking API change and if they are bigger, they likely include some discussion that may help you to fix Clippy.

To check if Clippy is available for a specific target platform, you can check
the [rustup component history][rustup_component_history].

If you decide to make Clippy work again with a Rust commit that breaks it,
you probably want to install the latest Rust from master locally and run Clippy
using that version of Rust.

You can use [rustup-toolchain-install-master][rtim] to do that:

```bash
cargo install rustup-toolchain-install-master
rustup-toolchain-install-master --force -n master -c rustc-dev
rustup override set master
cargo test
```

After fixing the build failure on this repository, we can submit a pull request
to [`rust-lang/rust`] to fix the toolstate.

To submit a pull request, you should follow these steps:

```bash
# Assuming you already cloned the rust-lang/rust repo and you're in the correct directory
git submodule update --remote src/tools/clippy
cargo update -p clippy
git add -u
git commit -m "Update Clippy"
./x.py test -i --stage 1 src/tools/clippy # This is optional and should succeed anyway
# Open a PR in rust-lang/rust
```

## Issue and PR triage

Clippy is following the [Rust triage procedure][triage] for issues and pull
requests.

However, we are a smaller project with all contributors being volunteers
currently. Between writing new lints, fixing issues, reviewing pull requests and
responding to issues there may not always be enough time to stay on top of it
all.

Our highest priority is fixing [crashes][l-crash] and [bugs][l-bug]. We don't
want Clippy to crash on your code and we want it to be as reliable as the
suggestions from Rust compiler errors.

## Bors and Homu

We use a bot powered by [Homu][homu] to help automate testing and landing of pull
requests in Clippy. The bot's username is @bors.

You can find the Clippy bors queue [here][homu_queue].

If you have @bors permissions, you can find an overview of the available
commands [here][homu_instructions].


## Contributions

Contributions to Clippy should be made in the form of GitHub pull requests. Each pull request will
be reviewed by a core contributor (someone with permission to land patches) and either landed in the
main tree or given feedback for changes that would be required.

All code in this repository is under the [Apache-2.0](http://www.apache.org/licenses/LICENSE-2.0>)
or the [MIT](http://opensource.org/licenses/MIT) license.

<!-- adapted from https://github.com/servo/servo/blob/master/CONTRIBUTING.md -->

[main_entry]: https://github.com/rust-lang/rust-clippy/blob/c5b39a5917ffc0f1349b6e414fa3b874fdcf8429/src/lib.rs#L14
[lint_crate]: https://github.com/rust-lang/rust-clippy/tree/c5b39a5917ffc0f1349b6e414fa3b874fdcf8429/clippy_lints/src
[lint_crate_entry]: https://github.com/rust-lang/rust-clippy/blob/c5b39a5917ffc0f1349b6e414fa3b874fdcf8429/clippy_lints/src/lib.rs
[else_if_without_else]: https://github.com/rust-lang/rust-clippy/blob/c5b39a5917ffc0f1349b6e414fa3b874fdcf8429/clippy_lints/src/else_if_without_else.rs
[compiler_plugin]: https://doc.rust-lang.org/unstable-book/language-features/plugin.html#lint-plugins
[plugin_registry]: https://doc.rust-lang.org/nightly/nightly-rustc/rustc_plugin_impl/registry/struct.Registry.html
[reg_early_lint_pass]: https://doc.rust-lang.org/nightly/nightly-rustc/rustc_plugin_impl/registry/struct.Registry.html#method.register_early_lint_pass
[reg_late_lint_pass]: https://doc.rust-lang.org/nightly/nightly-rustc/rustc_plugin_impl/registry/struct.Registry.html#method.register_late_lint_pass
[early_lint_pass]: https://doc.rust-lang.org/nightly/nightly-rustc/rustc/lint/trait.EarlyLintPass.html
[late_lint_pass]: https://doc.rust-lang.org/nightly/nightly-rustc/rustc/lint/trait.LateLintPass.html
[toolstate_commit_history]: https://github.com/rust-lang-nursery/rust-toolstate/commits/master
[toolstate_commit]: https://github.com/rust-lang-nursery/rust-toolstate/commit/6ce0459f6bfa7c528ae1886492a3e0b5ef0ee547
[rtim]: https://github.com/kennytm/rustup-toolchain-install-master
[rustup_component_history]: https://mexus.github.io/rustup-components-history
[clippy_rfc]: https://github.com/rust-lang/rfcs/blob/master/text/2476-clippy-uno.md
[rfc_stability]: https://github.com/rust-lang/rfcs/blob/master/text/2476-clippy-uno.md#stability-guarantees
[rfc_lint_cats]: https://github.com/rust-lang/rfcs/blob/master/text/2476-clippy-uno.md#lint-audit-and-categories
[triage]: https://forge.rust-lang.org/triage-procedure.html
[l-crash]: https://github.com/rust-lang/rust-clippy/labels/L-crash%20%3Aboom%3A
[l-bug]: https://github.com/rust-lang/rust-clippy/labels/L-bug%20%3Abeetle%3A
[homu]: https://github.com/servo/homu
[homu_instructions]: https://buildbot2.rust-lang.org/homu/
[homu_queue]: https://buildbot2.rust-lang.org/homu/queue/clippy
[`rust-lang/rust`]: https://github.com/rust-lang/rust
