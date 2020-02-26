// run-rustfix

#![deny(clippy::mem_discriminant_non_enum)]

use std::mem;

enum Foo {
    One(usize),
    Two(u8),
}

fn main() {
    // bad
    mem::discriminant(&&Some(2));
    mem::discriminant(&&None::<u8>);
    mem::discriminant(&&Foo::One(5));
    mem::discriminant(&&Foo::Two(5));

    let ro = &Some(3);
    let rro = &ro;
    mem::discriminant(&ro);
    mem::discriminant(rro);
    mem::discriminant(&rro);

    macro_rules! mem_discriminant_but_in_a_macro {
        ($param:expr) => {
            mem::discriminant($param)
        };
    }

    mem_discriminant_but_in_a_macro!(&rro);

    let rrrrro = &&&rro;
    mem::discriminant(&rrrrro);
    mem::discriminant(*rrrrro);

    // ok
    mem::discriminant(&Some(2));
    mem::discriminant(&None::<u8>);
    mem::discriminant(&Foo::One(5));
    mem::discriminant(&Foo::Two(5));
    mem::discriminant(ro);
    mem::discriminant(*rro);
    mem::discriminant(****rrrrro);
}
