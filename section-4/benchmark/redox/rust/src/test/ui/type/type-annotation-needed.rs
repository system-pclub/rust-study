fn foo<T: Into<String>>(x: i32) {}
//~^ NOTE required by
//~| NOTE

fn main() {
    foo(42);
    //~^ ERROR type annotations needed
}
