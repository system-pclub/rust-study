enum Whatever {
}

fn foo(x: Whatever) {
    match x {
        Some(field) =>
//~^ ERROR mismatched types
//~| expected enum `Whatever`, found enum `std::option::Option`
//~| expected enum `Whatever`
//~| found enum `std::option::Option<_>`
            field.access(),
    }
}

fn main(){}
