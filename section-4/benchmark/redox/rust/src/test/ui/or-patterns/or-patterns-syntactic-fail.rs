// Test some cases where or-patterns may ostensibly be allowed but are in fact not.
// This is not a semantic test. We only test parsing.

#![feature(or_patterns)]
//~^ WARN the feature `or_patterns` is incomplete and may cause the compiler to crash

fn main() {}

// Test the `pat` macro fragment parser:
macro_rules! accept_pat {
    ($p:pat) => {}
}

accept_pat!(p | q); //~ ERROR no rules expected the token `|`
accept_pat!(| p | q); //~ ERROR no rules expected the token `|`

// Non-macro tests:

enum E { A, B }
use E::*;

fn no_top_level_or_patterns() {
    // We do *not* allow or-patterns at the top level of lambdas...
    let _ = |A | B: E| (); //~ ERROR binary operation `|` cannot be applied to type `E`
    //           -------- This looks like an or-pattern but is in fact `|A| (B: E | ())`.

    // ...and for now neither do we allow or-patterns at the top level of functions.
    fn fun1(A | B: E) {} //~ ERROR an or-pattern parameter must be wrapped in parenthesis

    fn fun2(| A | B: E) {}
    //~^ ERROR a leading `|` is not allowed in a parameter pattern
    //~| ERROR an or-pattern parameter must be wrapped in parenthesis
}

// We also do not allow a leading `|` when not in a top level position:

fn no_leading_inner() {
    struct TS(E);
    struct NS { f: E }

    let ( | A | B) = E::A; //~ ERROR a leading `|` is only allowed in a top-level pattern
    let ( | A | B,) = (E::B,); //~ ERROR a leading `|` is only allowed in a top-level pattern
    let [ | A | B ] = [E::A]; //~ ERROR a leading `|` is only allowed in a top-level pattern
    let TS( | A | B ); //~ ERROR a leading `|` is only allowed in a top-level pattern
    let NS { f: | A | B }; //~ ERROR a leading `|` is only allowed in a top-level pattern

    let ( || A | B) = E::A; //~ ERROR a leading `|` is only allowed in a top-level pattern
    let [ || A | B ] = [E::A]; //~ ERROR a leading `|` is only allowed in a top-level pattern
    let TS( || A | B ); //~ ERROR a leading `|` is only allowed in a top-level pattern
    let NS { f: || A | B }; //~ ERROR a leading `|` is only allowed in a top-level pattern

    let recovery_witness: String = 0; //~ ERROR mismatched types
}
