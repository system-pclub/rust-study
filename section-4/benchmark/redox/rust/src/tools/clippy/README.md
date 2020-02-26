# Clippy

[![Build Status](https://travis-ci.com/rust-lang/rust-clippy.svg?branch=master)](https://travis-ci.com/rust-lang/rust-clippy)
[![Windows Build status](https://ci.appveyor.com/api/projects/status/id677xpw1dguo7iw?svg=true)](https://ci.appveyor.com/project/rust-lang-libs/rust-clippy)
[![License: MIT OR Apache-2.0](https://img.shields.io/crates/l/clippy.svg)](#license)

A collection of lints to catch common mistakes and improve your [Rust](https://github.com/rust-lang/rust) code.

[There are 333 lints included in this crate!](https://rust-lang.github.io/rust-clippy/master/index.html)

We have a bunch of lint categories to allow you to choose how much Clippy is supposed to ~~annoy~~ help you:

* `clippy::all` (everything that is on by default: all the categories below except for `nursery`, `pedantic`, and `cargo`)
* `clippy::correctness` (code that is just **outright wrong** or **very very useless**, causes hard errors by default)
* `clippy::style` (code that should be written in a more idiomatic way)
* `clippy::complexity` (code that does something simple but in a complex way)
* `clippy::perf` (code that can be written in a faster way)
* `clippy::pedantic` (lints which are rather strict, off by default)
* `clippy::nursery` (new lints that aren't quite ready yet, off by default)
* `clippy::cargo` (checks against the cargo manifest, off by default)

More to come, please [file an issue](https://github.com/rust-lang/rust-clippy/issues) if you have ideas!

Only the following of those categories are enabled by default:

* `clippy::style`
* `clippy::correctness`
* `clippy::complexity`
* `clippy::perf`

Other categories need to be enabled in order for their lints to be executed.

The [lint list](https://rust-lang.github.io/rust-clippy/master/index.html) also contains "restriction lints", which are for things which are usually not considered "bad", but may be useful to turn on in specific cases. These should be used very selectively, if at all.

Table of contents:

*   [Usage instructions](#usage)
*   [Configuration](#configuration)
*   [Contributing](#contributing)
*   [License](#license)

## Usage

Since this is a tool for helping the developer of a library or application
write better code, it is recommended not to include Clippy as a hard dependency.
Options include using it as an optional dependency, as a cargo subcommand, or
as an included feature during build. These options are detailed below.

### As a cargo subcommand (`cargo clippy`)

One way to use Clippy is by installing Clippy through rustup as a cargo
subcommand.

#### Step 1: Install rustup

You can install [rustup](https://rustup.rs/) on supported platforms. This will help
us install Clippy and its dependencies.

If you already have rustup installed, update to ensure you have the latest
rustup and compiler:

```terminal
rustup update
```

#### Step 2: Install Clippy

Once you have rustup and the latest stable release (at least Rust 1.29) installed, run the following command:

```terminal
rustup component add clippy
```
If it says that it can't find the `clippy` component, please run `rustup self update`.

#### Step 3: Run Clippy

Now you can run Clippy by invoking the following command:

```terminal
cargo clippy
```

#### Automatically applying Clippy suggestions

Some Clippy lint suggestions can be automatically applied by `cargo fix`.
Note that this is still experimental and only supported on the nightly channel:

```terminal
cargo fix -Z unstable-options --clippy
```

### Running Clippy from the command line without installing it

To have cargo compile your crate with Clippy without Clippy installation
in your code, you can use:

```terminal
cargo run --bin cargo-clippy --manifest-path=path_to_clippys_Cargo.toml
```

*Note:* Be sure that Clippy was compiled with the same version of rustc that cargo invokes here!

### Travis CI

You can add Clippy to Travis CI in the same way you use it locally:

```yml
language: rust
rust:
  - stable
  - beta
before_script:
  - rustup component add clippy
script:
  - cargo clippy
  # if you want the build job to fail when encountering warnings, use
  - cargo clippy -- -D warnings
  # in order to also check tests and non-default crate features, use
  - cargo clippy --all-targets --all-features -- -D warnings
  - cargo test
  # etc.
```

If you are on nightly, It might happen that Clippy is not available for a certain nightly release.
In this case you can try to conditionally install Clippy from the Git repo.

```yaml
language: rust
rust:
  - nightly
before_script:
   - rustup component add clippy --toolchain=nightly || cargo install --git https://github.com/rust-lang/rust-clippy/ --force clippy
   # etc.
```

Note that adding `-D warnings` will cause your build to fail if **any** warnings are found in your code.
That includes warnings found by rustc (e.g. `dead_code`, etc.). If you want to avoid this and only cause
an error for Clippy warnings, use `#![deny(clippy::all)]` in your code or `-D clippy::all` on the command
line. (You can swap `clippy::all` with the specific lint category you are targeting.)

## Configuration

Some lints can be configured in a TOML file named `clippy.toml` or `.clippy.toml`. It contains a basic `variable = value` mapping eg.

```toml
blacklisted-names = ["toto", "tata", "titi"]
cognitive-complexity-threshold = 30
```

See the [list of lints](https://rust-lang.github.io/rust-clippy/master/index.html) for more information about which lints can be configured and the
meaning of the variables.

To deactivate the “for further information visit *lint-link*” message you can
define the `CLIPPY_DISABLE_DOCS_LINKS` environment variable.

### Allowing/denying lints

You can add options to your code to `allow`/`warn`/`deny` Clippy lints:

*   the whole set of `Warn` lints using the `clippy` lint group (`#![deny(clippy::all)]`)

*   all lints using both the `clippy` and `clippy::pedantic` lint groups (`#![deny(clippy::all)]`,
    `#![deny(clippy::pedantic)]`). Note that `clippy::pedantic` contains some very aggressive
    lints prone to false positives.

*   only some lints (`#![deny(clippy::single_match, clippy::box_vec)]`, etc.)

*   `allow`/`warn`/`deny` can be limited to a single function or module using `#[allow(...)]`, etc.

Note: `deny` produces errors instead of warnings.

If you do not want to include your lint levels in your code, you can globally enable/disable lints by passing extra flags to Clippy during the run: `cargo clippy -- -A clippy::lint_name` will run Clippy with `lint_name` disabled and `cargo clippy -- -W clippy::lint_name` will run it with that enabled. This also works with lint groups. For example you can run Clippy with warnings for all lints enabled: `cargo clippy -- -W clippy::pedantic`

## Contributing

If you want to contribute to Clippy, you can find more information in [CONTRIBUTING.md](https://github.com/rust-lang/rust-clippy/blob/master/CONTRIBUTING.md).

## License

Copyright 2014-2019 The Rust Project Developers

Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
[https://www.apache.org/licenses/LICENSE-2.0](https://www.apache.org/licenses/LICENSE-2.0)> or the MIT license
<LICENSE-MIT or [https://opensource.org/licenses/MIT](https://opensource.org/licenses/MIT)>, at your
option. All files in the project carrying such notice may not be
copied, modified, or distributed except according to those terms.
