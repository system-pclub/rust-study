// run-pass
#![allow(dead_code)]
// pretty-expanded FIXME #23616

#![feature(optin_builtin_traits)]

use std::marker::Send;

struct TestType;

impl !Send for TestType {}

fn main() {}
