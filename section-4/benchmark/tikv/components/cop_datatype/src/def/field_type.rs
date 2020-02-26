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

use std::fmt;

use tipb::expression::FieldType;
use tipb::schema::ColumnInfo;

use num_traits::FromPrimitive;

/// Valid values of `tipb::expression::FieldType::tp` and `tipb::schema::ColumnInfo::tp`.
///
/// `FieldType` is the field type of a column defined by schema.
///
/// `ColumnInfo` describes a column. It contains `FieldType` and some other column specific
/// information. However for historical reasons, fields in `FieldType` (for example, `tp`)
/// are flattened into `ColumnInfo`. Semantically these fields are identical.
///
/// Please refer to `mysql/type.go` in TiDB.
#[derive(Primitive, PartialEq, Debug, Clone, Copy)]
pub enum FieldTypeTp {
    Unspecified = 0, // Default
    Tiny = 1,
    Short = 2,
    Long = 3,
    Float = 4,
    Double = 5,
    Null = 6,
    Timestamp = 7,
    LongLong = 8,
    Int24 = 9,
    Date = 10,
    Duration = 11,
    DateTime = 12,
    Year = 13,
    NewDate = 14,
    VarChar = 15,
    Bit = 16,
    JSON = 0xf5,
    NewDecimal = 0xf6,
    Enum = 0xf7,
    Set = 0xf8,
    TinyBlob = 0xf9,
    MediumBlob = 0xfa,
    LongBlob = 0xfb,
    Blob = 0xfc,
    VarString = 0xfd,
    String = 0xfe,
    Geometry = 0xff,
}

impl fmt::Display for FieldTypeTp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

/// Valid values of `tipb::expression::FieldType::collate` and
/// `tipb::schema::ColumnInfo::collation`.
///
/// The default value if `UTF8Bin`.
#[derive(Primitive, PartialEq, Debug, Clone, Copy)]
pub enum Collation {
    Binary = 63,
    UTF8Bin = 83, // Default
}

impl fmt::Display for Collation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

bitflags! {
    pub struct FieldTypeFlag: u32 {
        /// Field can't be NULL.
        const NOT_NULL = 1;

        /// Field is unsigned.
        const UNSIGNED = 1 << 5;

        /// Field is binary.
        const BINARY = 1 << 7;

        /// Internal: Used when we want to parse string to JSON in CAST.
        const PARSE_TO_JSON = 1 << 18;

        /// Internal: Used for telling boolean literal from integer.
        const IS_BOOLEAN = 1 << 19;
    }
}

/// A uniform `FieldType` access interface for `FieldType` and `ColumnInfo`.
pub trait FieldTypeAccessor {
    fn tp(&self) -> FieldTypeTp;

    fn set_tp(&mut self, tp: FieldTypeTp) -> &mut dyn FieldTypeAccessor;

    fn flag(&self) -> FieldTypeFlag;

    fn set_flag(&mut self, flag: FieldTypeFlag) -> &mut dyn FieldTypeAccessor;

    fn flen(&self) -> isize;

    fn set_flen(&mut self, flen: isize) -> &mut dyn FieldTypeAccessor;

    fn decimal(&self) -> isize;

    fn set_decimal(&mut self, decimal: isize) -> &mut dyn FieldTypeAccessor;

    fn collation(&self) -> Collation;

    fn set_collation(&mut self, collation: Collation) -> &mut dyn FieldTypeAccessor;

    /// Convert reference to `FieldTypeAccessor` interface.
    fn as_accessor(&self) -> &dyn FieldTypeAccessor
    where
        Self: Sized,
    {
        self as &dyn FieldTypeAccessor
    }

    /// Convert mutable reference to `FieldTypeAccessor` interface.
    fn as_mut_accessor(&mut self) -> &mut dyn FieldTypeAccessor
    where
        Self: Sized,
    {
        self as &mut dyn FieldTypeAccessor
    }

    /// Whether this type is a hybrid type, which can represent different types of value in
    /// specific context.
    ///
    /// Please refer to `Hybrid` in TiDB.
    #[inline]
    fn is_hybrid(&self) -> bool {
        let tp = self.tp();
        tp == FieldTypeTp::Enum || tp == FieldTypeTp::Bit || tp == FieldTypeTp::Set
    }

    /// Whether this type is a blob type.
    ///
    /// Please refer to `IsTypeBlob` in TiDB.
    #[inline]
    fn is_blob_like(&self) -> bool {
        let tp = self.tp();
        tp == FieldTypeTp::TinyBlob
            || tp == FieldTypeTp::MediumBlob
            || tp == FieldTypeTp::Blob
            || tp == FieldTypeTp::LongBlob
    }

    /// Whether this type is a char-like type like a string type or a varchar type.
    ///
    /// Please refer to `IsTypeChar` in TiDB.
    #[inline]
    fn is_char_like(&self) -> bool {
        let tp = self.tp();
        tp == FieldTypeTp::String || tp == FieldTypeTp::VarChar
    }

