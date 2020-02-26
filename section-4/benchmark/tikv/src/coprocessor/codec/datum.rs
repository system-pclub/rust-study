// Copyright 2016 PingCAP, Inc.
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

use byteorder::WriteBytesExt;
use std::borrow::Cow;
use std::cmp::Ordering;
use std::fmt::{self, Debug, Display, Formatter};
use std::io::Write;
use std::str::FromStr;
use std::{i64, str};

use cop_datatype::FieldTypeTp;

use super::mysql::{
    self, parse_json_path_expr, Decimal, DecimalEncoder, Duration, Json, JsonEncoder,
    PathExpression, RoundMode, Time, DEFAULT_FSP, MAX_FSP,
};
use super::{convert, Error, Result};
use crate::coprocessor::dag::expr::EvalContext;
use crate::util::codec::bytes::{self, BytesEncoder};
use crate::util::codec::{number, BytesSlice};
use crate::util::escape;

pub const NIL_FLAG: u8 = 0;
pub const BYTES_FLAG: u8 = 1;
pub const COMPACT_BYTES_FLAG: u8 = 2;
pub const INT_FLAG: u8 = 3;
pub const UINT_FLAG: u8 = 4;
pub const FLOAT_FLAG: u8 = 5;
pub const DECIMAL_FLAG: u8 = 6;
pub const DURATION_FLAG: u8 = 7;
pub const VAR_INT_FLAG: u8 = 8;
pub const VAR_UINT_FLAG: u8 = 9;
pub const JSON_FLAG: u8 = 10;
pub const MAX_FLAG: u8 = 250;

pub const DATUM_DATA_NULL: &[u8; 1] = &[NIL_FLAG];

/// `Datum` stores data with different types.
#[derive(PartialEq, Clone)]
pub enum Datum {
    Null,
    I64(i64),
    U64(u64),
    F64(f64),
    Dur(Duration),
    Bytes(Vec<u8>),
    Dec(Decimal),
    Time(Time),
    Json(Json),
    Min,
    Max,
}

impl Display for Datum {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match *self {
            Datum::Null => write!(f, "NULL"),
            Datum::I64(i) => write!(f, "I64({})", i),
            Datum::U64(u) => write!(f, "U64({})", u),
            Datum::F64(v) => write!(f, "F64({})", v),
            Datum::Dur(ref d) => write!(f, "Dur({})", d),
            Datum::Bytes(ref bs) => write!(f, "Bytes(\"{}\")", escape(bs)),
            Datum::Dec(ref d) => write!(f, "Dec({})", d),
            Datum::Time(ref t) => write!(f, "Time({})", t),
            Datum::Json(ref j) => write!(f, "Json({})", j.to_string()),
            Datum::Min => write!(f, "MIN"),
            Datum::Max => write!(f, "MAX"),
        }
    }
}

impl Debug for Datum {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}

/// `cmp_f64` compares the f64 values and returns the Ordering.
#[inline]
pub fn cmp_f64(l: f64, r: f64) -> Result<Ordering> {
    l.partial_cmp(&r)
        .ok_or_else(|| invalid_type!("{} and {} can't be compared", l, r))
}

/// `checked_add_i64`  checks and adds `r` to the `l`. Return None if the sum is negative.
#[inline]
fn checked_add_i64(l: u64, r: i64) -> Option<u64> {
    if r >= 0 {
        Some(l + r as u64)
    } else {
        l.checked_sub(r.overflowing_neg().0 as u64)
    }
}

impl Datum {
    /// `cmp` compares the datum and returns an Ordering.
    pub fn cmp(&self, ctx: &mut EvalContext, datum: &Datum) -> Result<Ordering> {
        if let Datum::Json(_) = *self {
            if let Datum::Json(_) = *datum {
            } else {
                // reverse compare when self is json while datum not.
                let order = datum.cmp(ctx, self)?;
                return Ok(order.reverse());
            }
        }

        match *datum {
            Datum::Null => match *self {
                Datum::Null => Ok(Ordering::Equal),
                _ => Ok(Ordering::Greater),
            },
            Datum::Min => match *self {
                Datum::Null => Ok(Ordering::Less),
                Datum::Min => Ok(Ordering::Equal),
                _ => Ok(Ordering::Greater),
            },
            Datum::Max => match *self {
                Datum::Max => Ok(Ordering::Equal),
                _ => Ok(Ordering::Less),
            },
            Datum::I64(i) => self.cmp_i64(ctx, i),
            Datum::U64(u) => self.cmp_u64(ctx, u),
            Datum::F64(f) => self.cmp_f64(ctx, f),
            Datum::Bytes(ref bs) => self.cmp_bytes(ctx, bs),
            Datum::Dur(ref d) => self.cmp_dur(ctx, d),
            Datum::Dec(ref d) => self.cmp_dec(ctx, d),
            Datum::Time(ref t) => self.cmp_time(ctx, t),
            Datum::Json(ref j) => self.cmp_json(j),
        }
    }

    fn cmp_i64(&self, ctx: &mut EvalContext, i: i64) -> Result<Ordering> {
        match *self {
            Datum::I64(ii) => Ok(ii.cmp(&i)),
            Datum::U64(u) => {
                if i < 0 || u > i64::MAX as u64 {
                    Ok(Ordering::Greater)
                } else {
                    Ok(u.cmp(&(i as u64)))
                }
            }
            _ => self.cmp_f64(ctx, i as f64),
        }
    }

    fn cmp_u64(&self, ctx: &mut EvalContext, u: u64) -> Result<Ordering> {
        match *self {
            Datum::I64(i) => {
                if i < 0 || u > i64::MAX as u64 {
                    Ok(Ordering::Less)
                } else {
                    Ok(i.cmp(&(u as i64)))
                }
            }
            Datum::U64(uu) => Ok(uu.cmp(&u)),
            _ => self.cmp_f64(ctx, u as f64),
        }
    }

    fn cmp_f64(&self, ctx: &mut EvalContext, f: f64) -> Result<Ordering> {
        match *self {
            Datum::Null | Datum::Min => Ok(Ordering::Less),
            Datum::Max => Ok(Ordering::Greater),
            Datum::I64(i) => cmp_f64(i as f64, f),
            Datum::U64(u) => cmp_f64(u as f64, f),
            Datum::F64(ff) => cmp_f64(ff, f),
            Datum::Bytes(ref bs) => {
                let ff = convert::bytes_to_f64(ctx, bs)?;
                cmp_f64(ff, f)
            }
            Datum::Dec(ref d) => {
                let ff = d.as_f64()?;
                cmp_f64(ff, f)
            }
            Datum::Dur(ref d) => {
                let ff = d.to_secs();
                cmp_f64(ff, f)
            }
            Datum::Time(ref t) => {
                let ff = t.to_f64()?;
                cmp_f64(ff, f)
            }
            Datum::Json(ref json) => Datum::F64(f).cmp_json(json),
        }
    }

    fn cmp_bytes(&self, ctx: &mut EvalContext, bs: &[u8]) -> Result<Ordering> {
        match *self {
            Datum::Null | Datum::Min => Ok(Ordering::Less),
            Datum::Max => Ok(Ordering::Greater),
            Datum::Bytes(ref bss) => Ok((bss as &[u8]).cmp(bs)),
            Datum::Dec(ref d) => {
                let s = str::from_utf8(bs)?;
                let d2 = s.parse()?;
                Ok(d.cmp(&d2))
            }
            Datum::Time(ref t) => {
                let s = str::from_utf8(bs)?;
                let t2 = Time::parse_datetime(s, DEFAULT_FSP, &ctx.cfg.tz)?;
                Ok(t.cmp(&t2))
            }
            Datum::Dur(ref d) => {
                let d2 = Duration::parse(bs, MAX_FSP)?;
                Ok(d.cmp(&d2))
            }
            _ => {
                let f = convert::bytes_to_f64(ctx, bs)?;
                self.cmp_f64(ctx, f)
            }
        }
    }

    fn cmp_dec(&self, ctx: &mut EvalContext, dec: &Decimal) -> Result<Ordering> {
        match *self {
            Datum::Dec(ref d) => Ok(d.cmp(dec)),
            Datum::Bytes(ref bs) => {
                let s = str::from_utf8(bs)?;
                let d = s.parse::<Decimal>()?;
                Ok(d.cmp(dec))
            }
            _ => {
                let f = dec.as_f64()?;
                self.cmp_f64(ctx, f)
            }
        }
    }

