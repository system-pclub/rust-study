// Copyright 2018 PingCAP, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// See the License for the specific language governing permissions and
// limitations under the License.

#![cfg_attr(test, feature(test))]

#[macro_use]
extern crate bitflags;
#[macro_use]
extern crate enum_primitive_derive;
#[macro_use]
extern crate failure;
#[allow(unused_extern_crates)]
extern crate tikv_alloc;

mod def;
mod error;

pub mod prelude {
    pub use super::def::FieldTypeAccessor;
}

pub use self::def::*;
pub use self::error::*;
