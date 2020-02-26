#![warn(clippy::items_after_statements)]

fn ok() {
    fn foo() {
        println!("foo");
    }
    foo();
}

fn last() {
    foo();
    fn foo() {
        println!("foo");
    }
}

fn main() {
    foo();
    fn foo() {
        println!("foo");
    }
    foo();
}

fn mac() {
    let mut a = 5;
    println!("{}", a);
    // do not lint this, because it needs to be after `a`
    macro_rules! b {
        () => {{
            a = 6
        }};
    }
    b!();
    println!("{}", a);
}