    fn cmp_dur(&self, ctx: &mut EvalContext, d: &Duration) -> Result<Ordering> {
        match *self {
            Datum::Dur(ref d2) => Ok(d2.cmp(d)),
            Datum::Bytes(ref bs) => {
                let d2 = Duration::parse(bs, MAX_FSP)?;
                Ok(d2.cmp(d))
            }
            _ => self.cmp_f64(ctx, d.to_secs()),
        }
    }

    fn cmp_time(&self, ctx: &mut EvalContext, time: &Time) -> Result<Ordering> {
        match *self {
            Datum::Bytes(ref bs) => {
                let s = str::from_utf8(bs)?;
                let t = Time::parse_datetime(s, DEFAULT_FSP, &ctx.cfg.tz)?;
                Ok(t.cmp(time))
            }
            Datum::Time(ref t) => Ok(t.cmp(time)),
            _ => {
                let f = time.to_f64()?;
                self.cmp_f64(ctx, f)
            }
        }
    }

    fn cmp_json(&self, json: &Json) -> Result<Ordering> {
        let order = match *self {
            Datum::Json(ref j) => j.cmp(json),
            Datum::I64(d) => Json::I64(d).cmp(json),
            Datum::U64(d) => Json::U64(d).cmp(json),
            Datum::F64(d) => Json::Double(d).cmp(json),
            Datum::Dec(ref d) => {
                let ff = d.as_f64()?;
                Json::Double(ff).cmp(json)
            }
            Datum::Bytes(ref d) => {
                let data = str::from_utf8(d)?;
                Json::String(String::from(data)).cmp(json)
            }
            _ => {
                let data = self.to_string().unwrap_or_default();
                Json::String(data).cmp(json)
            }
        };
        Ok(order)
    }

    /// `into_bool` converts self to a bool.
    /// source function name is `ToBool`.
    pub fn into_bool(self, ctx: &mut EvalContext) -> Result<Option<bool>> {
        let b = match self {
            Datum::I64(i) => Some(i != 0),
            Datum::U64(u) => Some(u != 0),
            Datum::F64(f) => Some(f.round() != 0f64),
            Datum::Bytes(ref bs) => Some(!bs.is_empty() && convert::bytes_to_int(ctx, bs)? != 0),
            Datum::Time(t) => Some(!t.is_zero()),
            Datum::Dur(d) => Some(!d.is_zero()),
            Datum::Dec(d) => Some(d.as_f64()?.round() != 0f64),
            Datum::Null => None,
            _ => return Err(invalid_type!("can't convert {:?} to bool", self)),
        };
        Ok(b)
    }

    /// `to_string` returns a string representation of the datum.
    pub fn to_string(&self) -> Result<String> {
        let s = match *self {
            Datum::I64(i) => format!("{}", i),
            Datum::U64(u) => format!("{}", u),
            Datum::F64(f) => format!("{}", f),
            Datum::Bytes(ref bs) => String::from_utf8(bs.to_vec())?,
            Datum::Time(ref t) => format!("{}", t),
            Datum::Dur(ref d) => format!("{}", d),
            Datum::Dec(ref d) => format!("{}", d),
            Datum::Json(ref d) => d.to_string(),
            ref d => return Err(invalid_type!("can't convert {:?} to string", d)),
        };
        Ok(s)
    }

    /// into_string convert self into a string.
    /// source function name is `ToString`.
    pub fn into_string(self) -> Result<String> {
        if let Datum::Bytes(bs) = self {
            let data = String::from_utf8(bs)?;
            Ok(data)
        } else {
            self.to_string()
        }
    }

    /// `into_f64` converts self into f64.
    /// source function name is `ToFloat64`.
    pub fn into_f64(self, ctx: &mut EvalContext) -> Result<f64> {
        match self {
            Datum::I64(i) => Ok(i as f64),
            Datum::U64(u) => Ok(u as f64),
            Datum::F64(f) => Ok(f),
            Datum::Bytes(bs) => convert::bytes_to_f64(ctx, &bs),
            Datum::Time(t) => {
                let d = t.to_decimal()?;
                d.as_f64()
            }
            Datum::Dur(d) => {
                let d = d.to_decimal()?;
                d.as_f64()
            }
            Datum::Dec(d) => d.as_f64(),
            Datum::Json(j) => j.cast_to_real(ctx),
            _ => Err(box_err!("failed to convert {} to f64", self)),
        }
    }

    /// `into_i64` converts self into i64.
    /// source function name is `ToInt64`.
    pub fn into_i64(self, ctx: &mut EvalContext) -> Result<i64> {
        let (lower_bound, upper_bound) = (i64::MIN, i64::MAX);
        let tp = FieldTypeTp::LongLong;
        match self {
            Datum::I64(i) => Ok(i),
            Datum::U64(u) => convert::convert_uint_to_int(u, upper_bound, tp),

            Datum::F64(f) => convert::convert_float_to_int(f, lower_bound, upper_bound, tp),
            Datum::Bytes(bs) => convert::bytes_to_int(ctx, &bs),
            Datum::Time(mut t) => {
                t.round_frac(mysql::DEFAULT_FSP)?;
                let d = t.to_decimal()?;
                d.as_i64().into()
            }
            Datum::Dur(d) => {
                let d = d.round_frac(mysql::DEFAULT_FSP)?.to_decimal()?;
                d.as_i64().into()
            }
            Datum::Dec(d) => {
                let res: Result<Decimal> = d.round(mysql::DEFAULT_FSP, RoundMode::HalfEven).into();
                if let Err(e) = res {
                    Err(e)
                } else {
                    res.unwrap().as_i64().into()
                }
            }
            Datum::Json(j) => Ok(j.cast_to_int()),
            _ => Err(box_err!("failed to convert {} to i64", self)),
        }
    }

    /// Keep compatible with TiDB's `GetFloat64` function.
    #[inline]
    pub fn f64(&self) -> f64 {
        let i = self.i64();
        f64::from_bits(i as u64)
    }

    /// Keep compatible with TiDB's `GetInt64` function.
    #[inline]
    pub fn i64(&self) -> i64 {
        match *self {
            Datum::I64(i) => i,
            Datum::U64(u) => u as i64,
            Datum::F64(f) => f.to_bits() as i64,
            Datum::Dur(ref d) => d.to_nanos(),
            Datum::Time(_)
            | Datum::Bytes(_)
            | Datum::Dec(_)
            | Datum::Json(_)
            | Datum::Max
            | Datum::Min
            | Datum::Null => 0,
        }
    }

    /// Keep compatible with TiDB's `GetUint64` function.
    #[inline]
    pub fn u64(&self) -> u64 {
        self.i64() as u64
    }

    /// into_arith converts datum to appropriate datum for arithmetic computing.
    /// Keep compatible with TiDB's `CoerceArithmetic` function.
    pub fn into_arith(self, ctx: &mut EvalContext) -> Result<Datum> {
        match self {
            // MySQL will convert string to float for arithmetic operation
            Datum::Bytes(bs) => convert::bytes_to_f64(ctx, &bs).map(From::from),
            Datum::Time(t) => {
                // if time has no precision, return int64
                let dec = t.to_decimal()?;
                if t.get_fsp() == 0 {
                    return Ok(Datum::I64(dec.as_i64().unwrap()));
                }
                Ok(Datum::Dec(dec))
            }
            Datum::Dur(d) => {
                let dec = d.to_decimal()?;
                if d.fsp() == 0 {
                    return Ok(Datum::I64(dec.as_i64().unwrap()));
                }
                Ok(Datum::Dec(dec))
            }
            a => Ok(a),
        }
    }

    /// Keep compatible with TiDB's `ToDecimal` function.
    pub fn into_dec(self) -> Result<Decimal> {
        match self {
            Datum::Time(t) => t.to_decimal().map_err(From::from),
            Datum::Dur(d) => d.to_decimal().map_err(From::from),
            d => match d.coerce_to_dec()? {
                Datum::Dec(d) => Ok(d),
                d => Err(box_err!("failed to conver {} to decimal", d)),
            },
        }
    }

    /// cast_as_json converts Datum::Bytes(bs) into Json::from_str(bs)
    /// and Datum::Null would be illegal. It would be used in cast,
    /// json_merge,json_extract,json_type
    /// mysql> SELECT CAST('null' AS JSON);
    /// +----------------------+
    /// | CAST('null' AS JSON) |
    /// +----------------------+
    /// | null                 |
    /// +----------------------+
    pub fn cast_as_json(self) -> Result<Json> {
        match self {
            Datum::Bytes(ref bs) => {
                let s = box_try!(str::from_utf8(bs));
                let json: Json = s.parse()?;
                Ok(json)
            }
            Datum::I64(d) => Ok(Json::I64(d)),
            Datum::U64(d) => Ok(Json::U64(d)),
            Datum::F64(d) => Ok(Json::Double(d)),
            Datum::Dec(d) => {
                let ff = d.as_f64()?;
                Ok(Json::Double(ff))
            }
            Datum::Json(d) => Ok(d),
            _ => {
                let s = self.into_string()?;
                Ok(Json::String(s))
            }
        }
    }

