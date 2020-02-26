// run-rustfix

#[warn(clippy::bool_comparison)]
fn main() {
    let x = true;
    if x == true {
        "yes"
    } else {
        "no"
    };
    if x == false {
        "yes"
    } else {
        "no"
    };
    if true == x {
        "yes"
    } else {
        "no"
    };
    if false == x {
        "yes"
    } else {
        "no"
    };
    if x != true {
        "yes"
    } else {
        "no"
    };
    if x != false {
        "yes"
    } else {
        "no"
    };
    if true != x {
        "yes"
    } else {
        "no"
    };
    if false != x {
        "yes"
    } else {
        "no"
    };
    if x < true {
        "yes"
    } else {
        "no"
    };
    if false < x {
        "yes"
    } else {
        "no"
    };
    if x > false {
        "yes"
    } else {
        "no"
    };
    if true > x {
        "yes"
    } else {
        "no"
    };
    let y = true;
    if x < y {
        "yes"
    } else {
        "no"
    };
    if x > y {
        "yes"
    } else {
        "no"
    };
}

#[allow(dead_code)]
fn issue3703() {
    struct Foo;
    impl PartialEq<bool> for Foo {
        fn eq(&self, _: &bool) -> bool {
            true
        }
    }
    impl PartialEq<Foo> for bool {
        fn eq(&self, _: &Foo) -> bool {
            true
        }
    }
    impl PartialOrd<bool> for Foo {
        fn partial_cmp(&self, _: &bool) -> Option<std::cmp::Ordering> {
            None
        }
    }
    impl PartialOrd<Foo> for bool {
        fn partial_cmp(&self, _: &Foo) -> Option<std::cmp::Ordering> {
            None
        }
    }

    if Foo == true {}
    if true == Foo {}
    if Foo != true {}
    if true != Foo {}
    if Foo == false {}
    if false == Foo {}
    if Foo != false {}
    if false != Foo {}
    if Foo < false {}
    if false < Foo {}
}
