#![feature(or_patterns)]
//~^ WARN the feature `or_patterns` is incomplete and may cause the compiler to crash

fn main() {
    let x = 3;

    match x {
        1 | 2 || 3 => (), //~ ERROR unexpected token `||` after pattern
        _ => (),
    }

    match x {
        (1 | 2 || 3) => (), //~ ERROR unexpected token `||` after pattern
        _ => (),
    }

    match (x,) {
        (1 | 2 || 3,) => (), //~ ERROR unexpected token `||` after pattern
        _ => (),
    }

    struct TS(u8);

    match TS(x) {
        TS(1 | 2 || 3) => (), //~ ERROR unexpected token `||` after pattern
        _ => (),
    }

    struct NS { f: u8 }

    match (NS { f: x }) {
        NS { f: 1 | 2 || 3 } => (), //~ ERROR unexpected token `||` after pattern
        _ => (),
    }

    match [x] {
        [1 | 2 || 3] => (), //~ ERROR unexpected token `||` after pattern
        _ => (),
    }

    match x {
        || 1 | 2 | 3 => (), //~ ERROR unexpected token `||` after pattern
        _ => (),
    }
}