    /// into_json would convert Datum::Bytes(bs) into Json::String(bs)
    /// and convert Datum::Null into Json::None.
    /// This func would be used in json_unquote and json_modify
    pub fn into_json(self) -> Result<Json> {
        match self {
            Datum::Null => Ok(Json::None),
            Datum::Bytes(bs) => {
                let s = String::from_utf8(bs)?;
                Ok(Json::String(s))
            }
            _ => self.cast_as_json(),
        }
    }

    /// `to_json_path_expr` parses Datum::Bytes(b) to a JSON PathExpression.
    pub fn to_json_path_expr(&self) -> Result<PathExpression> {
        let v = match *self {
            Datum::Bytes(ref bs) => str::from_utf8(bs)?,
            _ => "",
        };
        parse_json_path_expr(v)
    }

    /// Try its best effort to convert into a decimal datum.
    /// source function name is `ConvertDatumToDecimal`.
    fn coerce_to_dec(self) -> Result<Datum> {
        let dec = match self {
            Datum::I64(i) => i.into(),
            Datum::U64(u) => u.into(),
            Datum::F64(f) => Decimal::from_f64(f)?,
            Datum::Bytes(ref bs) => {
                let s = box_try!(str::from_utf8(bs));
                Decimal::from_str(s)?
            }
            d @ Datum::Dec(_) => return Ok(d),
            _ => return Err(box_err!("failed to convert {} to decimal", self)),
        };
        Ok(Datum::Dec(dec))
    }

    /// Try its best effort to convert into a f64 datum.
    fn coerce_to_f64(self) -> Result<Datum> {
        match self {
            Datum::I64(i) => Ok(Datum::F64(i as f64)),
            Datum::U64(u) => Ok(Datum::F64(u as f64)),
            Datum::Dec(d) => {
                let f = d.as_f64()?;
                Ok(Datum::F64(f))
            }
            a => Ok(a),
        }
    }

    /// `coerce` changes type.
    /// If left or right is F64, changes the both to F64.
    /// Else if left or right is Decimal, changes the both to Decimal.
    /// Keep compatible with TiDB's `CoerceDatum` function.
    pub fn coerce(left: Datum, right: Datum) -> Result<(Datum, Datum)> {
        let res = match (left, right) {
            a @ (Datum::Dec(_), Datum::Dec(_)) | a @ (Datum::F64(_), Datum::F64(_)) => a,
            (l @ Datum::F64(_), r) => (l, r.coerce_to_f64()?),
            (l, r @ Datum::F64(_)) => (l.coerce_to_f64()?, r),
            (l @ Datum::Dec(_), r) => (l, r.coerce_to_dec()?),
            (l, r @ Datum::Dec(_)) => (l.coerce_to_dec()?, r),
            p => p,
        };
        Ok(res)
    }

    /// `checked_div` computes the result of `self / d`.
    pub fn checked_div(self, ctx: &mut EvalContext, d: Datum) -> Result<Datum> {
        match (self, d) {
            (Datum::F64(f), d) => {
                let f2 = d.into_f64(ctx)?;
                if f2 == 0f64 {
                    return Ok(Datum::Null);
                }
                Ok(Datum::F64(f / f2))
            }
            (a, b) => {
                let a = a.into_dec()?;
                let b = b.into_dec()?;
                match a / b {
                    None => Ok(Datum::Null),
                    Some(res) => {
                        let d: Result<Decimal> = res.into();
                        d.map(Datum::Dec)
                    }
                }
            }
        }
    }

    /// Keep compatible with TiDB's `ComputePlus` function.
    pub fn checked_add(self, _: &mut EvalContext, d: Datum) -> Result<Datum> {
        let res: Datum = match (&self, &d) {
            (&Datum::I64(l), &Datum::I64(r)) => l.checked_add(r).into(),
            (&Datum::I64(l), &Datum::U64(r)) | (&Datum::U64(r), &Datum::I64(l)) => {
                checked_add_i64(r, l).into()
            }
            (&Datum::U64(l), &Datum::U64(r)) => l.checked_add(r).into(),
            (&Datum::F64(l), &Datum::F64(r)) => {
                let res = l + r;
                if !res.is_finite() {
                    Datum::Null
                } else {
                    Datum::F64(res)
                }
            }
            (&Datum::Dec(ref l), &Datum::Dec(ref r)) => {
                let dec: Result<Decimal> = (l + r).into();
                return dec.map(Datum::Dec);
            }
            (l, r) => return Err(invalid_type!("{:?} and {:?} can't be add together.", l, r)),
        };
        if let Datum::Null = res {
            return Err(box_err!("{:?} + {:?} overflow", self, d));
        }
        Ok(res)
    }

    /// `checked_minus` computes the result of `self - d`.
    pub fn checked_minus(self, _: &mut EvalContext, d: Datum) -> Result<Datum> {
        let res = match (&self, &d) {
            (&Datum::I64(l), &Datum::I64(r)) => l.checked_sub(r).into(),
            (&Datum::I64(l), &Datum::U64(r)) => {
                if l < 0 {
                    Datum::Null
                } else {
                    (l as u64).checked_sub(r).into()
                }
            }
            (&Datum::U64(l), &Datum::I64(r)) => {
                if r < 0 {
                    l.checked_add(r.overflowing_neg().0 as u64).into()
                } else {
                    l.checked_sub(r as u64).into()
                }
            }
            (&Datum::U64(l), &Datum::U64(r)) => l.checked_sub(r).into(),
            (&Datum::F64(l), &Datum::F64(r)) => return Ok(Datum::F64(l - r)),
            (&Datum::Dec(ref l), &Datum::Dec(ref r)) => {
                let dec: Result<Decimal> = (l - r).into();
                return dec.map(Datum::Dec);
            }
            (l, r) => return Err(invalid_type!("{:?} can't minus {:?}", l, r)),
        };
        if let Datum::Null = res {
            return Err(box_err!("{:?} - {:?} overflow", self, d));
        }
        Ok(res)
    }

    // `checked_mul` computes the result of a * b.
    pub fn checked_mul(self, _: &mut EvalContext, d: Datum) -> Result<Datum> {
        let res = match (&self, &d) {
            (&Datum::I64(l), &Datum::I64(r)) => l.checked_mul(r).into(),
            (&Datum::I64(l), &Datum::U64(r)) | (&Datum::U64(r), &Datum::I64(l)) => {
                if l < 0 {
                    return Err(box_err!("{} * {} overflow.", l, r));
                }
                r.checked_mul(l as u64).into()
            }
            (&Datum::U64(l), &Datum::U64(r)) => l.checked_mul(r).into(),
            (&Datum::F64(l), &Datum::F64(r)) => return Ok(Datum::F64(l * r)),
            (&Datum::Dec(ref l), &Datum::Dec(ref r)) => return Ok(Datum::Dec((l * r).unwrap())),
            (l, r) => return Err(invalid_type!("{:?} can't multiply {:?}", l, r)),
        };

        if let Datum::Null = res {
            return Err(box_err!("{} * {} overflow", self, d));
        }
        Ok(res)
    }

    // `checked_rem` computes the result of a mod b.
    pub fn checked_rem(self, _: &mut EvalContext, d: Datum) -> Result<Datum> {
        match d {
            Datum::I64(0) | Datum::U64(0) => return Ok(Datum::Null),
            Datum::F64(f) if f == 0f64 => return Ok(Datum::Null),
            _ => {}
        }
        match (self, d) {
            (Datum::I64(l), Datum::I64(r)) => Ok(Datum::I64(l % r)),
            (Datum::I64(l), Datum::U64(r)) => {
                if l < 0 {
                    Ok(Datum::I64(-((l.overflowing_neg().0 as u64 % r) as i64)))
                } else {
                    Ok(Datum::I64((l as u64 % r) as i64))
                }
            }
            (Datum::U64(l), Datum::I64(r)) => Ok(Datum::U64(l % r.overflowing_abs().0 as u64)),
            (Datum::U64(l), Datum::U64(r)) => Ok(Datum::U64(l % r)),
            (Datum::F64(l), Datum::F64(r)) => Ok(Datum::F64(l % r)),
            (Datum::Dec(l), Datum::Dec(r)) => match l % r {
                None => Ok(Datum::Null),
                Some(res) => {
                    let d: Result<Decimal> = res.into();
                    d.map(Datum::Dec)
                }
            },
            (l, r) => Err(invalid_type!("{:?} can't mod {:?}", l, r)),
        }
    }

