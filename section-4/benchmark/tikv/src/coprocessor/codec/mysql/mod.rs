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

use super::Result;
use crate::util::escape;

/// `UNSPECIFIED_FSP` is the unspecified fractional seconds part.
pub const UNSPECIFIED_FSP: i8 = -1;
/// `MAX_FSP` is the maximum digit of fractional seconds part.
pub const MAX_FSP: i8 = 6;
/// `MIN_FSP` is the minimum digit of fractional seconds part.
pub const MIN_FSP: i8 = 0;
/// `DEFAULT_FSP` is the default digit of fractional seconds part.
/// `MySQL` use 0 as the default Fsp.
pub const DEFAULT_FSP: i8 = 0;

fn check_fsp(fsp: i8) -> Result<u8> {
    if fsp == UNSPECIFIED_FSP {
        return Ok(DEFAULT_FSP as u8);
    }
    if fsp > MAX_FSP || fsp < MIN_FSP {
        return Err(invalid_type!("Invalid fsp {}", fsp));
    }
    Ok(fsp as u8)
}

/// Parse string as if it's a fraction part of a number and keep
/// only `fsp` precision.
fn parse_frac(s: &[u8], fsp: u8) -> Result<u32> {
    if s.is_empty() {
        return Ok(0);
    }

    if s.iter().any(|&c| c < b'0' || c > b'9') {
        return Err(invalid_type!("{} contains invalid char", escape(s)));
    }
    let res = s
        .iter()
        .take(fsp as usize + 1)
        .fold(0, |l, r| l * 10 + u32::from(r - b'0'));
    if s.len() > fsp as usize {
        if res % 10 >= 5 {
            Ok(res / 10 + 1)
        } else {
            Ok(res / 10)
        }
    } else {
        Ok(res * 10u32.pow((fsp as usize - s.len()) as u32))
    }
}

pub mod charset;
pub mod decimal;
pub mod duration;
pub mod json;
pub mod time;

pub use self::decimal::{dec_encoded_len, Decimal, DecimalEncoder, Res, RoundMode};
pub use self::duration::{Duration, DurationEncoder};
pub use self::json::{parse_json_path_expr, Json, JsonEncoder, ModifyType, PathExpression};
pub use self::time::{Time, TimeEncoder, TimeType, Tz};

#[cfg(test)]
mod tests {
    #[test]
    fn test_parse_frac() {
        let cases = vec![
            ("1234567", 0, 0),
            ("1234567", 1, 1),
            ("0000567", 5, 6),
            ("1234567", 5, 12346),
            ("1234567", 6, 123457),
            ("9999999", 6, 1000000),
        ];

        for (s, fsp, exp) in cases {
            let res = super::parse_frac(s.as_bytes(), fsp).unwrap();
            assert_eq!(res, exp);
        }
    }
}