    /// Whether this type is a varchar-like type like a varstring type or a varchar type.
    ///
    /// Please refer to `IsTypeVarchar` in TiDB.
    #[inline]
    fn is_varchar_like(&self) -> bool {
        let tp = self.tp();
        tp == FieldTypeTp::VarString || tp == FieldTypeTp::VarChar
    }

    /// Whether this type is a string-like type.
    ///
    /// Please refer to `IsString` in TiDB.
    #[inline]
    fn is_string_like(&self) -> bool {
        self.is_blob_like()
            || self.is_char_like()
            || self.is_varchar_like()
            || self.tp() == FieldTypeTp::Unspecified
    }

    /// Whether this type is a binary-string-like type.
    ///
    /// Please refer to `IsBinaryStr` in TiDB.
    #[inline]
    fn is_binary_string_like(&self) -> bool {
        self.collation() == Collation::Binary && self.is_string_like()
    }

    /// Whether this type is a non-binary-string-like type.
    ///
    /// Please refer to `IsNonBinaryStr` in TiDB.
    #[inline]
    fn is_non_binary_string_like(&self) -> bool {
        self.collation() != Collation::Binary && self.is_string_like()
    }
}

impl FieldTypeAccessor for FieldType {
    #[inline]
    fn tp(&self) -> FieldTypeTp {
        FieldTypeTp::from_i32(self.get_tp()).unwrap_or(FieldTypeTp::Unspecified)
    }

    #[inline]
    fn set_tp(&mut self, tp: FieldTypeTp) -> &mut dyn FieldTypeAccessor {
        FieldType::set_tp(self, tp as i32);
        self as &mut dyn FieldTypeAccessor
    }

    #[inline]
    fn flag(&self) -> FieldTypeFlag {
        FieldTypeFlag::from_bits_truncate(self.get_flag())
    }

    #[inline]
    fn set_flag(&mut self, flag: FieldTypeFlag) -> &mut dyn FieldTypeAccessor {
        FieldType::set_flag(self, flag.bits());
        self as &mut dyn FieldTypeAccessor
    }

    #[inline]
    fn flen(&self) -> isize {
        self.get_flen() as isize
    }

    #[inline]
    fn set_flen(&mut self, flen: isize) -> &mut dyn FieldTypeAccessor {
        FieldType::set_flen(self, flen as i32);
        self as &mut dyn FieldTypeAccessor
    }

    #[inline]
    fn decimal(&self) -> isize {
        self.get_decimal() as isize
    }

    #[inline]
    fn set_decimal(&mut self, decimal: isize) -> &mut dyn FieldTypeAccessor {
        FieldType::set_decimal(self, decimal as i32);
        self as &mut dyn FieldTypeAccessor
    }

    #[inline]
    fn collation(&self) -> Collation {
        Collation::from_i32(self.get_collate()).unwrap_or(Collation::UTF8Bin)
    }

    #[inline]
    fn set_collation(&mut self, collation: Collation) -> &mut dyn FieldTypeAccessor {
        FieldType::set_collate(self, collation as i32);
        self as &mut dyn FieldTypeAccessor
    }
}

impl FieldTypeAccessor for ColumnInfo {
    #[inline]
    fn tp(&self) -> FieldTypeTp {
        FieldTypeTp::from_i32(self.get_tp()).unwrap_or(FieldTypeTp::Unspecified)
    }

    #[inline]
    fn set_tp(&mut self, tp: FieldTypeTp) -> &mut dyn FieldTypeAccessor {
        ColumnInfo::set_tp(self, tp as i32);
        self as &mut dyn FieldTypeAccessor
    }

    #[inline]
    fn flag(&self) -> FieldTypeFlag {
        FieldTypeFlag::from_bits_truncate(self.get_flag() as u32)
    }

    #[inline]
    fn set_flag(&mut self, flag: FieldTypeFlag) -> &mut dyn FieldTypeAccessor {
        ColumnInfo::set_flag(self, flag.bits() as i32);
        self as &mut dyn FieldTypeAccessor
    }

    #[inline]
    fn flen(&self) -> isize {
        self.get_columnLen() as isize
    }

    #[inline]
    fn set_flen(&mut self, flen: isize) -> &mut dyn FieldTypeAccessor {
        ColumnInfo::set_columnLen(self, flen as i32);
        self as &mut dyn FieldTypeAccessor
    }

    #[inline]
    fn decimal(&self) -> isize {
        self.get_decimal() as isize
    }

    #[inline]
    fn set_decimal(&mut self, decimal: isize) -> &mut dyn FieldTypeAccessor {
        ColumnInfo::set_decimal(self, decimal as i32);
        self as &mut dyn FieldTypeAccessor
    }

    #[inline]
    fn collation(&self) -> Collation {
        Collation::from_i32(self.get_collation()).unwrap_or(Collation::UTF8Bin)
    }

    #[inline]
    fn set_collation(&mut self, collation: Collation) -> &mut dyn FieldTypeAccessor {
        ColumnInfo::set_collation(self, collation as i32);
        self as &mut dyn FieldTypeAccessor
    }
}