    // `checked_int_div` computes the result of a / b, both a and b are integer.
    pub fn checked_int_div(self, _: &mut EvalContext, datum: Datum) -> Result<Datum> {
        match datum {
            Datum::I64(0) | Datum::U64(0) => return Ok(Datum::Null),
            _ => {}
        }
        match (self, datum) {
            (Datum::I64(left), Datum::I64(right)) => match left.checked_div(right) {
                None => Err(box_err!("{} intdiv {} overflow", left, right)),
                Some(res) => Ok(Datum::I64(res)),
            },
            (Datum::I64(left), Datum::U64(right)) => {
                if left < 0 {
                    if left.overflowing_neg().0 as u64 >= right {
                        Err(box_err!("{} intdiv {} overflow", left, right))
                    } else {
                        Ok(Datum::U64(0))
                    }
                } else {
                    Ok(Datum::U64(left as u64 / right))
                }
            }
            (Datum::U64(left), Datum::I64(right)) => {
                if right < 0 {
                    if left != 0 && right.overflowing_neg().0 as u64 <= left {
                        Err(box_err!("{} intdiv {} overflow", left, right))
                    } else {
                        Ok(Datum::U64(0))
                    }
                } else {
                    Ok(Datum::U64(left / right as u64))
                }
            }
            (Datum::U64(left), Datum::U64(right)) => Ok(Datum::U64(left / right)),
            (left, right) => {
                let a = left.into_dec()?;
                let b = right.into_dec()?;
                match a / b {
                    None => Ok(Datum::Null),
                    Some(res) => {
                        let i = res.unwrap().as_i64().unwrap();
                        Ok(Datum::I64(i))
                    }
                }
            }
        }
    }
}

impl From<bool> for Datum {
    fn from(b: bool) -> Datum {
        if b {
            Datum::I64(1)
        } else {
            Datum::I64(0)
        }
    }
}

impl<T: Into<Datum>> From<Option<T>> for Datum {
    fn from(opt: Option<T>) -> Datum {
        match opt {
            None => Datum::Null,
            Some(t) => t.into(),
        }
    }
}

impl<'a, T: Clone + Into<Datum>> From<Cow<'a, T>> for Datum {
    fn from(c: Cow<'a, T>) -> Datum {
        c.into_owned().into()
    }
}

impl From<Vec<u8>> for Datum {
    fn from(data: Vec<u8>) -> Datum {
        Datum::Bytes(data)
    }
}

impl<'a> From<&'a [u8]> for Datum {
    fn from(data: &'a [u8]) -> Datum {
        data.to_vec().into()
    }
}

impl<'a> From<Cow<'a, [u8]>> for Datum {
    fn from(data: Cow<'_, [u8]>) -> Datum {
        data.into_owned().into()
    }
}

impl From<Duration> for Datum {
    fn from(dur: Duration) -> Datum {
        Datum::Dur(dur)
    }
}

impl From<i64> for Datum {
    fn from(data: i64) -> Datum {
        Datum::I64(data)
    }
}

impl From<u64> for Datum {
    fn from(data: u64) -> Datum {
        Datum::U64(data)
    }
}

impl From<Decimal> for Datum {
    fn from(data: Decimal) -> Datum {
        Datum::Dec(data)
    }
}

impl From<Time> for Datum {
    fn from(t: Time) -> Datum {
        Datum::Time(t)
    }
}

impl From<f64> for Datum {
    fn from(data: f64) -> Datum {
        Datum::F64(data)
    }
}

impl From<Json> for Datum {
    fn from(data: Json) -> Datum {
        Datum::Json(data)
    }
}

/// `decode_datum` decodes on a datum from a byte slice generated by tidb.
pub fn decode_datum(data: &mut BytesSlice<'_>) -> Result<Datum> {
    if !data.is_empty() {
        let flag = data[0];
        *data = &data[1..];
        let datum = match flag {
            INT_FLAG => number::decode_i64(data).map(Datum::I64)?,
            UINT_FLAG => number::decode_u64(data).map(Datum::U64)?,
            BYTES_FLAG => bytes::decode_bytes(data, false).map(Datum::Bytes)?,
            COMPACT_BYTES_FLAG => bytes::decode_compact_bytes(data).map(Datum::Bytes)?,
            NIL_FLAG => Datum::Null,
            FLOAT_FLAG => number::decode_f64(data).map(Datum::F64)?,
            DURATION_FLAG => {
                let nanos = number::decode_i64(data)?;
                let dur = Duration::from_nanos(nanos, MAX_FSP)?;
                Datum::Dur(dur)
            }
            DECIMAL_FLAG => Decimal::decode(data).map(Datum::Dec)?,
            VAR_INT_FLAG => number::decode_var_i64(data).map(Datum::I64)?,
            VAR_UINT_FLAG => number::decode_var_u64(data).map(Datum::U64)?,
            JSON_FLAG => Json::decode(data).map(Datum::Json)?,
            f => Err(invalid_type!("unsupported data type `{}`", f))?,
        };
        Ok(datum)
    } else {
        Err(Error::unexpected_eof())
    }
}

/// `decode` decodes all datum from a byte slice generated by tidb.
pub fn decode(data: &mut BytesSlice<'_>) -> Result<Vec<Datum>> {
    let mut res = vec![];
    while !data.is_empty() {
        let v = decode_datum(data)?;
        res.push(v);
    }
    Ok(res)
}

/// `DatumEncoder` encodes the datum.
pub trait DatumEncoder: BytesEncoder + DecimalEncoder + JsonEncoder {
    /// Encode values to buf slice.
    fn encode(&mut self, values: &[Datum], comparable: bool) -> Result<()> {
        let mut find_min = false;
        for v in values {
            if find_min {
                return Err(invalid_type!(
                    "MinValue should be the last datum.".to_owned()
                ));
            }
            match *v {
                Datum::I64(i) => {
                    if comparable {
                        self.write_u8(INT_FLAG)?;
                        self.encode_i64(i)?;
                    } else {
                        self.write_u8(VAR_INT_FLAG)?;
                        self.encode_var_i64(i)?;
                    }
                }
                Datum::U64(u) => {
                    if comparable {
                        self.write_u8(UINT_FLAG)?;
                        self.encode_u64(u)?;
                    } else {
                        self.write_u8(VAR_UINT_FLAG)?;
                        self.encode_var_u64(u)?;
                    }
                }
                Datum::Bytes(ref bs) => {
                    if comparable {
                        self.write_u8(BYTES_FLAG)?;
                        self.encode_bytes(bs, false)?;
                    } else {
                        self.write_u8(COMPACT_BYTES_FLAG)?;
                        self.encode_compact_bytes(bs)?;
                    }
                }
                Datum::F64(f) => {
                    self.write_u8(FLOAT_FLAG)?;
                    self.encode_f64(f)?;
                }
                Datum::Null => self.write_u8(NIL_FLAG)?,
                Datum::Min => {
                    self.write_u8(BYTES_FLAG)?; // for backward compatibility
                    find_min = true;
                }
                Datum::Max => self.write_u8(MAX_FLAG)?,
                Datum::Time(ref t) => {
                    self.write_u8(UINT_FLAG)?;
                    self.encode_u64(t.to_packed_u64())?;
                }
                Datum::Dur(ref d) => {
                    self.write_u8(DURATION_FLAG)?;
                    self.encode_i64(d.to_nanos())?;
                }
                Datum::Dec(ref d) => {
                    self.write_u8(DECIMAL_FLAG)?;
                    let (prec, frac) = d.prec_and_frac();
                    self.encode_decimal(d, prec, frac)?;
                }
                Datum::Json(ref j) => {
                    self.write_u8(JSON_FLAG)?;
                    self.encode_json(j)?;
                }
            }
        }
        Ok(())
    }
}

impl<T: Write> DatumEncoder for T {}

