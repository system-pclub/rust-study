// ignore-x86 FIXME: missing sysroot spans (#53081)
// This file was auto-generated using 'src/etc/generate-deriving-span-tests.py'

#[derive(Eq,PartialOrd,PartialEq)]
struct Error;

#[derive(Ord,Eq,PartialOrd,PartialEq)]
struct Struct(
    Error //~ ERROR
);

fn main() {}
