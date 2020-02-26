// run-rustfix
// rustfix-only-machine-applicable

use std::ffi::OsString;
use std::path::Path;

fn main() {
    let _s = ["lorem", "ipsum"].join(" ").to_string();

    let s = String::from("foo");
    let _s = s.clone();

    let s = String::from("foo");
    let _s = s.to_string();

    let s = String::from("foo");
    let _s = s.to_owned();

    let _s = Path::new("/a/b/").join("c").to_owned();

    let _s = Path::new("/a/b/").join("c").to_path_buf();

    let _s = OsString::new().to_owned();

    let _s = OsString::new().to_os_string();

    // Check that lint level works
    #[allow(clippy::redundant_clone)]
    let _s = String::new().to_string();

    let tup = (String::from("foo"),);
    let _t = tup.0.clone();

    let tup_ref = &(String::from("foo"),);
    let _s = tup_ref.0.clone(); // this `.clone()` cannot be removed

    {
        let x = String::new();
        let y = &x;

        let _x = x.clone(); // ok; `x` is borrowed by `y`

        let _ = y.len();
    }

    let x = (String::new(),);
    let _ = Some(String::new()).unwrap_or_else(|| x.0.clone()); // ok; closure borrows `x`

    with_branch(Alpha, true);
    cannot_double_move(Alpha);
    cannot_move_from_type_with_drop();
    borrower_propagation();
}

#[derive(Clone)]
struct Alpha;
fn with_branch(a: Alpha, b: bool) -> (Alpha, Alpha) {
    if b {
        (a.clone(), a.clone())
    } else {
        (Alpha, a)
    }
}

fn cannot_double_move(a: Alpha) -> (Alpha, Alpha) {
    (a.clone(), a)
}

struct TypeWithDrop {
    x: String,
}

impl Drop for TypeWithDrop {
    fn drop(&mut self) {}
}

fn cannot_move_from_type_with_drop() -> String {
    let s = TypeWithDrop { x: String::new() };
    s.x.clone() // removing this `clone()` summons E0509
}

fn borrower_propagation() {
    let s = String::new();
    let t = String::new();

    {
        fn b() -> bool {
            unimplemented!()
        }
        let _u = if b() { &s } else { &t };

        // ok; `s` and `t` are possibly borrowed
        let _s = s.clone();
        let _t = t.clone();
    }

    {
        let _u = || s.len();
        let _v = [&t; 32];
        let _s = s.clone(); // ok
        let _t = t.clone(); // ok
    }

    {
        let _u = {
            let u = Some(&s);
            let _ = s.clone(); // ok
            u
        };
        let _s = s.clone(); // ok
    }

    {
        use std::convert::identity as id;
        let _u = id(id(&s));
        let _s = s.clone(); // ok, `u` borrows `s`
    }

    let _s = s.clone();
    let _t = t.clone();

    #[derive(Clone)]
    struct Foo {
        x: usize,
    }

    {
        let f = Foo { x: 123 };
        let _x = Some(f.x);
        let _f = f.clone();
    }

    {
        let f = Foo { x: 123 };
        let _x = &f.x;
        let _f = f.clone(); // ok
    }
}
