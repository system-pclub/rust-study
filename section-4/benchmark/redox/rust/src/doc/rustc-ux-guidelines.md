% Rustc UX guidelines

Don't forget the user. Whether human or another program, such as an IDE, a
good user experience with the compiler goes a long way toward making developers'
lives better. We do not want users to be baffled by compiler output or
learn arcane patterns to compile their program.

## Error, Warning, Help, Note Messages

When the compiler detects a problem, it can emit one of the following: an error, a warning,
a note, or a help message.

An `error` is emitted when the compiler detects a problem that makes it unable
 to compile the program, either because the program is invalid or the
 programmer has decided to make a specific `warning` into an error.

A `warning` is emitted when the compiler detects something odd about a
program. For instance, dead code and unused `Result` values.

A `help` message is emitted following an `error` or `warning` to give additional
information to the user about how to solve their problem.

A `note` is emitted to identify additional circumstances and parts of the code
that caused the warning or error. For example, the borrow checker will note any
previous conflicting borrows.

* Write in plain simple English. If your message, when shown on a – possibly
small – screen (which hasn't been cleaned for a while), cannot be understood
by a normal programmer, who just came out of bed after a night partying, it's
too complex.
* `Errors` and `Warnings` should not suggest how to fix the problem. A `Help`
message should be emitted instead.
* `Error`, `Warning`, `Note`, and `Help` messages start with a lowercase
letter and do not end with punctuation.
* Error messages should be succinct. Users will see these error messages many
times, and more verbose descriptions can be viewed with the `--explain` flag.
That said, don't make it so terse that it's hard to understand.
* The word "illegal" is illegal. Prefer "invalid" or a more specific word
instead.
* Errors should document the span of code where they occur – the `span_..`
methods allow to easily do this. Also `note` other spans that have contributed
to the error if the span isn't too large.
* When emitting a message with span, try to reduce the span to the smallest
amount possible that still signifies the issue
* Try not to emit multiple error messages for the same error. This may require
detecting duplicates.
* When the compiler has too little information for a specific error message,
lobby for annotations for library code that allow adding more. For example see
`#[on_unimplemented]`. Use these annotations when available!
* Keep in mind that Rust's learning curve is rather steep, and that the
compiler messages are an important learning tool.

## Error Explanations

Error explanations are long form descriptions of error messages provided with
the compiler. They are accessible via the `--explain` flag. Each explanation
comes with an example of how to trigger it and advice on how to fix it.

Please read [RFC 1567](https://github.com/rust-lang/rfcs/blob/master/text/1567-long-error-codes-explanation-normalization.md)
for details on how to format and write long error codes.

* All of them are accessible [online](http://doc.rust-lang.org/error-index.html),
  which are auto-generated from rustc source code in different places:
  [librustc](https://github.com/rust-lang/rust/blob/master/src/librustc/error_codes.rs),
  [libsyntax](https://github.com/rust-lang/rust/blob/master/src/libsyntax/error_codes.rs),
  [librustc_borrowck](https://github.com/rust-lang/rust/blob/master/src/librustc_borrowck/error_codes.rs),
  [librustc_metadata](https://github.com/rust-lang/rust/blob/master/src/librustc_metadata/error_codes.rs),
  [librustc_mir](https://github.com/rust-lang/rust/blob/master/src/librustc_mir/error_codes.rs),
  [librustc_passes](https://github.com/rust-lang/rust/blob/master/src/librustc_passes/error_codes.rs),
  [librustc_privacy](https://github.com/rust-lang/rust/blob/master/src/librustc_privacy/error_codes.rs),
  [librustc_resolve](https://github.com/rust-lang/rust/blob/master/src/librustc_resolve/error_codes.rs),
  [librustc_codegen_llvm](https://github.com/rust-lang/rust/blob/master/src/librustc_codegen_llvm/error_codes.rs),
  [librustc_plugin_impl](https://github.com/rust-lang/rust/blob/master/src/librustc_plugin/error_codes.rs),
  [librustc_typeck](https://github.com/rust-lang/rust/blob/master/src/librustc_typeck/error_codes.rs).
* Explanations have full markdown support. Use it, especially to highlight
code with backticks.
* When talking about the compiler, call it `the compiler`, not `Rust` or
`rustc`.

## Compiler Flags

* Flags should be orthogonal to each other. For example, if we'd have a
json-emitting variant of multiple actions `foo` and `bar`, an additional
--json flag is better than adding `--foo-json` and `--bar-json`.
* Always give options a long descriptive name, if only for more
understandable compiler scripts.
* The `--verbose` flag is for adding verbose information to `rustc` output
when not compiling a program. For example, using it with the `--version` flag
gives information about the hashes of the code.
* Experimental flags and options must be guarded behind the `-Z unstable-options` flag.
