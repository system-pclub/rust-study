# Adding new tests

**In general, we expect every PR that fixes a bug in rustc to come
accompanied by a regression test of some kind.** This test should fail
in master but pass after the PR. These tests are really useful for
preventing us from repeating the mistakes of the past.

To add a new test, the first thing you generally do is to create a
file, typically a Rust source file. Test files have a particular
structure:

- They should have some kind of
  [comment explaining what the test is about](#explanatory_comment);
- next, they can have one or more [header commands](#header_commands), which
  are special comments that the test interpreter knows how to interpret.
- finally, they have the Rust source. This may have various [error
  annotations](#error_annotations) which indicate expected compilation errors or
  warnings.

Depending on the test suite, there may be some other details to be aware of:
  - For [the `ui` test suite](#ui), you need to generate reference output files.

## What kind of test should I add?

It can be difficult to know what kind of test to use. Here are some
rough heuristics:

- Some tests have specialized needs:
  - need to run gdb or lldb? use the `debuginfo` test suite
  - need to inspect LLVM IR or MIR IR? use the `codegen` or `mir-opt` test
    suites
  - need to run rustdoc? Prefer a `rustdoc` test
  - need to inspect the resulting binary in some way? Then use `run-make`
- For most other things, [a `ui` (or `ui-fulldeps`) test](#ui) is to be
  preferred:
  - `ui` tests subsume both run-pass, compile-fail, and parse-fail tests
  - in the case of warnings or errors, `ui` tests capture the full output,
    which makes it easier to review but also helps prevent "hidden" regressions
    in the output

## Naming your test

We have not traditionally had a lot of structure in the names of
tests.  Moreover, for a long time, the rustc test runner did not
support subdirectories (it now does), so test suites like
[`src/test/ui`] have a huge mess of files in them.  This is not
considered an ideal setup.

[`src/test/ui`]: https://github.com/rust-lang/rust/tree/master/src/test/ui/

For regression tests – basically, some random snippet of code that
came in from the internet – we often name the test after the issue
plus a short description. Ideally, the test should be added to a
directory that helps identify what piece of code is being tested here
(e.g., `src/test/ui/borrowck/issue-54597-reject-move-out-of-borrow-via-pat.rs`)
If you've tried and cannot find a more relevant place,
the test may be added to `src/test/ui/issues/`.
Still, **do include the issue number somewhere**.

When writing a new feature, **create a subdirectory to store your
tests**. For example, if you are implementing RFC 1234 ("Widgets"),
then it might make sense to put the tests in a directory like
`src/test/ui/rfc1234-widgets/`.

In other cases, there may already be a suitable directory. (The proper
directory structure to use is actually an area of active debate.)

<a name="explanatory_comment"></a>

## Comment explaining what the test is about

When you create a test file, **include a comment summarizing the point
of the test at the start of the file**. This should highlight which
parts of the test are more important, and what the bug was that the
test is fixing. Citing an issue number is often very helpful.

This comment doesn't have to be super extensive. Just something like
"Regression test for #18060: match arms were matching in the wrong
order."  might already be enough.

These comments are very useful to others later on when your test
breaks, since they often can highlight what the problem is. They are
also useful if for some reason the tests need to be refactored, since
they let others know which parts of the test were important (often a
test must be rewritten because it no longer tests what is was meant to
test, and then it's useful to know what it *was* meant to test
exactly).

<a name="header_commands"></a>

## Header commands: configuring rustc

Header commands are special comments that the test runner knows how to
interpret.  They must appear before the Rust source in the test. They
are normally put after the short comment that explains the point of
this test. For example, this test uses the `// compile-flags` command
to specify a custom flag to give to rustc when the test is compiled:

```rust,ignore
// Test the behavior of `0 - 1` when overflow checks are disabled.

// compile-flags: -Coverflow-checks=off

fn main() {
    let x = 0 - 1;
    ...
}
```

### Ignoring tests

These are used to ignore the test in some situations, which means the test won't
be compiled or run.

* `ignore-X` where `X` is a target detail or stage will ignore the
  test accordingly (see below)
* `only-X` is like `ignore-X`, but will *only* run the test on that
  target or stage
* `ignore-pretty` will not compile the pretty-printed test (this is
  done to test the pretty-printer, but might not always work)
* `ignore-test` always ignores the test
* `ignore-lldb` and `ignore-gdb` will skip a debuginfo test on that
  debugger.
* `ignore-gdb-version` can be used to ignore the test when certain gdb
  versions are used

Some examples of `X` in `ignore-X`:

* Architecture: `aarch64`, `arm`, `asmjs`, `mips`, `wasm32`, `x86_64`,
  `x86`, ...
* OS: `android`, `emscripten`, `freebsd`, `ios`, `linux`, `macos`,
  `windows`, ...
* Environment (fourth word of the target triple): `gnu`, `msvc`,
  `musl`.
* Pointer width: `32bit`, `64bit`.
* Stage: `stage0`, `stage1`, `stage2`.

### Other Header Commands

Here is a list of other header commands. This list is not
exhaustive. Header commands can generally be found by browsing the
`TestProps` structure found in [`header.rs`] from the compiletest
source.

* `run-rustfix` for UI tests, indicates that the test produces
  structured suggestions. The test writer should create a `.fixed`
  file, which contains the source with the suggestions applied.
  When the test is run, compiletest first checks that the correct
  lint/warning is generated. Then, it applies the suggestion and
  compares against `.fixed` (they must match). Finally, the fixed
  source is compiled, and this compilation is required to succeed.
  The `.fixed` file can also be generated automatically with the
  `--bless` option, discussed [below](#bless).
* `min-gdb-version` specifies the minimum gdb version required for
  this test; see also `ignore-gdb-version`
* `min-lldb-version` specifies the minimum lldb version required for
  this test
* `rust-lldb` causes the lldb part of the test to only be run if the
  lldb in use contains the Rust plugin
* `no-system-llvm` causes the test to be ignored if the system llvm is used
* `min-llvm-version` specifies the minimum llvm version required for
  this test
* `min-system-llvm-version` specifies the minimum system llvm version
  required for this test; the test is ignored if the system llvm is in
  use and it doesn't meet the minimum version.  This is useful when an
  llvm feature has been backported to rust-llvm
* `ignore-llvm-version` can be used to skip the test when certain LLVM
  versions are used.  This takes one or two arguments; the first
  argument is the first version to ignore.  If no second argument is
  given, all subsequent versions are ignored; otherwise, the second
  argument is the last version to ignore.
* `build-pass` for UI tests, indicates that the test is supposed to
  successfully compile and link, as opposed to the default where the test is
  supposed to error out.
* `compile-flags` passes extra command-line args to the compiler,
  e.g. `compile-flags -g` which forces debuginfo to be enabled.
* `should-fail` indicates that the test should fail; used for "meta
  testing", where we test the compiletest program itself to check that
  it will generate errors in appropriate scenarios. This header is
  ignored for pretty-printer tests.
* `gate-test-X` where `X` is a feature marks the test as "gate test"
  for feature X.  Such tests are supposed to ensure that the compiler
  errors when usage of a gated feature is attempted without the proper
  `#![feature(X)]` tag.  Each unstable lang feature is required to
  have a gate test.

[`header.rs`]: https://github.com/rust-lang/rust/tree/master/src/tools/compiletest/src/header.rs

<a name="error_annotations"></a>

## Error annotations

Error annotations specify the errors that the compiler is expected to
emit. They are "attached" to the line in source where the error is
located.

* `~`: Associates the following error level and message with the
  current line
* `~|`: Associates the following error level and message with the same
  line as the previous comment
* `~^`: Associates the following error level and message with the
  previous line. Each caret (`^`) that you add adds a line to this, so
  `~^^^^^^^` is seven lines up.

The error levels that you can have are:

1. `ERROR`
2. `WARNING`
3. `NOTE`
4. `HELP` and `SUGGESTION`*

\* **Note**: `SUGGESTION` must follow immediately after `HELP`.

## Revisions

Certain classes of tests support "revisions" (as of the time of this
writing, this includes compile-fail, run-fail, and
incremental, though incremental tests are somewhat
different). Revisions allow a single test file to be used for multiple
tests. This is done by adding a special header at the top of the file:

```rust
// revisions: foo bar baz
```

This will result in the test being compiled (and tested) three times,
once with `--cfg foo`, once with `--cfg bar`, and once with `--cfg
baz`. You can therefore use `#[cfg(foo)]` etc within the test to tweak
each of these results.

You can also customize headers and expected error messages to a particular
revision. To do this, add `[foo]` (or `bar`, `baz`, etc) after the `//`
comment, like so:

```rust
// A flag to pass in only for cfg `foo`:
//[foo]compile-flags: -Z verbose

#[cfg(foo)]
fn test_foo() {
    let x: usize = 32_u32; //[foo]~ ERROR mismatched types
}
```

Note that not all headers have meaning when customized to a revision.
For example, the `ignore-test` header (and all "ignore" headers)
currently only apply to the test as a whole, not to particular
revisions. The only headers that are intended to really work when
customized to a revision are error patterns and compiler flags.

<a name="ui"></a>

## Guide to the UI tests

The UI tests are intended to capture the compiler's complete output,
so that we can test all aspects of the presentation. They work by
compiling a file (e.g., [`ui/hello_world/main.rs`][hw-main]),
capturing the output, and then applying some normalization (see
below). This normalized result is then compared against reference
files named `ui/hello_world/main.stderr` and
`ui/hello_world/main.stdout`. If either of those files doesn't exist,
the output must be empty (that is actually the case for
[this particular test][hw]). If the test run fails, we will print out
the current output, but it is also saved in
`build/<target-triple>/test/ui/hello_world/main.stdout` (this path is
printed as part of the test failure message), so you can run `diff`
and so forth.

[hw-main]: https://github.com/rust-lang/rust/blob/master/src/test/ui/hello_world/main.rs
[hw]: https://github.com/rust-lang/rust/blob/master/src/test/ui/hello_world/

### Tests that do not result in compile errors

By default, a UI test is expected **not to compile** (in which case,
it should contain at least one `//~ ERROR` annotation). However, you
can also make UI tests where compilation is expected to succeed, and
you can even run the resulting program. Just add one of the following
[header commands](#header_commands):

- `// check-pass` - compilation should succeed but skip codegen
  (which is expensive and isn't supposed to fail in most cases)
- `// build-pass` – compilation and linking should succeed but do
  not run the resulting binary
- `// run-pass` – compilation should succeed and we should run the
  resulting binary

<a name="bless"></a>

### Editing and updating the reference files

If you have changed the compiler's output intentionally, or you are
making a new test, you can pass `--bless` to the test subcommand. E.g.
if some tests in `src/test/ui` are failing, you can run

```text
./x.py test --stage 1 src/test/ui --bless
```

to automatically adjust the `.stderr`, `.stdout` or `.fixed` files of
all tests. Of course you can also target just specific tests with the
`--test-args your_test_name` flag, just like when running the tests.

### Normalization

The normalization applied is aimed at eliminating output difference
between platforms, mainly about filenames:

- the test directory is replaced with `$DIR`
- all backslashes (`\`) are converted to forward slashes (`/`) (for Windows)
- all CR LF newlines are converted to LF

Sometimes these built-in normalizations are not enough. In such cases, you
may provide custom normalization rules using the header commands, e.g.

```rust
// normalize-stdout-test: "foo" -> "bar"
// normalize-stderr-32bit: "fn\(\) \(32 bits\)" -> "fn\(\) \($$PTR bits\)"
// normalize-stderr-64bit: "fn\(\) \(64 bits\)" -> "fn\(\) \($$PTR bits\)"
```

This tells the test, on 32-bit platforms, whenever the compiler writes
`fn() (32 bits)` to stderr, it should be normalized to read `fn() ($PTR bits)`
instead. Similar for 64-bit. The replacement is performed by regexes using
default regex flavor provided by `regex` crate.

The corresponding reference file will use the normalized output to test both
32-bit and 64-bit platforms:

```text
...
   |
   = note: source type: fn() ($PTR bits)
   = note: target type: u16 (16 bits)
...
```

Please see [`ui/transmute/main.rs`][mrs] and [`main.stderr`][] for a
concrete usage example.

[mrs]: https://github.com/rust-lang/rust/blob/master/src/test/ui/transmute/main.rs
[`main.stderr`]: https://github.com/rust-lang/rust/blob/master/src/test/ui/transmute/main.stderr

Besides `normalize-stderr-32bit` and `-64bit`, one may use any target
information or stage supported by `ignore-X` here as well (e.g.
`normalize-stderr-windows` or simply `normalize-stderr-test` for unconditional
replacement).