/// Get the approximate needed buffer size of values.
///
/// This function ensures that encoded values must fit in the given buffer size.
pub fn approximate_size(values: &[Datum], comparable: bool) -> usize {
    values
        .iter()
        .map(|v| {
            1 + match *v {
                Datum::I64(_) => {
                    if comparable {
                        number::I64_SIZE
                    } else {
                        number::MAX_VAR_I64_LEN
                    }
                }
                Datum::U64(_) => {
                    if comparable {
                        number::U64_SIZE
                    } else {
                        number::MAX_VAR_U64_LEN
                    }
                }
                Datum::F64(_) => number::F64_SIZE,
                Datum::Time(_) => number::U64_SIZE,
                Datum::Dur(_) => number::I64_SIZE,
                Datum::Bytes(ref bs) => {
                    if comparable {
                        bytes::max_encoded_bytes_size(bs.len())
                    } else {
                        bs.len() + number::MAX_VAR_I64_LEN
                    }
                }
                Datum::Dec(ref d) => d.approximate_encoded_size(),
                Datum::Json(ref d) => d.binary_len(),
                Datum::Null | Datum::Min | Datum::Max => 0,
            }
        })
        .sum()
}

/// `encode` encodes a datum slice into a buffer.
/// Uses comparable to encode or not to encode a memory comparable buffer.
pub fn encode(values: &[Datum], comparable: bool) -> Result<Vec<u8>> {
    let mut buf = vec![];
    encode_to(&mut buf, values, comparable)?;
    buf.shrink_to_fit();
    Ok(buf)
}

/// `encode_key` encodes a datum slice into a memory comparable buffer as the key.
pub fn encode_key(values: &[Datum]) -> Result<Vec<u8>> {
    encode(values, true)
}

/// `encode_value` encodes a datum slice into a buffer.
pub fn encode_value(values: &[Datum]) -> Result<Vec<u8>> {
    encode(values, false)
}

/// `encode_to` encodes a datum slice and appends the buffer to a vector.
/// Uses comparable to encode a memory comparable buffer or not.
pub fn encode_to(buf: &mut Vec<u8>, values: &[Datum], comparable: bool) -> Result<()> {
    buf.reserve(approximate_size(values, comparable));
    buf.encode(values, comparable)?;
    Ok(())
}

