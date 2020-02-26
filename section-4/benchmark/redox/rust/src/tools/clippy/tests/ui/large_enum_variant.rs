#![allow(dead_code)]
#![allow(unused_variables)]
#![warn(clippy::large_enum_variant)]

enum LargeEnum {
    A(i32),
    B([i32; 8000]),
}

enum GenericEnumOk<T> {
    A(i32),
    B([T; 8000]),
}

enum GenericEnum2<T> {
    A(i32),
    B([i32; 8000]),
    C(T, [i32; 8000]),
}

trait SomeTrait {
    type Item;
}

enum LargeEnumGeneric<A: SomeTrait> {
    Var(A::Item),
}

enum LargeEnum2 {
    VariantOk(i32, u32),
    ContainingLargeEnum(LargeEnum),
}
enum LargeEnum3 {
    ContainingMoreThanOneField(i32, [i32; 8000], [i32; 9500]),
    VoidVariant,
    StructLikeLittle { x: i32, y: i32 },
}

enum LargeEnum4 {
    VariantOk(i32, u32),
    StructLikeLarge { x: [i32; 8000], y: i32 },
}

enum LargeEnum5 {
    VariantOk(i32, u32),
    StructLikeLarge2 { x: [i32; 8000] },
}

enum LargeEnumOk {
    LargeA([i32; 8000]),
    LargeB([i32; 8001]),
}

fn main() {}
