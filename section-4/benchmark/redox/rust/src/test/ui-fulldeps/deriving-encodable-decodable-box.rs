// run-pass

#![allow(unused_imports)]

#![feature(box_syntax)]
#![feature(rustc_private)]

extern crate serialize as rustc_serialize;

use rustc_serialize::{Encodable, Decodable};
use rustc_serialize::json;

#[derive(RustcEncodable, RustcDecodable)]
struct A {
    foo: Box<[bool]>,
}

fn main() {
    let obj = A { foo: Box::new([true, false]) };
    let s = json::encode(&obj).unwrap();
    let obj2: A = json::decode(&s).unwrap();
    assert_eq!(obj.foo, obj2.foo);
}