/// Split bytes array into two part: first one is a whole datum's encoded data,
/// and the second part is the remaining data.
pub fn split_datum(buf: &[u8], desc: bool) -> Result<(&[u8], &[u8])> {
    if buf.is_empty() {
        return Err(box_err!("{} is too short", escape(buf)));
    }
    let pos = match buf[0] {
        INT_FLAG => number::I64_SIZE,
        UINT_FLAG => number::U64_SIZE,
        BYTES_FLAG => bytes::encoded_bytes_len(&buf[1..], desc),
        COMPACT_BYTES_FLAG => bytes::encoded_compact_len(&buf[1..]),
        NIL_FLAG => 0,
        FLOAT_FLAG => number::F64_SIZE,
        DURATION_FLAG => number::I64_SIZE,
        DECIMAL_FLAG => mysql::dec_encoded_len(&buf[1..])?,
        VAR_INT_FLAG => {
            let mut v = &buf[1..];
            let l = v.len();
            number::decode_var_i64(&mut v)?;
            l - v.len()
        }
        VAR_UINT_FLAG => {
            let mut v = &buf[1..];
            let l = v.len();
            number::decode_var_u64(&mut v)?;
            l - v.len()
        }
        JSON_FLAG => {
            let mut v = &buf[1..];
            let l = v.len();
            Json::decode(&mut v)?;
            l - v.len()
        }
        f => return Err(invalid_type!("unsupported data type `{}`", f)),
    };
    if buf.len() < pos + 1 {
        return Err(box_err!("{} is too short", escape(buf)));
    }
    Ok(buf.split_at(1 + pos))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coprocessor::codec::mysql::{Decimal, Duration, Time, MAX_FSP};
    use crate::coprocessor::dag::expr::{EvalConfig, EvalContext};
    use crate::util::as_slice;

    use std::cmp::Ordering;
    use std::sync::Arc;
    use std::time::Duration as StdDuration;
    use std::{i16, i32, i64, i8, u16, u32, u64, u8};

    fn same_type(l: &Datum, r: &Datum) -> bool {
        match (l, r) {
            (&Datum::I64(_), &Datum::I64(_))
            | (&Datum::U64(_), &Datum::U64(_))
            | (&Datum::F64(_), &Datum::F64(_))
            | (&Datum::Max, &Datum::Max)
            | (&Datum::Min, &Datum::Min)
            | (&Datum::Bytes(_), &Datum::Bytes(_))
            | (&Datum::Dur(_), &Datum::Dur(_))
            | (&Datum::Null, &Datum::Null)
            | (&Datum::Time(_), &Datum::Time(_))
            | (&Datum::Json(_), &Datum::Json(_)) => true,
            (&Datum::Dec(ref d1), &Datum::Dec(ref d2)) => d1.prec_and_frac() == d2.prec_and_frac(),
            _ => false,
        }
    }

    #[test]
    fn test_datum_codec() {
        let table = vec![
            vec![Datum::I64(1)],
            vec![Datum::F64(1.0), Datum::F64(3.15), b"123".as_ref().into()],
            vec![
                Datum::U64(1),
                Datum::F64(3.15),
                b"123".as_ref().into(),
                Datum::I64(-1),
            ],
            vec![Datum::Null],
            vec![
                Duration::new(StdDuration::from_millis(23), false, MAX_FSP)
                    .unwrap()
                    .into(),
                Duration::new(StdDuration::from_millis(23), true, MAX_FSP)
                    .unwrap()
                    .into(),
            ],
            vec![
                Datum::U64(1),
                Datum::Dec(Decimal::from_f64(2.3).unwrap()),
                Datum::Dec("-34".parse().unwrap()),
            ],
            vec![
                Datum::Dec("1234.00".parse().unwrap()),
                Datum::Dec("1234".parse().unwrap()),
                Datum::Dec("12.34".parse().unwrap()),
                Datum::Dec("12.340".parse().unwrap()),
                Datum::Dec("0.1234".parse().unwrap()),
                Datum::Dec("0.0".parse().unwrap()),
                Datum::Dec("0".parse().unwrap()),
                Datum::Dec("-0.0".parse().unwrap()),
                Datum::Dec("-0.0000".parse().unwrap()),
                Datum::Dec("-1234.00".parse().unwrap()),
                Datum::Dec("-1234".parse().unwrap()),
                Datum::Dec("-12.34".parse().unwrap()),
                Datum::Dec("-12.340".parse().unwrap()),
                Datum::Dec("-0.1234".parse().unwrap()),
            ],
            vec![
                Datum::Json(Json::from_str(r#"{"key":"value"}"#).unwrap()),
                Datum::Json(Json::from_str(r#"["d1","d2"]"#).unwrap()),
                Datum::Json(Json::from_str(r#"3"#).unwrap()),
                Datum::Json(Json::from_str(r#"3.0"#).unwrap()),
                Datum::Json(Json::from_str(r#"null"#).unwrap()),
                Datum::Json(Json::from_str(r#"true"#).unwrap()),
                Datum::Json(Json::from_str(r#"false"#).unwrap()),
                Datum::Json(
                    Json::from_str(
                        r#"[
                                    {
                                        "a": 1,
                                        "b": true
                                    },
                                    3,
                                    3.5,
                                    "hello, world",
                                    null,
                                    true]"#,
                    )
                    .unwrap(),
                ),
            ],
        ];
        for vs in table {
            let mut buf = encode_key(&vs).unwrap();
            let decoded = decode(&mut buf.as_slice()).unwrap();
            assert_eq!(vs, decoded);

            buf = encode_value(&vs).unwrap();
            let decoded = decode(&mut buf.as_slice()).unwrap();
            assert_eq!(vs, decoded);
        }
    }

    #[test]
    fn test_datum_cmp() {
        let tests = vec![
            (Datum::F64(-1.0), Datum::Min, Ordering::Greater),
            (Datum::F64(1.0), Datum::Max, Ordering::Less),
            (Datum::F64(1.0), Datum::F64(1.0), Ordering::Equal),
            (Datum::F64(1.0), b"1".as_ref().into(), Ordering::Equal),
            (Datum::I64(1), Datum::I64(1), Ordering::Equal),
            (Datum::I64(-1), Datum::I64(1), Ordering::Less),
            (Datum::I64(-1), b"-1".as_ref().into(), Ordering::Equal),
            (Datum::U64(1), Datum::U64(1), Ordering::Equal),
            (Datum::U64(1), Datum::I64(-1), Ordering::Greater),
            (Datum::U64(1), b"1".as_ref().into(), Ordering::Equal),
            (
                Datum::Dec(1i64.into()),
                Datum::Dec(1i64.into()),
                Ordering::Equal,
            ),
            (
                Datum::Dec(1i64.into()),
                b"2".as_ref().into(),
                Ordering::Less,
            ),
            (
                Datum::Dec(1i64.into()),
                b"0.2".as_ref().into(),
                Ordering::Greater,
            ),
            (
                Datum::Dec(1i64.into()),
                b"1".as_ref().into(),
                Ordering::Equal,
            ),
            (b"1".as_ref().into(), b"1".as_ref().into(), Ordering::Equal),
            (b"1".as_ref().into(), Datum::I64(-1), Ordering::Greater),
            (b"1".as_ref().into(), Datum::U64(1), Ordering::Equal),
            (
                b"1".as_ref().into(),
                Datum::Dec(1i64.into()),
                Ordering::Equal,
            ),
            (Datum::Null, Datum::I64(2), Ordering::Less),
            (Datum::Null, Datum::Null, Ordering::Equal),
            (false.into(), Datum::Null, Ordering::Greater),
            (false.into(), true.into(), Ordering::Less),
            (true.into(), true.into(), Ordering::Equal),
            (false.into(), false.into(), Ordering::Equal),
            (true.into(), Datum::I64(2), Ordering::Less),
            (Datum::F64(1.23), Datum::Null, Ordering::Greater),
            (Datum::F64(0.0), Datum::F64(3.45), Ordering::Less),
            (Datum::F64(354.23), Datum::F64(3.45), Ordering::Greater),
            (Datum::F64(3.452), Datum::F64(3.452), Ordering::Equal),
            (Datum::I64(432), Datum::Null, Ordering::Greater),
            (Datum::I64(-4), Datum::I64(32), Ordering::Less),
            (Datum::I64(4), Datum::I64(-32), Ordering::Greater),
            (Datum::I64(432), Datum::I64(12), Ordering::Greater),
            (Datum::I64(23), Datum::I64(128), Ordering::Less),
            (Datum::I64(123), Datum::I64(123), Ordering::Equal),
            (Datum::I64(23), Datum::I64(123), Ordering::Less),
            (Datum::I64(133), Datum::I64(183), Ordering::Less),
            (Datum::U64(123), Datum::U64(183), Ordering::Less),
            (Datum::U64(2), Datum::I64(-2), Ordering::Greater),
            (Datum::U64(2), Datum::I64(1), Ordering::Greater),
            (b"".as_ref().into(), Datum::Null, Ordering::Greater),
            (b"".as_ref().into(), b"24".as_ref().into(), Ordering::Less),
            (
                b"aasf".as_ref().into(),
                b"4".as_ref().into(),
                Ordering::Greater,
            ),
            (b"".as_ref().into(), b"".as_ref().into(), Ordering::Equal),
            (
                Duration::new(StdDuration::from_millis(34), false, 2)
                    .unwrap()
                    .into(),
                Datum::Null,
                Ordering::Greater,
            ),
            (
                Duration::new(StdDuration::from_millis(3340), false, 2)
                    .unwrap()
                    .into(),
                Duration::new(StdDuration::from_millis(29034), false, 2)
                    .unwrap()
                    .into(),
                Ordering::Less,
            ),
            (
                Duration::new(StdDuration::from_millis(3340), false, 2)
                    .unwrap()
                    .into(),
                Duration::new(StdDuration::from_millis(34), false, 2)
                    .unwrap()
                    .into(),
                Ordering::Greater,
            ),
            (
                Duration::new(StdDuration::from_millis(34), false, 2)
                    .unwrap()
                    .into(),
                Duration::new(StdDuration::from_millis(34), false, 2)
                    .unwrap()
                    .into(),
                Ordering::Equal,
            ),
            (
                Duration::new(StdDuration::from_millis(34), true, 2)
                    .unwrap()
                    .into(),
                Datum::Null,
                Ordering::Greater,
            ),
            (
                Duration::new(StdDuration::from_millis(0), true, 2)
                    .unwrap()
                    .into(),
                Datum::I64(0),
                Ordering::Equal,
            ),
            (
                Duration::new(StdDuration::from_millis(3340), false, 2)
                    .unwrap()
                    .into(),
                Duration::new(StdDuration::from_millis(29034), true, 2)
                    .unwrap()
                    .into(),
                Ordering::Greater,
            ),
            (
                Duration::new(StdDuration::from_millis(3340), true, 2)
                    .unwrap()
                    .into(),
                Duration::new(StdDuration::from_millis(34), false, 2)
                    .unwrap()
                    .into(),
                Ordering::Less,
            ),
            (
                Duration::new(StdDuration::from_millis(34), false, 2)
                    .unwrap()
                    .into(),
                Duration::new(StdDuration::from_millis(34), true, 2)
                    .unwrap()
                    .into(),
                Ordering::Greater,
            ),
            (
                Duration::new(StdDuration::from_millis(34), true, 2)
                    .unwrap()
                    .into(),
                b"-00.34".as_ref().into(),
                Ordering::Greater,
            ),
            (
                Time::parse_utc_datetime("2011-10-10 00:00:00", 0)
                    .unwrap()
                    .into(),
                Time::parse_utc_datetime("2000-12-12 11:11:11", 0)
                    .unwrap()
                    .into(),
                Ordering::Greater,
            ),
            (
                Time::parse_utc_datetime("2011-10-10 00:00:00", 0)
                    .unwrap()
                    .into(),
                b"2000-12-12 11:11:11".as_ref().into(),
                Ordering::Greater,
            ),
            (
                Time::parse_utc_datetime("2000-10-10 00:00:00", 0)
                    .unwrap()
                    .into(),
                Time::parse_utc_datetime("2001-10-10 00:00:00", 0)
                    .unwrap()
                    .into(),
                Ordering::Less,
            ),
            (
                Time::parse_utc_datetime("2000-10-10 00:00:00", 0)
                    .unwrap()
                    .into(),
                Time::parse_utc_datetime("2000-10-10 00:00:00", 0)
                    .unwrap()
                    .into(),
                Ordering::Equal,
            ),
            (
                Time::parse_utc_datetime("2000-10-10 00:00:00", 0)
                    .unwrap()
                    .into(),
                Datum::I64(20001010000000),
                Ordering::Equal,
            ),
            (
                Time::parse_utc_datetime("2000-10-10 00:00:00", 0)
                    .unwrap()
                    .into(),
                Datum::I64(0),
                Ordering::Greater,
            ),
            (
                Datum::I64(0),
                Time::parse_utc_datetime("2000-10-10 00:00:00", 0)
                    .unwrap()
                    .into(),
                Ordering::Less,
            ),
            (
                Datum::Dec("1234".parse().unwrap()),
                Datum::Dec("123400".parse().unwrap()),
                Ordering::Less,
            ),
            (
                Datum::Dec("12340".parse().unwrap()),
                Datum::Dec("123400".parse().unwrap()),
                Ordering::Less,
            ),
            (
                Datum::Dec("1234".parse().unwrap()),
                Datum::Dec("1234.5".parse().unwrap()),
                Ordering::Less,
            ),
            (
                Datum::Dec("1234".parse().unwrap()),
                Datum::Dec("1234.0000".parse().unwrap()),
                Ordering::Equal,
            ),
            (
                Datum::Dec("1234".parse().unwrap()),
                Datum::Dec("12.34".parse().unwrap()),
                Ordering::Greater,
            ),
            (
                Datum::Dec("12.34".parse().unwrap()),
                Datum::Dec("12.35".parse().unwrap()),
                Ordering::Less,
            ),
            (
                Datum::Dec("0.12".parse().unwrap()),
                Datum::Dec("0.1234".parse().unwrap()),
                Ordering::Less,
            ),
            (
                Datum::Dec("0.1234".parse().unwrap()),
                Datum::Dec("12.3400".parse().unwrap()),
                Ordering::Less,
            ),
            (
                Datum::Dec("0.1234".parse().unwrap()),
                Datum::Dec("0.1235".parse().unwrap()),
                Ordering::Less,
            ),
            (
                Datum::Dec("0.123400".parse().unwrap()),
                Datum::Dec("12.34".parse().unwrap()),
                Ordering::Less,
            ),
            (
                Datum::Dec("12.34000".parse().unwrap()),
                Datum::Dec("12.34".parse().unwrap()),
                Ordering::Equal,
            ),
            (
                Datum::Dec("0.01234".parse().unwrap()),
                Datum::Dec("0.01235".parse().unwrap()),
                Ordering::Less,
            ),
            (
                Datum::Dec("0.1234".parse().unwrap()),
                Datum::Dec("0".parse().unwrap()),
                Ordering::Greater,
            ),
            (
                Datum::Dec("0.0000".parse().unwrap()),
                Datum::Dec("0".parse().unwrap()),
                Ordering::Equal,
            ),
            (
                Datum::Dec("0.0001".parse().unwrap()),
                Datum::Dec("0".parse().unwrap()),
                Ordering::Greater,
            ),
            (
                Datum::Dec("0.0001".parse().unwrap()),
                Datum::Dec("0.0000".parse().unwrap()),
                Ordering::Greater,
            ),
            (
                Datum::Dec("0".parse().unwrap()),
                Datum::Dec("-0.0000".parse().unwrap()),
                Ordering::Equal,
            ),
            (
                Datum::Dec("-0.0001".parse().unwrap()),
                Datum::Dec("0".parse().unwrap()),
                Ordering::Less,
            ),
            (
                Datum::Dec("-0.1234".parse().unwrap()),
                Datum::Dec("0".parse().unwrap()),
                Ordering::Less,
            ),
            (
                Datum::Dec("-0.1234".parse().unwrap()),
                Datum::Dec("-0.12".parse().unwrap()),
                Ordering::Less,
            ),
            (
                Datum::Dec("-0.12".parse().unwrap()),
                Datum::Dec("-0.1234".parse().unwrap()),
                Ordering::Greater,
            ),
            (
                Datum::Dec("-0.12".parse().unwrap()),
                Datum::Dec("-0.1200".parse().unwrap()),
                Ordering::Equal,
            ),
            (
                Datum::Dec("-0.1234".parse().unwrap()),
                Datum::Dec("0.1234".parse().unwrap()),
                Ordering::Less,
            ),
            (
                Datum::Dec("-1.234".parse().unwrap()),
                Datum::Dec("-12.34".parse().unwrap()),
                Ordering::Greater,
            ),
            (
                Datum::Dec("-0.1234".parse().unwrap()),
                Datum::Dec("-12.34".parse().unwrap()),
                Ordering::Greater,
            ),
            (
                Datum::Dec("-12.34".parse().unwrap()),
                Datum::Dec("1234".parse().unwrap()),
                Ordering::Less,
            ),
            (
                Datum::Dec("-12.34".parse().unwrap()),
                Datum::Dec("-12.35".parse().unwrap()),
                Ordering::Greater,
            ),
            (
                Datum::Dec("-0.01234".parse().unwrap()),
                Datum::Dec("-0.01235".parse().unwrap()),
                Ordering::Greater,
            ),
            (
                Datum::Dec("-1234".parse().unwrap()),
                Datum::Dec("-123400".parse().unwrap()),
                Ordering::Greater,
            ),
            (
                Datum::Dec("-12340".parse().unwrap()),
                Datum::Dec("-123400".parse().unwrap()),
                Ordering::Greater,
            ),
            (Datum::Dec(100.into()), Datum::I64(1), Ordering::Greater),
            (Datum::Dec((-100).into()), Datum::I64(-1), Ordering::Less),
            (Datum::Dec((-100).into()), Datum::I64(-100), Ordering::Equal),
            (Datum::Dec(100.into()), Datum::I64(100), Ordering::Equal),
            // Test for int type decimal.
            (
                Datum::Dec((-1i64).into()),
                Datum::Dec(1i64.into()),
                Ordering::Less,
            ),
            (
                Datum::Dec(i64::MAX.into()),
                Datum::Dec(i64::MIN.into()),
                Ordering::Greater,
            ),
            (
                Datum::Dec(i64::MAX.into()),
                Datum::Dec(i32::MAX.into()),
                Ordering::Greater,
            ),
            (
                Datum::Dec(i32::MIN.into()),
                Datum::Dec(i16::MAX.into()),
                Ordering::Less,
            ),
            (
                Datum::Dec(i64::MIN.into()),
                Datum::Dec(i8::MAX.into()),
                Ordering::Less,
            ),
            (
                Datum::Dec(0i64.into()),
                Datum::Dec(i8::MAX.into()),
                Ordering::Less,
            ),
            (
                Datum::Dec(i8::MIN.into()),
                Datum::Dec(0i64.into()),
                Ordering::Less,
            ),
            (
                Datum::Dec(i16::MIN.into()),
                Datum::Dec(i16::MAX.into()),
                Ordering::Less,
            ),
            (
                Datum::Dec(1i64.into()),
                Datum::Dec((-1i64).into()),
                Ordering::Greater,
            ),
            (
                Datum::Dec(1i64.into()),
                Datum::Dec(0i64.into()),
                Ordering::Greater,
            ),
            (
                Datum::Dec((-1i64).into()),
                Datum::Dec(0i64.into()),
                Ordering::Less,
            ),
            (
                Datum::Dec(0i64.into()),
                Datum::Dec(0i64.into()),
                Ordering::Equal,
            ),
            (
                Datum::Dec(i16::MAX.into()),
                Datum::Dec(i16::MAX.into()),
                Ordering::Equal,
            ),
            // Test for uint type decimal.
            (
                Datum::Dec(0u64.into()),
                Datum::Dec(0u64.into()),
                Ordering::Equal,
            ),
            (
                Datum::Dec(1u64.into()),
                Datum::Dec(0u64.into()),
                Ordering::Greater,
            ),
            (
                Datum::Dec(0u64.into()),
                Datum::Dec(1u64.into()),
                Ordering::Less,
            ),
            (
                Datum::Dec(i8::MAX.into()),
                Datum::Dec(i16::MAX.into()),
                Ordering::Less,
            ),
            (
                Datum::Dec(u32::MAX.into()),
                Datum::Dec(i32::MAX.into()),
                Ordering::Greater,
            ),
            (
                Datum::Dec(u8::MAX.into()),
                Datum::Dec(i8::MAX.into()),
                Ordering::Greater,
            ),
            (
                Datum::Dec(u16::MAX.into()),
                Datum::Dec(i32::MAX.into()),
                Ordering::Less,
            ),
            (
                Datum::Dec(u64::MAX.into()),
                Datum::Dec(i64::MAX.into()),
                Ordering::Greater,
            ),
            (
                Datum::Dec(i64::MAX.into()),
                Datum::Dec(u32::MAX.into()),
                Ordering::Greater,
            ),
            (
                Datum::Dec(u64::MAX.into()),
                Datum::Dec(0u64.into()),
                Ordering::Greater,
            ),
            (
                Datum::Dec(0u64.into()),
                Datum::Dec(u64::MAX.into()),
                Ordering::Less,
            ),
            (
                b"abc".as_ref().into(),
                b"ab".as_ref().into(),
                Ordering::Greater,
            ),
            (b"123".as_ref().into(), Datum::I64(1234), Ordering::Less),
            (b"1".as_ref().into(), Datum::Max, Ordering::Less),
            (b"".as_ref().into(), Datum::Null, Ordering::Greater),
            (Datum::Max, Datum::Max, Ordering::Equal),
            (Datum::Max, Datum::Min, Ordering::Greater),
            (Datum::Null, Datum::Min, Ordering::Less),
            (Datum::Min, Datum::Min, Ordering::Equal),
            (
                Datum::Json(Json::from_str(r#"{"key":"value"}"#).unwrap()),
                Datum::Json(Json::from_str(r#"{"key":"value"}"#).unwrap()),
                Ordering::Equal,
            ),
            (Datum::I64(18), Datum::Json(Json::I64(18)), Ordering::Equal),
            (Datum::U64(18), Datum::Json(Json::I64(20)), Ordering::Less),
            (
                Datum::F64(1.2),
                Datum::Json(Json::Double(1.0)),
                Ordering::Greater,
            ),
            (
                Datum::Dec(i32::MIN.into()),
                Datum::Json(Json::Double(f64::from(i32::MIN))),
                Ordering::Equal,
            ),
            (
                b"hi".as_ref().into(),
                Datum::Json(Json::from_str(r#""hi""#).unwrap()),
                Ordering::Equal,
            ),
            (
                Datum::Max,
                Datum::Json(Json::from_str(r#""MAX""#).unwrap()),
                Ordering::Less,
            ),
        ];
        let mut ctx = EvalContext::default();
        for (lhs, rhs, ret) in tests {
            if ret != lhs.cmp(&mut ctx, &rhs).unwrap() {
                panic!("{:?} should be {:?} to {:?}", lhs, ret, rhs);
            }

            let rev_ret = ret.reverse();

            if rev_ret != rhs.cmp(&mut ctx, &lhs).unwrap() {
                panic!("{:?} should be {:?} to {:?}", rhs, rev_ret, lhs);
            }

            if same_type(&lhs, &rhs) {
                let lhs_bs = encode_key(as_slice(&lhs)).unwrap();
                let rhs_bs = encode_key(as_slice(&rhs)).unwrap();

                if ret != lhs_bs.cmp(&rhs_bs) {
                    panic!("{:?} should be {:?} to {:?} when encoded", lhs, ret, rhs);
                }

                let lhs_str = format!("{:?}", lhs);
                let rhs_str = format!("{:?}", rhs);
                if ret == Ordering::Equal {
                    assert_eq!(lhs_str, rhs_str);
                }
            }
        }
    }

    #[test]
    fn test_datum_to_bool() {
        let tests = vec![
            (Datum::I64(0), Some(false)),
            (Datum::I64(-1), Some(true)),
            (Datum::U64(0), Some(false)),
            (Datum::U64(1), Some(true)),
            (Datum::F64(0f64), Some(false)),
            (Datum::F64(0.4), Some(false)),
            (Datum::F64(0.5), Some(true)),
            (Datum::F64(-0.5), Some(true)),
            (Datum::F64(-0.4), Some(false)),
            (Datum::Null, None),
            (b"".as_ref().into(), Some(false)),
            (b"0.5".as_ref().into(), Some(false)),
            (b"0".as_ref().into(), Some(false)),
            (b"2".as_ref().into(), Some(true)),
            (b"abc".as_ref().into(), Some(false)),
            (
                Time::parse_utc_datetime("2011-11-10 11:11:11.999999", 6)
                    .unwrap()
                    .into(),
                Some(true),
            ),
            (
                Duration::parse(b"11:11:11.999999", MAX_FSP).unwrap().into(),
                Some(true),
            ),
            (
                Datum::Dec(Decimal::from_f64(0.1415926).unwrap()),
                Some(false),
            ),
            (Datum::Dec(0u64.into()), Some(false)),
        ];

        let mut ctx = EvalContext::new(Arc::new(EvalConfig::default_for_test()));

        for (d, b) in tests {
            if d.clone().into_bool(&mut ctx).unwrap() != b {
                panic!("expect {:?} to be {:?}", d, b);
            }
        }
    }

    #[test]
    fn test_split_datum() {
        let table = vec![
            vec![Datum::I64(1)],
            vec![
                Datum::F64(1f64),
                Datum::F64(3.15),
                Datum::Bytes(b"123".to_vec()),
            ],
            vec![
                Datum::U64(1),
                Datum::F64(3.15),
                Datum::Bytes(b"123".to_vec()),
                Datum::I64(-1),
            ],
            vec![Datum::I64(1), Datum::I64(0)],
            vec![Datum::Null],
            vec![Datum::I64(100), Datum::U64(100)],
            vec![Datum::U64(1), Datum::U64(1)],
            vec![Datum::Dec(10.into())],
            vec![
                Datum::F64(1f64),
                Datum::F64(3.15),
                Datum::Bytes(b"123456789012345".to_vec()),
            ],
            vec![Datum::Json(Json::from_str(r#"{"key":"value"}"#).unwrap())],
            vec![
                Datum::F64(1f64),
                Datum::Json(Json::from_str(r#"{"key":"value"}"#).unwrap()),
                Datum::F64(3.15),
                Datum::Bytes(b"123456789012345".to_vec()),
            ],
        ];

        for case in table {
            let key_bs = encode_key(&case).unwrap();
            let mut buf = key_bs.as_slice();
            for exp in &case {
                let (act, rem) = split_datum(buf, false).unwrap();
                let exp_bs = encode_key(as_slice(exp)).unwrap();
                assert_eq!(exp_bs, act);
                buf = rem;
            }
            assert!(buf.is_empty());

            let value_bs = encode_value(&case).unwrap();
            let mut buf = value_bs.as_slice();
            for exp in &case {
                let (act, rem) = split_datum(buf, false).unwrap();
                let exp_bs = encode_value(as_slice(exp)).unwrap();
                assert_eq!(exp_bs, act);
                buf = rem;
            }
            assert!(buf.is_empty());
        }
    }

    #[test]
    fn test_coerce_datum() {
        let cases = vec![
            (Datum::I64(1), Datum::I64(1), Datum::I64(1), Datum::I64(1)),
            (Datum::U64(1), Datum::I64(1), Datum::U64(1), Datum::I64(1)),
            (
                Datum::U64(1),
                Datum::Dec(1.into()),
                Datum::Dec(1.into()),
                Datum::Dec(1.into()),
            ),
            (
                Datum::F64(1.0),
                Datum::Dec(1.into()),
                Datum::F64(1.0),
                Datum::F64(1.0),
            ),
            (
                Datum::F64(1.0),
                Datum::F64(1.0),
                Datum::F64(1.0),
                Datum::F64(1.0),
            ),
        ];

        for (x, y, exp_x, exp_y) in cases {
            let (res_x, res_y) = Datum::coerce(x, y).unwrap();
            assert_eq!(res_x, exp_x);
            assert_eq!(res_y, exp_y);
        }
    }

    #[test]
    fn test_cast_as_json() {
        let tests = vec![
            (Datum::I64(1), "1.0"),
            (Datum::F64(3.3), "3.3"),
            (
                Datum::Bytes(br#""Hello,world""#.to_vec()),
                r#""Hello,world""#,
            ),
            (Datum::Bytes(b"[1, 2, 3]".to_vec()), "[1, 2, 3]"),
            (Datum::Bytes(b"{}".to_vec()), "{}"),
            (Datum::I64(1), "true"),
        ];

        for (d, json) in tests {
            assert_eq!(d.cast_as_json().unwrap(), json.parse().unwrap());
        }

        let illegal_cases = vec![
            Datum::Bytes(b"hello,world".to_vec()),
            Datum::Null,
            Datum::Max,
            Datum::Min,
        ];

        for d in illegal_cases {
            assert!(d.cast_as_json().is_err());
        }
    }

    #[test]
    fn test_datum_into_json() {
        let tests = vec![
            (Datum::I64(1), "1.0"),
            (Datum::F64(3.3), "3.3"),
            (Datum::Bytes(b"Hello,world".to_vec()), r#""Hello,world""#),
            (Datum::Bytes(b"[1, 2, 3]".to_vec()), r#""[1, 2, 3]""#),
            (Datum::Null, "null"),
        ];

        for (d, json) in tests {
            assert_eq!(d.into_json().unwrap(), json.parse().unwrap());
        }

        let illegal_cases = vec![Datum::Max, Datum::Min];

        for d in illegal_cases {
            assert!(d.into_json().is_err());
        }
    }

    #[test]
    fn test_into_f64() {
        let tests = vec![
            (Datum::I64(1), f64::from(1)),
            (Datum::U64(1), f64::from(1)),
            (Datum::F64(3.3), 3.3),
            (Datum::Bytes(b"Hello,world".to_vec()), f64::from(0)),
            (Datum::Bytes(b"123".to_vec()), f64::from(123)),
            (
                Datum::Time(Time::parse_utc_datetime("2012-12-31 11:30:45", 0).unwrap()),
                Decimal::from_bytes(b"20121231113045")
                    .unwrap()
                    .unwrap()
                    .as_f64()
                    .unwrap(),
            ),
            (
                Datum::Dur(Duration::parse(b"11:30:45", 0).unwrap()),
                f64::from(113045),
            ),
            (
                Datum::Dec(Decimal::from_bytes(b"11.2").unwrap().unwrap()),
                11.2,
            ),
            (
                Datum::Json(Json::from_str(r#"false"#).unwrap()),
                f64::from(0),
            ),
        ];

        let mut ctx = EvalContext::new(Arc::new(EvalConfig::default_for_test()));
        for (d, exp) in tests {
            let got = d.into_f64(&mut ctx).unwrap();
            assert_eq!(Datum::F64(got), Datum::F64(exp));
        }
    }

    #[test]
    fn test_into_i64() {
        let tests = vec![
            (Datum::Bytes(b"0".to_vec()), 0),
            (Datum::I64(1), 1),
            (Datum::U64(1), 1),
            (Datum::F64(3.3), 3),
            (Datum::Bytes(b"100".to_vec()), 100),
            (
                Datum::Time(Time::parse_utc_datetime("2012-12-31 11:30:45.9999", 0).unwrap()),
                20121231113046,
            ),
            (
                Datum::Dur(Duration::parse(b"11:30:45.999", 0).unwrap()),
                113046,
            ),
            (
                Datum::Dec(Decimal::from_bytes(b"11.2").unwrap().unwrap()),
                11,
            ),
            (Datum::Json(Json::from_str(r#"false"#).unwrap()), 0),
        ];

        let mut ctx = EvalContext::new(Arc::new(EvalConfig::default_for_test()));
        for (d, exp) in tests {
            let got = d.into_i64(&mut ctx).unwrap();
            assert_eq!(got, exp);
        }
    }
}
