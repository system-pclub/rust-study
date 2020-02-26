// run-rustfix
#![allow(clippy::cognitive_complexity, clippy::assertions_on_constants)]

#[rustfmt::skip]
#[warn(clippy::collapsible_if)]
fn main() {
    let x = "hello";
    let y = "world";
    if x == "hello" {
        if y == "world" {
            println!("Hello world!");
        }
    }

    if x == "hello" || x == "world" {
        if y == "world" || y == "hello" {
            println!("Hello world!");
        }
    }

    if x == "hello" && x == "world" {
        if y == "world" || y == "hello" {
            println!("Hello world!");
        }
    }

    if x == "hello" || x == "world" {
        if y == "world" && y == "hello" {
            println!("Hello world!");
        }
    }

    if x == "hello" && x == "world" {
        if y == "world" && y == "hello" {
            println!("Hello world!");
        }
    }

    if 42 == 1337 {
        if 'a' != 'A' {
            println!("world!")
        }
    }

    // Collapse `else { if .. }` to `else if ..`
    if x == "hello" {
        print!("Hello ");
    } else {
        if y == "world" {
            println!("world!")
        }
    }

    if x == "hello" {
        print!("Hello ");
    } else {
        if let Some(42) = Some(42) {
            println!("world!")
        }
    }

    if x == "hello" {
        print!("Hello ");
    } else {
        if y == "world" {
            println!("world")
        }
        else {
            println!("!")
        }
    }

    if x == "hello" {
        print!("Hello ");
    } else {
        if let Some(42) = Some(42) {
            println!("world")
        }
        else {
            println!("!")
        }
    }

    if let Some(42) = Some(42) {
        print!("Hello ");
    } else {
        if let Some(42) = Some(42) {
            println!("world")
        }
        else {
            println!("!")
        }
    }

    if let Some(42) = Some(42) {
        print!("Hello ");
    } else {
        if x == "hello" {
            println!("world")
        }
        else {
            println!("!")
        }
    }

    if let Some(42) = Some(42) {
        print!("Hello ");
    } else {
        if let Some(42) = Some(42) {
            println!("world")
        }
        else {
            println!("!")
        }
    }

    // Works because any if with an else statement cannot be collapsed.
    if x == "hello" {
        if y == "world" {
            println!("Hello world!");
        }
    } else {
        println!("Not Hello world");
    }

    if x == "hello" {
        if y == "world" {
            println!("Hello world!");
        } else {
            println!("Hello something else");
        }
    }

    if x == "hello" {
        print!("Hello ");
        if y == "world" {
            println!("world!")
        }
    }

    if true {
    } else {
        assert!(true); // assert! is just an `if`
    }


    // The following tests check for the fix of https://github.com/rust-lang/rust-clippy/issues/798
    if x == "hello" {// Not collapsible
        if y == "world" {
            println!("Hello world!");
        }
    }

    if x == "hello" { // Not collapsible
        if y == "world" {
            println!("Hello world!");
        }
    }

    if x == "hello" {
        // Not collapsible
        if y == "world" {
            println!("Hello world!");
        }
    }

    if x == "hello" {
        if y == "world" { // Collapsible
            println!("Hello world!");
        }
    }

    if x == "hello" {
        print!("Hello ");
    } else {
        // Not collapsible
        if y == "world" {
            println!("world!")
        }
    }

    if x == "hello" {
        print!("Hello ");
    } else {
        // Not collapsible
        if let Some(42) = Some(42) {
            println!("world!")
        }
    }

    if x == "hello" {
        /* Not collapsible */
        if y == "world" {
            println!("Hello world!");
        }
    }

    if x == "hello" { /* Not collapsible */
        if y == "world" {
            println!("Hello world!");
        }
    }

    // Test behavior wrt. `let_chains`.
    // None of the cases below should be collapsed.
    fn truth() -> bool { true }

    // Prefix:
    if let 0 = 1 {
        if truth() {}
    }

    // Suffix:
    if truth() {
        if let 0 = 1 {}
    }

    // Midfix:
    if truth() {
        if let 0 = 1 {
            if truth() {}
        }
    }
}
