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

mod chunk;
mod column;

pub use crate::coprocessor::codec::{Error, Result};

pub use self::chunk::{Chunk, ChunkEncoder};

#[cfg(test)]
mod tests {
    use cop_datatype::FieldTypeAccessor;
    use cop_datatype::FieldTypeTp;
    use tipb::expression::FieldType;

    pub fn field_type(tp: FieldTypeTp) -> FieldType {
        let mut fp = FieldType::new();
        fp.as_mut_accessor().set_tp(tp);
        fp
    }
}
