/**
 * This issue need to be reproduced in rust-1.12.0
 */

use std::mem;

#[repr(C)]
pub enum CXTypeKind {
    Invalid = 0,
    UShort = 1,
    UInt = 2,
    ULong = 3,
    ULongLong = 4,
    Short = 5,
    Int = 6,
    Long = 7,
    LongLong = 8,
    Float = 9,
}


fn get_bad_kind() -> CXTypeKind {
    unsafe { mem::transmute(119) }
}

fn test() -> i32 {
    match get_bad_kind() {
        CXTypeKind::UShort => 0,
        CXTypeKind::UInt => 1,
        CXTypeKind::ULong => 2,
        CXTypeKind::ULongLong => 3,
        CXTypeKind::Short => 4,
        CXTypeKind::Int => 5,
        CXTypeKind::Long => 6,
        CXTypeKind::LongLong => 7,
        CXTypeKind::Float => 8,
        _ => {
            -1
        }
    }
}

fn main() {
    println!("{}", test());
}

