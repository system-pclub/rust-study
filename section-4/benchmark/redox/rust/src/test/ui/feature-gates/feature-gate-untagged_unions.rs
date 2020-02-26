union U1 { // OK
    a: u8,
}

union U2<T: Copy> { // OK
    a: T,
}

union U3 { //~ ERROR unions with non-`Copy` fields are unstable
    a: String, //~ ERROR unions may not contain fields that need dropping
}

union U4<T> { //~ ERROR unions with non-`Copy` fields are unstable
    a: T, //~ ERROR unions may not contain fields that need dropping
}

union U5 { //~ ERROR unions with `Drop` implementations are unstable
    a: u8,
}

impl Drop for U5 {
    fn drop(&mut self) {}
}

fn main() {}
