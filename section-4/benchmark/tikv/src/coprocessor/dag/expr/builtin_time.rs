// Copyright 2017 PingCAP, Inc.
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

use super::{EvalContext, Result, ScalarFunc};
use crate::coprocessor::codec::error::Error;
use crate::coprocessor::codec::mysql::time::extension::DateTimeExtension;
use crate::coprocessor::codec::mysql::time::weekmode::WeekMode;
use crate::coprocessor::codec::mysql::{Duration as MyDuration, Time, TimeType};
use crate::coprocessor::codec::Datum;
use chrono::offset::TimeZone;
use chrono::Datelike;
use std::borrow::Cow;

fn handle_incorrect_datetime_error(ctx: &mut EvalContext, t: Cow<'_, Time>) -> Result<()> {
    Error::handle_invalid_time_error(ctx, Error::incorrect_datetime_value(&format!("{}", t)))
}

impl ScalarFunc {
    #[inline]
    pub fn date_format<'a, 'b: 'a>(
        &'b self,
        ctx: &mut EvalContext,
        row: &'a [Datum],
    ) -> Result<Option<Cow<'a, [u8]>>> {
        let t: Cow<'a, Time> = try_opt!(self.children[0].eval_time(ctx, row));
        if t.invalid_zero() {
            return handle_incorrect_datetime_error(ctx, t).map(|_| None);
        }
        let format_mask: Cow<'a, str> = try_opt!(self.children[1].eval_string_and_decode(ctx, row));
        let t = t.date_format(format_mask.into_owned())?;
        Ok(Some(Cow::Owned(t.into_bytes())))
    }

    #[inline]
    pub fn date<'a, 'b: 'a>(
        &'b self,
        ctx: &mut EvalContext,
        row: &'a [Datum],
    ) -> Result<Option<Cow<'a, Time>>> {
        let mut t: Cow<'a, Time> = try_opt!(self.children[0].eval_time(ctx, row));
        if t.is_zero() {
            return handle_incorrect_datetime_error(ctx, t).map(|_| None);
        }
        let mut res = t.to_mut().clone();
        res.set_time_type(TimeType::Date).unwrap();
        Ok(Some(Cow::Owned(res)))
    }

    #[inline]
    pub fn hour(&self, ctx: &mut EvalContext, row: &[Datum]) -> Result<Option<i64>> {
        let dur: Cow<'_, MyDuration> = try_opt!(self.children[0].eval_duration(ctx, row));
        Ok(Some(dur.hours() as i64))
    }

    #[inline]
    pub fn minute(&self, ctx: &mut EvalContext, row: &[Datum]) -> Result<Option<i64>> {
        let dur: Cow<'_, MyDuration> = try_opt!(self.children[0].eval_duration(ctx, row));
        Ok(Some(dur.minutes() as i64))
    }

    #[inline]
    pub fn second(&self, ctx: &mut EvalContext, row: &[Datum]) -> Result<Option<i64>> {
        let dur: Cow<'_, MyDuration> = try_opt!(self.children[0].eval_duration(ctx, row));
        Ok(Some(dur.secs() as i64))
    }

    #[inline]
    pub fn micro_second(&self, ctx: &mut EvalContext, row: &[Datum]) -> Result<Option<i64>> {
        let dur: Cow<'_, MyDuration> = try_opt!(self.children[0].eval_duration(ctx, row));
        Ok(Some(i64::from(dur.micro_secs())))
    }

    #[inline]
    pub fn month<'a, 'b: 'a>(
        &'b self,
        ctx: &mut EvalContext,
        row: &'a [Datum],
    ) -> Result<Option<i64>> {
        let t: Cow<'a, Time> = try_opt!(self.children[0].eval_time(ctx, row));
        if t.is_zero() {
            if ctx.cfg.mode_no_zero_date_mode() {
                return handle_incorrect_datetime_error(ctx, t).map(|_| None);
            }
            return Ok(Some(0));
        }
        Ok(Some(i64::from(t.get_time().month())))
    }

    #[inline]
    pub fn month_name<'a, 'b: 'a>(
        &'b self,
        ctx: &mut EvalContext,
        row: &'a [Datum],
    ) -> Result<Option<Cow<'a, [u8]>>> {
        let t: Cow<'a, Time> = try_opt!(self.children[0].eval_time(ctx, row));
        let month = t.get_time().month() as usize;
        if t.is_zero() && ctx.cfg.mode_no_zero_date_mode() {
            return handle_incorrect_datetime_error(ctx, t).map(|_| None);
        } else if month == 0 || t.is_zero() {
            return Ok(None);
        }
        use crate::coprocessor::codec::mysql::time::MONTH_NAMES;
        Ok(Some(Cow::Owned(
            MONTH_NAMES[month - 1].to_string().into_bytes(),
        )))
    }

    #[inline]
    pub fn day_name<'a, 'b: 'a>(
        &'b self,
        ctx: &mut EvalContext,
        row: &'a [Datum],
    ) -> Result<Option<Cow<'a, [u8]>>> {
        let t: Cow<'a, Time> = try_opt!(self.children[0].eval_time(ctx, row));
        if t.is_zero() {
            return handle_incorrect_datetime_error(ctx, t).map(|_| None);
        }
        use crate::coprocessor::codec::mysql::time::WeekdayExtension;
        let weekday = t.get_time().weekday();
        Ok(Some(Cow::Owned(weekday.name().to_string().into_bytes())))
    }

    #[inline]
    pub fn day_of_month(&self, ctx: &mut EvalContext, row: &[Datum]) -> Result<Option<i64>> {
        let t: Cow<'_, Time> = try_opt!(self.children[0].eval_time(ctx, row));
        if t.is_zero() {
            if ctx.cfg.mode_no_zero_date_mode() {
                return handle_incorrect_datetime_error(ctx, t).map(|_| None);
            }
            return Ok(Some(0));
        }
        let day = t.get_time().day();
        Ok(Some(i64::from(day)))
    }

    #[inline]
    pub fn day_of_week(&self, ctx: &mut EvalContext, row: &[Datum]) -> Result<Option<i64>> {
        let t: Cow<'_, Time> = try_opt!(self.children[0].eval_time(ctx, row));
        if t.is_zero() {
            return handle_incorrect_datetime_error(ctx, t).map(|_| None);
        }
        let day = t.get_time().weekday().number_from_sunday();
        Ok(Some(i64::from(day)))
    }

    #[inline]
    pub fn day_of_year(&self, ctx: &mut EvalContext, row: &[Datum]) -> Result<Option<i64>> {
        let t: Cow<'_, Time> = try_opt!(self.children[0].eval_time(ctx, row));
        if t.is_zero() {
            return handle_incorrect_datetime_error(ctx, t).map(|_| None);
        }
        let day = t.get_time().days();
        Ok(Some(i64::from(day)))
    }

    #[inline]
    pub fn year(&self, ctx: &mut EvalContext, row: &[Datum]) -> Result<Option<i64>> {
        let t: Cow<'_, Time> = try_opt!(self.children[0].eval_time(ctx, row));
        if t.is_zero() {
            if ctx.cfg.mode_no_zero_date_mode() {
                return handle_incorrect_datetime_error(ctx, t).map(|_| None);
            }
            return Ok(Some(0));
        }
        Ok(Some(i64::from(t.get_time().year())))
    }

    #[inline]
    pub fn last_day<'a, 'b: 'a>(
        &'b self,
        ctx: &mut EvalContext,
        row: &'a [Datum],
    ) -> Result<Option<Cow<'a, Time>>> {
        let mut t: Cow<'a, Time> = try_opt!(self.children[0].eval_time(ctx, row));
        if t.is_zero() {
            return handle_incorrect_datetime_error(ctx, t).map(|_| None);
        }
        let time = t.get_time();
        let mut res = t.to_mut().clone();
        res.set_time(
            time.timezone()
                .ymd_opt(time.year(), time.month(), t.last_day_of_month())
                .and_hms_opt(0, 0, 0)
                .unwrap(),
        );
        Ok(Some(Cow::Owned(res)))
    }

    #[inline]
    pub fn week_with_mode(&self, ctx: &mut EvalContext, row: &[Datum]) -> Result<Option<i64>> {
        let t: Cow<'_, Time> = try_opt!(self.children[0].eval_time(ctx, row));
        if t.is_zero() {
            return handle_incorrect_datetime_error(ctx, t).map(|_| None);
        }
        let mode: i64 = try_opt!(self.children[1].eval_int(ctx, row));
        let week = t.get_time().week(WeekMode::from_bits_truncate(mode as u32));
        Ok(Some(i64::from(week)))
    }

    #[inline]
    pub fn week_without_mode(&self, ctx: &mut EvalContext, row: &[Datum]) -> Result<Option<i64>> {
        let t: Cow<'_, Time> = try_opt!(self.children[0].eval_time(ctx, row));
        if t.is_zero() {
            return handle_incorrect_datetime_error(ctx, t).map(|_| None);
        }
        let week = t.get_time().week(WeekMode::from_bits_truncate(0u32));
        Ok(Some(i64::from(week)))
    }

    #[inline]
    pub fn week_day(&self, ctx: &mut EvalContext, row: &[Datum]) -> Result<Option<i64>> {
        let t: Cow<'_, Time> = try_opt!(self.children[0].eval_time(ctx, row));
        if t.is_zero() {
            return handle_incorrect_datetime_error(ctx, t).map(|_| None);
        }
        let day = t.get_time().weekday().num_days_from_monday();
        Ok(Some(i64::from(day)))
    }

    #[inline]
    pub fn week_of_year(&self, ctx: &mut EvalContext, row: &[Datum]) -> Result<Option<i64>> {
        let t: Cow<'_, Time> = try_opt!(self.children[0].eval_time(ctx, row));
        if t.is_zero() {
            return handle_incorrect_datetime_error(ctx, t).map(|_| None);
        }
        // is equivalent to week_with_mode() with mode 3.
        let week = t.get_time().iso_week().week();
        Ok(Some(i64::from(week)))
    }

    #[inline]
    pub fn year_week_with_mode(&self, ctx: &mut EvalContext, row: &[Datum]) -> Result<Option<i64>> {
        let t: Cow<'_, Time> = try_opt!(self.children[0].eval_time(ctx, row));
        if t.is_zero() {
            return handle_incorrect_datetime_error(ctx, t).map(|_| None);
        }
        let mode = match self.children[1].eval_int(ctx, row) {
            Err(e) => return Err(e),
            Ok(None) => 0,
            Ok(Some(num)) => num,
        };
        let (year, week) = t
            .get_time()
            .year_week(WeekMode::from_bits_truncate(mode as u32));
        let mut result = i64::from(week + year * 100);
        if result < 0 {
            result = i64::from(u32::max_value());
        }
        Ok(Some(result))
    }

    #[inline]
    pub fn year_week_without_mode(
        &self,
        ctx: &mut EvalContext,
        row: &[Datum],
    ) -> Result<Option<i64>> {
        let t: Cow<'_, Time> = try_opt!(self.children[0].eval_time(ctx, row));
        if t.is_zero() {
            return handle_incorrect_datetime_error(ctx, t).map(|_| None);
        }
        let (year, week) = t.get_time().year_week(WeekMode::from_bits_truncate(0u32));
        let mut result = i64::from(week + year * 100);
        if result < 0 {
            result = i64::from(u32::max_value());
        }
        Ok(Some(result))
    }

    #[inline]
    pub fn add_datetime_and_duration<'a, 'b: 'a>(
        &'b self,
        ctx: &mut EvalContext,
        row: &'a [Datum],
    ) -> Result<Option<Cow<'a, Time>>> {
        let arg0: Cow<'a, Time> = try_opt!(self.children[0].eval_time(ctx, row));
        let arg1: Cow<'a, MyDuration> = try_opt!(self.children[1].eval_duration(ctx, row));
        let overflow = Error::overflow("TIME", &format!("({} + {})", &arg0, &arg1));
        let mut res = match arg0.into_owned().checked_add(&arg1) {
            Some(res) => res,
            None => return Err(overflow),
        };
        res.set_time_type(TimeType::DateTime)?;
        Ok(Some(Cow::Owned(res)))
    }

    #[inline]
    pub fn add_datetime_and_string<'a, 'b: 'a>(
        &'b self,
        ctx: &mut EvalContext,
        row: &'a [Datum],
    ) -> Result<Option<Cow<'a, Time>>> {
        let arg0: Cow<'a, Time> = try_opt!(self.children[0].eval_time(ctx, row));
        let arg1: Cow<'a, [u8]> = try_opt!(self.children[1].eval_string(ctx, row));
        let s = ::std::str::from_utf8(&arg1)?;
        let arg1 = MyDuration::parse(&arg1, Time::parse_fsp(s))?;
        let overflow = Error::overflow("TIME", &format!("({} + {})", &arg0, &arg1));
        let mut res = match arg0.into_owned().checked_add(&arg1) {
            Some(res) => res,
            None => return Err(overflow),
        };
        res.set_time_type(TimeType::DateTime)?;
        Ok(Some(Cow::Owned(res)))
    }

    #[inline]
    pub fn add_time_datetime_null<'a>(
        &self,
        _ctx: &mut EvalContext,
        _row: &[Datum],
    ) -> Result<Option<Cow<'a, Time>>> {
        Ok(None)
    }

    #[inline]
    pub fn to_days(&self, ctx: &mut EvalContext, row: &[Datum]) -> Result<Option<i64>> {
        let t: Cow<'_, Time> = try_opt!(self.children[0].eval_time(ctx, row));
        if t.is_zero() {
            return handle_incorrect_datetime_error(ctx, t).map(|_| None);
        }
        let time = t.get_time();
        Ok(Some(i64::from(time.day_number())))
    }

    pub fn date_diff(&self, ctx: &mut EvalContext, row: &[Datum]) -> Result<Option<i64>> {
        let lhs: Cow<'_, Time> = try_opt!(self.children[0].eval_time(ctx, row));
        if lhs.invalid_zero() {
            return handle_incorrect_datetime_error(ctx, lhs).map(|_| None);
        }
        let rhs: Cow<'_, Time> = try_opt!(self.children[1].eval_time(ctx, row));
        if rhs.invalid_zero() {
            return handle_incorrect_datetime_error(ctx, rhs).map(|_| None);
        }
        let days_diff = lhs
            .get_time()
            .date()
            .signed_duration_since(rhs.get_time().date())
            .num_days();
        Ok(Some(days_diff))
    }

    #[inline]
    pub fn add_duration_and_duration<'a, 'b: 'a>(
        &'b self,
        ctx: &mut EvalContext,
        row: &'a [Datum],
    ) -> Result<Option<Cow<'a, MyDuration>>> {
        let arg0: Cow<'a, MyDuration> = try_opt!(self.children[0].eval_duration(ctx, row));
        let arg1: Cow<'a, MyDuration> = try_opt!(self.children[1].eval_duration(ctx, row));
        let overflow = Error::overflow("DURATION", &format!("({} + {})", &arg0, &arg1));
        let res = match arg0.into_owned().checked_add(&arg1) {
            Some(res) => res,
            None => return Err(overflow),
        };
        Ok(Some(Cow::Owned(res)))
    }

    #[inline]
    pub fn add_duration_and_string<'a, 'b: 'a>(
        &'b self,
        ctx: &mut EvalContext,
        row: &'a [Datum],
    ) -> Result<Option<Cow<'a, MyDuration>>> {
        let arg0: Cow<'a, MyDuration> = try_opt!(self.children[0].eval_duration(ctx, row));
        let arg1: Cow<'a, [u8]> = try_opt!(self.children[1].eval_string(ctx, row));
        let s = ::std::str::from_utf8(&arg1)?;
        let arg1 = MyDuration::parse(&arg1, Time::parse_fsp(s))?;
        let overflow = Error::overflow("DURATION", &format!("({} + {})", &arg0, &arg1));
        let res = match arg0.into_owned().checked_add(&arg1) {
            Some(res) => res,
            None => return Err(overflow),
        };
        Ok(Some(Cow::Owned(res)))
    }

    #[inline]
    pub fn add_time_duration_null<'a>(
        &self,
        _ctx: &mut EvalContext,
        _row: &[Datum],
    ) -> Result<Option<Cow<'a, MyDuration>>> {
        Ok(None)
    }

    #[inline]
    pub fn sub_datetime_and_duration<'a, 'b: 'a>(
        &'b self,
        ctx: &mut EvalContext,
        row: &'a [Datum],
    ) -> Result<Option<Cow<'a, Time>>> {
        let arg0: Cow<'a, Time> = try_opt!(self.children[0].eval_time(ctx, row));
        let arg1: Cow<'a, MyDuration> = try_opt!(self.children[1].eval_duration(ctx, row));
        let overflow = Error::overflow("TIME", &format!("({} - {})", &arg0, &arg1));
        let mut res = match arg0.into_owned().checked_sub(&arg1) {
            Some(res) => res,
            None => return Err(overflow),
        };
        res.set_time_type(TimeType::DateTime)?;
        Ok(Some(Cow::Owned(res)))
    }

    #[inline]
    pub fn sub_datetime_and_string<'a, 'b: 'a>(
        &'b self,
        ctx: &mut EvalContext,
        row: &'a [Datum],
    ) -> Result<Option<Cow<'a, Time>>> {
        let arg0: Cow<'a, Time> = try_opt!(self.children[0].eval_time(ctx, row));
        let arg1: Cow<'a, [u8]> = try_opt!(self.children[1].eval_string(ctx, row));
        let s = ::std::str::from_utf8(&arg1)?;
        let arg1 = MyDuration::parse(&arg1, Time::parse_fsp(s))?;
        let overflow = Error::overflow("TIME", &format!("({} - {})", &arg0, &arg1));
        let mut res = match arg0.into_owned().checked_sub(&arg1) {
            Some(res) => res,
            None => return Err(overflow),
        };
        res.set_time_type(TimeType::DateTime)?;
        Ok(Some(Cow::Owned(res)))
    }

    #[inline]
    pub fn sub_time_datetime_null<'a>(
        &self,
        _ctx: &mut EvalContext,
        _row: &[Datum],
    ) -> Result<Option<Cow<'a, Time>>> {
        Ok(None)
    }

    #[inline]
    pub fn sub_duration_and_duration<'a, 'b: 'a>(
        &'b self,
        ctx: &mut EvalContext,
        row: &'a [Datum],
    ) -> Result<Option<Cow<'a, MyDuration>>> {
        let d0: Cow<'a, MyDuration> = try_opt!(self.children[0].eval_duration(ctx, row));
        let d1: Cow<'a, MyDuration> = try_opt!(self.children[1].eval_duration(ctx, row));
        let diff = match d0.to_nanos().checked_sub(d1.to_nanos()) {
            Some(result) => result,
            None => return Err(Error::overflow("DURATION", &format!("({} - {})", &d0, &d1))),
        };
        let res = MyDuration::from_nanos(diff, d0.fsp().max(d1.fsp()) as i8)?;
        Ok(Some(Cow::Owned(res)))
    }
}

#[cfg(test)]
mod tests {
    use crate::coprocessor::codec::mysql::{Duration, Time};
    use crate::coprocessor::codec::Datum;
    use crate::coprocessor::dag::expr::tests::{datum_expr, scalar_func_expr};
    use crate::coprocessor::dag::expr::*;
    use crate::coprocessor::dag::expr::{EvalContext, Expression};
    use std::sync::Arc;
    use tipb::expression::{Expr, ScalarFuncSig};

    fn expr_build(ctx: &mut EvalContext, sig: ScalarFuncSig, children: &[Expr]) -> Result<Datum> {
        let f = scalar_func_expr(sig, children);
        let op = Expression::build(ctx, f).unwrap();
        op.eval(ctx, &[])
    }

    fn test_ok_case_zero_arg(ctx: &mut EvalContext, sig: ScalarFuncSig, exp: Datum) {
        match expr_build(ctx, sig, &[]) {
            Ok(got) => assert_eq!(got, exp),
            Err(_) => panic!("eval failed"),
        }
    }

    fn test_ok_case_one_arg(ctx: &mut EvalContext, sig: ScalarFuncSig, arg: Datum, exp: Datum) {
        let children = &[datum_expr(arg)];
        match expr_build(ctx, sig, children) {
            Ok(got) => assert_eq!(got, exp),
            Err(_) => panic!("eval failed"),
        }
    }

    fn test_err_case_one_arg(ctx: &mut EvalContext, sig: ScalarFuncSig, arg: Datum) {
        let children = &[datum_expr(arg)];
        if let Ok(got) = expr_build(ctx, sig, children) {
            assert_eq!(got, Datum::Null);
        }
    }

    fn test_ok_case_two_arg(
        ctx: &mut EvalContext,
        sig: ScalarFuncSig,
        arg1: Datum,
        arg2: Datum,
        exp: Datum,
    ) {
        let children = &[datum_expr(arg1), datum_expr(arg2)];
        match expr_build(ctx, sig, children) {
            Ok(got) => assert_eq!(got, exp),
            Err(_) => panic!("eval failed"),
        }
    }

    fn test_err_case_two_arg(ctx: &mut EvalContext, sig: ScalarFuncSig, arg1: Datum, arg2: Datum) {
        let children = &[datum_expr(arg1), datum_expr(arg2)];
        if let Ok(got) = expr_build(ctx, sig, children) {
            assert_eq!(got, Datum::Null);
        }
    }

    #[test]
    fn test_date_format() {
        let cases = vec![
            (
                "2010-01-07 23:12:34.12345",
                "%b %M %m %c %D %d %e %j %k %h %i %p %r %T %s %f %U %u
                %V %v %a %W %w %X %x %Y %y %%",
                "Jan January 01 1 7th 07 7 007 23 11 12 PM 11:12:34 PM 23:12:34 34 123450 01 01
                01 01 Thu Thursday 4 2010 2010 2010 10 %",
            ),
            (
                "2012-12-21 23:12:34.123456",
                "%b %M %m %c %D %d %e %j %k %h %i %p %r %T %s %f %U
                %u %V %v %a %W %w %X %x %Y %y %%",
                "Dec December 12 12 21st 21 21 356 23 11 12 PM 11:12:34 PM 23:12:34 34 123456 51
                51 51 51 Fri Friday 5 2012 2012 2012 12 %",
            ),
            (
                "0000-01-01 00:00:00.123456",
                // Functions week() and yearweek() don't support multi mode,
                // so the result of "%U %u %V %Y" is different from MySQL.
                "%b %M %m %c %D %d %e %j %k %h %i %p %r %T %s %f %v
                %x %Y %y %%",
                "Jan January 01 1 1st 01 1 001 0 12 00 AM 12:00:00 AM 00:00:00 00 123456 52
                4294967295 0000 00 %",
            ),
            (
                "2016-09-3 00:59:59.123456",
                "abc%b %M %m %c %D %d %e %j %k %h %i %p %r %T %s %f %U
                %u %V %v %a %W %w %X %x %Y %y!123 %%xyz %z",
                "abcSep September 09 9 3rd 03 3 247 0 12 59 AM 12:59:59 AM 00:59:59 59 123456 35
                35 35 35 Sat Saturday 6 2016 2016 2016 16!123 %xyz z",
            ),
            (
                "2012-10-01 00:00:00",
                "%b %M %m %c %D %d %e %j %k %H %i %p %r %T %s %f %v
                %x %Y %y %%",
                "Oct October 10 10 1st 01 1 275 0 00 00 AM 12:00:00 AM 00:00:00 00 000000 40
                2012 2012 12 %",
            ),
        ];
        let mut ctx = EvalContext::default();
        for (arg1, arg2, exp) in cases {
            test_ok_case_two_arg(
                &mut ctx,
                ScalarFuncSig::DateFormatSig,
                Datum::Time(Time::parse_utc_datetime(arg1, 6).unwrap()),
                Datum::Bytes(arg2.to_string().into_bytes()),
                Datum::Bytes(exp.to_string().into_bytes()),
            );
        }
        // test NULL case
        test_err_case_two_arg(
            &mut ctx,
            ScalarFuncSig::DateFormatSig,
            Datum::Null,
            Datum::Null,
        );
        // test zero case
        let mut cfg = EvalConfig::new();
        cfg.set_by_flags(FLAG_IN_UPDATE_OR_DELETE_STMT)
            .set_sql_mode(MODE_ERROR_FOR_DIVISION_BY_ZERO)
            .set_strict_sql_mode(true);
        ctx = EvalContext::new(Arc::new(cfg));
        test_err_case_two_arg(
            &mut ctx,
            ScalarFuncSig::DateFormatSig,
            Datum::Null,
            Datum::Null,
        );
    }

    #[test]
    fn test_date() {
        let cases = vec![
            ("2011-11-11", "2011-11-11"),
            ("2011-11-11 10:10:10", "2011-11-11"),
        ];
        let mut ctx = EvalContext::default();
        for (arg, exp) in cases {
            test_ok_case_one_arg(
                &mut ctx,
                ScalarFuncSig::Date,
                Datum::Time(Time::parse_utc_datetime(arg, 6).unwrap()),
                Datum::Time(Time::parse_utc_datetime(exp, 6).unwrap()),
            );
        }
        // test NULL case
        test_err_case_one_arg(&mut ctx, ScalarFuncSig::Date, Datum::Null);
        // test zero case
        let mut cfg = EvalConfig::new();
        cfg.set_by_flags(FLAG_IN_UPDATE_OR_DELETE_STMT)
            .set_sql_mode(MODE_ERROR_FOR_DIVISION_BY_ZERO)
            .set_strict_sql_mode(true);
        ctx = EvalContext::new(Arc::new(cfg));
        test_err_case_one_arg(
            &mut ctx,
            ScalarFuncSig::Date,
            Datum::Time(Time::parse_utc_datetime("0000-00-00 00:00:00", 6).unwrap()),
        );
    }

    #[test]
    fn test_hour_min_sec_micro_sec() {
        // test hour, minute, second, micro_second
        let cases: Vec<(&str, i8, i64, i64, i64, i64)> = vec![
            ("31 11:30:45", 0, 31 * 24 + 11, 30, 45, 0),
            ("11:30:45.123345", 3, 11, 30, 45, 123000),
            ("11:30:45.123345", 5, 11, 30, 45, 123350),
            ("11:30:45.123345", 6, 11, 30, 45, 123345),
            ("11:30:45.1233456", 6, 11, 30, 45, 123346),
            ("11:30:45.000010", 6, 11, 30, 45, 10),
            ("11:30:45.00010", 5, 11, 30, 45, 100),
            ("-11:30:45.9233456", 0, 11, 30, 46, 0),
            ("-11:30:45.9233456", 1, 11, 30, 45, 900000),
            ("272:59:59.94", 2, 272, 59, 59, 940000),
            ("272:59:59.99", 1, 273, 0, 0, 0),
            ("272:59:59.99", 0, 273, 0, 0, 0),
        ];
        let mut ctx = EvalContext::default();
        for (arg, fsp, h, m, s, ms) in cases {
            let d = Datum::Dur(Duration::parse(arg.as_bytes(), fsp).unwrap());
            test_ok_case_one_arg(&mut ctx, ScalarFuncSig::Hour, d.clone(), Datum::I64(h));
            test_ok_case_one_arg(&mut ctx, ScalarFuncSig::Minute, d.clone(), Datum::I64(m));
            test_ok_case_one_arg(&mut ctx, ScalarFuncSig::Second, d.clone(), Datum::I64(s));
            test_ok_case_one_arg(&mut ctx, ScalarFuncSig::MicroSecond, d, Datum::I64(ms));
        }
        // test NULL case
        test_err_case_one_arg(&mut ctx, ScalarFuncSig::Hour, Datum::Null);
        test_err_case_one_arg(&mut ctx, ScalarFuncSig::Minute, Datum::Null);
        test_err_case_one_arg(&mut ctx, ScalarFuncSig::Second, Datum::Null);
        test_err_case_one_arg(&mut ctx, ScalarFuncSig::MicroSecond, Datum::Null);
        // test zero case
        let d = Datum::Dur(Duration::parse(b"0 00:00:00.0", 0).unwrap());
        test_ok_case_one_arg(&mut ctx, ScalarFuncSig::Hour, d.clone(), Datum::I64(0));
        test_ok_case_one_arg(&mut ctx, ScalarFuncSig::Minute, d.clone(), Datum::I64(0));
        test_ok_case_one_arg(&mut ctx, ScalarFuncSig::Second, d.clone(), Datum::I64(0));
        test_ok_case_one_arg(&mut ctx, ScalarFuncSig::MicroSecond, d, Datum::I64(0));
    }

    #[test]
    fn test_month() {
        let cases = vec![
            ("0000-00-00 00:00:00", 0i64),
            ("2018-01-01 01:01:01", 1i64),
            ("2018-02-01 01:01:01", 2i64),
            ("2018-03-01 01:01:01", 3i64),
            ("2018-04-01 01:01:01", 4i64),
            ("2018-05-01 01:01:01", 5i64),
            ("2018-06-01 01:01:01", 6i64),
            ("2018-07-01 01:01:01", 7i64),
            ("2018-08-01 01:01:01", 8i64),
            ("2018-09-01 01:01:01", 9i64),
            ("2018-10-01 01:01:01", 10i64),
            ("2018-11-01 01:01:01", 11i64),
            ("2018-12-01 01:01:01", 12i64),
        ];
        let mut ctx = EvalContext::default();
        for (arg, exp) in cases {
            test_ok_case_one_arg(
                &mut ctx,
                ScalarFuncSig::Month,
                Datum::Time(Time::parse_utc_datetime(arg, 6).unwrap()),
                Datum::I64(exp),
            );
        }
        // test NULL case
        test_err_case_one_arg(&mut ctx, ScalarFuncSig::Month, Datum::Null);
        // test zero case
        let mut cfg = EvalConfig::new();
        cfg.set_by_flags(FLAG_IN_UPDATE_OR_DELETE_STMT)
            .set_sql_mode(MODE_ERROR_FOR_DIVISION_BY_ZERO)
            .set_strict_sql_mode(true);
        ctx = EvalContext::new(Arc::new(cfg));
        test_err_case_one_arg(
            &mut ctx,
            ScalarFuncSig::Month,
            Datum::Time(Time::parse_utc_datetime("0000-00-00 00:00:00", 6).unwrap()),
        );
    }

    #[test]
    fn test_month_name() {
        let cases = vec![
            (
                Datum::Time(Time::parse_utc_datetime("0000-00-00 00:00:00.000000", 6).unwrap()),
                Datum::Null,
            ),
            (
                Datum::Time(Time::parse_utc_datetime("2018-01-01 00:00:00.000000", 6).unwrap()),
                Datum::Bytes(b"January".to_vec()),
            ),
            (
                Datum::Time(Time::parse_utc_datetime("2018-02-01 00:00:00.000000", 6).unwrap()),
                Datum::Bytes(b"February".to_vec()),
            ),
            (
                Datum::Time(Time::parse_utc_datetime("2018-03-01 00:00:00.000000", 6).unwrap()),
                Datum::Bytes(b"March".to_vec()),
            ),
            (
                Datum::Time(Time::parse_utc_datetime("2018-04-01 00:00:00.000000", 6).unwrap()),
                Datum::Bytes(b"April".to_vec()),
            ),
            (
                Datum::Time(Time::parse_utc_datetime("2018-05-01 00:00:00.000000", 6).unwrap()),
                Datum::Bytes(b"May".to_vec()),
            ),
            (
                Datum::Time(Time::parse_utc_datetime("2018-06-01 00:00:00.000000", 6).unwrap()),
                Datum::Bytes(b"June".to_vec()),
            ),
            (
                Datum::Time(Time::parse_utc_datetime("2018-07-01 00:00:00.000000", 6).unwrap()),
                Datum::Bytes(b"July".to_vec()),
            ),
            (
                Datum::Time(Time::parse_utc_datetime("2018-08-01 00:00:00.000000", 6).unwrap()),
                Datum::Bytes(b"August".to_vec()),
            ),
            (
                Datum::Time(Time::parse_utc_datetime("2018-09-01 00:00:00.000000", 6).unwrap()),
                Datum::Bytes(b"September".to_vec()),
            ),
            (
                Datum::Time(Time::parse_utc_datetime("2018-10-01 00:00:00.000000", 6).unwrap()),
                Datum::Bytes(b"October".to_vec()),
            ),
            (
                Datum::Time(Time::parse_utc_datetime("2018-11-01 00:00:00.000000", 6).unwrap()),
                Datum::Bytes(b"November".to_vec()),
            ),
            (
                Datum::Time(Time::parse_utc_datetime("2018-12-01 00:00:00.000000", 6).unwrap()),
                Datum::Bytes(b"December".to_vec()),
            ),
        ];
        let mut ctx = EvalContext::default();
        for (arg, exp) in cases {
            test_ok_case_one_arg(&mut ctx, ScalarFuncSig::MonthName, arg, exp);
        }
        // test NULL case
        test_err_case_one_arg(&mut ctx, ScalarFuncSig::MonthName, Datum::Null);
        //  test zero case
        let mut cfg = EvalConfig::new();
        cfg.set_by_flags(FLAG_IN_UPDATE_OR_DELETE_STMT)
            .set_sql_mode(MODE_NO_ZERO_DATE_MODE)
            .set_strict_sql_mode(true);
        ctx = EvalContext::new(Arc::new(cfg));
        test_err_case_one_arg(
            &mut ctx,
            ScalarFuncSig::MonthName,
            Datum::Time(Time::parse_utc_datetime("0000-00-00 00:00:00", 6).unwrap()),
        );
    }

    #[test]
    fn test_day_name() {
        let cases = vec![
            ("2018-11-11 00:00:00.000000", "Sunday"),
            ("2018-11-12 00:00:00.000000", "Monday"),
            ("2018-11-13 00:00:00.000000", "Tuesday"),
            ("2018-11-14 00:00:00.000000", "Wednesday"),
            ("2018-11-15 00:00:00.000000", "Thursday"),
            ("2018-11-16 00:00:00.000000", "Friday"),
            ("2018-11-17 00:00:00.000000", "Saturday"),
            ("2018-11-18 00:00:00.000000", "Sunday"),
        ];
        let mut ctx = EvalContext::default();
        for (arg, exp) in cases {
            test_ok_case_one_arg(
                &mut ctx,
                ScalarFuncSig::DayName,
                Datum::Time(Time::parse_utc_datetime(arg, 6).unwrap()),
                Datum::Bytes(exp.as_bytes().to_vec()),
            );
        }
        // test NULL case
        test_err_case_one_arg(&mut ctx, ScalarFuncSig::DayName, Datum::Null);
        //  test zero case
        test_err_case_one_arg(
            &mut ctx,
            ScalarFuncSig::DayName,
            Datum::Time(Time::parse_utc_datetime("0000-00-00 00:00:00", 6).unwrap()),
        );
    }

    #[test]
    fn test_day_of_month() {
        let cases = vec![
            ("0000-00-00 00:00:00.000000", 0),
            ("2018-02-01 00:00:00.000000", 1),
            ("2018-02-15 00:00:00.000000", 15),
            ("2018-02-28 00:00:00.000000", 28),
            ("2016-02-29 00:00:00.000000", 29),
        ];
        let mut ctx = EvalContext::default();
        for (arg, exp) in cases {
            test_ok_case_one_arg(
                &mut ctx,
                ScalarFuncSig::DayOfMonth,
                Datum::Time(Time::parse_utc_datetime(arg, 6).unwrap()),
                Datum::I64(exp),
            );
        }
        // test NULL case
        test_err_case_one_arg(&mut ctx, ScalarFuncSig::DayOfMonth, Datum::Null);
        //  test zero case
        let mut cfg = EvalConfig::new();
        cfg.set_by_flags(FLAG_IN_UPDATE_OR_DELETE_STMT)
            .set_sql_mode(MODE_NO_ZERO_DATE_MODE)
            .set_strict_sql_mode(true);
        ctx = EvalContext::new(Arc::new(cfg));
        test_err_case_one_arg(
            &mut ctx,
            ScalarFuncSig::DayOfMonth,
            Datum::Time(Time::parse_utc_datetime("0000-00-00 00:00:00", 6).unwrap()),
        );
    }

    #[test]
    fn test_day_of_week() {
        let cases = vec![
            ("2018-11-11 00:00:00.000000", 1),
            ("2018-11-12 00:00:00.000000", 2),
            ("2018-11-13 00:00:00.000000", 3),
            ("2018-11-14 00:00:00.000000", 4),
            ("2018-11-15 00:00:00.000000", 5),
            ("2018-11-16 00:00:00.000000", 6),
            ("2018-11-17 00:00:00.000000", 7),
            ("2018-11-18 00:00:00.000000", 1),
        ];
        let mut ctx = EvalContext::default();
        for (arg, exp) in cases {
            test_ok_case_one_arg(
                &mut ctx,
                ScalarFuncSig::DayOfWeek,
                Datum::Time(Time::parse_utc_datetime(arg, 6).unwrap()),
                Datum::I64(exp),
            );
        }
        // test NULL case
        test_err_case_one_arg(&mut ctx, ScalarFuncSig::DayOfWeek, Datum::Null);
        //  test zero case
        test_err_case_one_arg(
            &mut ctx,
            ScalarFuncSig::DayOfWeek,
            Datum::Time(Time::parse_utc_datetime("0000-00-00 00:00:00", 6).unwrap()),
        );
    }

    #[test]
    fn test_day_of_year() {
        let cases = vec![
            ("2018-11-11 00:00:00.000000", 315),
            ("2018-11-12 00:00:00.000000", 316),
            ("2018-11-30 00:00:00.000000", 334),
            ("2018-12-31 00:00:00.000000", 365),
            ("2016-12-31 00:00:00.000000", 366),
        ];
        let mut ctx = EvalContext::default();
        for (arg, exp) in cases {
            test_ok_case_one_arg(
                &mut ctx,
                ScalarFuncSig::DayOfYear,
                Datum::Time(Time::parse_utc_datetime(arg, 6).unwrap()),
                Datum::I64(exp),
            );
        }
        // test NULL case
        test_err_case_one_arg(&mut ctx, ScalarFuncSig::DayOfYear, Datum::Null);
        //  test zero case
        test_err_case_one_arg(
            &mut ctx,
            ScalarFuncSig::DayOfYear,
            Datum::Time(Time::parse_utc_datetime("0000-00-00 00:00:00", 6).unwrap()),
        );
    }

    #[test]
    fn test_last_day() {
        let cases = vec![
            ("2011-11-11", "2011-11-30"),
            ("2008-02-10", "2008-02-29"),
            ("2000-02-11", "2000-02-29"),
            ("2100-02-11", "2100-02-28"),
            ("2011-11-11", "2011-11-30"),
            ("2011-11-11 10:10:10", "2011-11-30 00:00:00"),
        ];
        let mut ctx = EvalContext::default();
        for (arg, exp) in cases {
            test_ok_case_one_arg(
                &mut ctx,
                ScalarFuncSig::LastDay,
                Datum::Time(Time::parse_utc_datetime(arg, 6).unwrap()),
                Datum::Time(Time::parse_utc_datetime(exp, 6).unwrap()),
            );
        }
        // test NULL case
        test_err_case_one_arg(&mut ctx, ScalarFuncSig::LastDay, Datum::Null);
        // test zero case
        test_err_case_one_arg(
            &mut ctx,
            ScalarFuncSig::LastDay,
            Datum::Time(Time::parse_utc_datetime("0000-00-00 00:00:00", 6).unwrap()),
        );
    }

    #[test]
    fn test_year() {
        let cases = vec![
            ("0000-00-00 00:00:00", 0i64),
            ("1-01-01 01:01:01", 1i64),
            ("2018-01-01 01:01:01", 2018i64),
            ("2019-01-01 01:01:01", 2019i64),
            ("2020-01-01 01:01:01", 2020i64),
            ("2021-01-01 01:01:01", 2021i64),
            ("2022-01-01 01:01:01", 2022i64),
            ("2023-01-01 01:01:01", 2023i64),
            ("2024-01-01 01:01:01", 2024i64),
            ("2025-01-01 01:01:01", 2025i64),
            ("2026-01-01 01:01:01", 2026i64),
            ("2027-01-01 01:01:01", 2027i64),
            ("2028-01-01 01:01:01", 2028i64),
            ("2029-01-01 01:01:01", 2029i64),
        ];
        let mut ctx = EvalContext::default();
        for (arg, exp) in cases {
            test_ok_case_one_arg(
                &mut ctx,
                ScalarFuncSig::Year,
                Datum::Time(Time::parse_utc_datetime(arg, 6).unwrap()),
                Datum::I64(exp),
            );
        }
        // test NULL case
        test_err_case_one_arg(&mut ctx, ScalarFuncSig::Year, Datum::Null);
        // test zero case
        let mut cfg = EvalConfig::new();
        cfg.set_by_flags(FLAG_IN_UPDATE_OR_DELETE_STMT);
        cfg.set_sql_mode(MODE_NO_ZERO_DATE_MODE);
        cfg.set_strict_sql_mode(true);
        ctx = EvalContext::new(Arc::new(cfg));
        test_err_case_one_arg(
            &mut ctx,
            ScalarFuncSig::Year,
            Datum::Time(Time::parse_utc_datetime("0000-00-00 00:00:00", 6).unwrap()),
        );
    }

    #[test]
    fn test_week_with_mode() {
        let cases = vec![
            ("2008-02-20 00:00:00", 1, 8i64),
            ("2008-12-31 00:00:00", 1, 53i64),
            ("2000-01-01", 0, 0i64),
            ("2008-02-20", 0, 7i64),
            ("2017-01-01", 0, 1i64),
            ("2017-01-01", 1, 0i64),
            ("2017-01-01", 2, 1i64),
            ("2017-01-01", 3, 52i64),
            ("2017-01-01", 4, 1i64),
            ("2017-01-01", 5, 0i64),
            ("2017-01-01", 6, 1i64),
            ("2017-01-01", 7, 52i64),
            ("2017-12-31", 0, 53i64),
            ("2017-12-31", 1, 52i64),
            ("2017-12-31", 2, 53i64),
            ("2017-12-31", 3, 52i64),
            ("2017-12-31", 4, 53i64),
            ("2017-12-31", 5, 52i64),
            ("2017-12-31", 6, 1i64),
            ("2017-12-31", 7, 52i64),
            ("2017-12-31", 14, 1i64),
        ];
        let mut ctx = EvalContext::default();
        for (arg1, arg2, exp) in cases {
            test_ok_case_two_arg(
                &mut ctx,
                ScalarFuncSig::WeekWithMode,
                Datum::Time(Time::parse_utc_datetime(arg1, 6).unwrap()),
                Datum::I64(arg2),
                Datum::I64(exp),
            );
        }
        // test NULL case
        test_err_case_two_arg(
            &mut ctx,
            ScalarFuncSig::WeekWithMode,
            Datum::Null,
            Datum::Null,
        );
    }

    #[test]
    fn test_week_without_mode() {
        let cases = vec![("2000-01-01", 0i64)];
        let mut ctx = EvalContext::default();
        for (arg, exp) in cases {
            test_ok_case_one_arg(
                &mut ctx,
                ScalarFuncSig::WeekWithoutMode,
                Datum::Time(Time::parse_utc_datetime(arg, 6).unwrap()),
                Datum::I64(exp),
            );
        }
        // test NULL case
        test_err_case_one_arg(&mut ctx, ScalarFuncSig::WeekWithoutMode, Datum::Null);
    }

    #[test]
    fn test_week_day() {
        let cases = vec![
            ("2018-12-03", 0i64),
            ("2018-12-04", 1i64),
            ("2018-12-05", 2i64),
            ("2018-12-06", 3i64),
            ("2018-12-07", 4i64),
            ("2018-12-08", 5i64),
            ("2018-12-09", 6i64),
        ];
        let mut ctx = EvalContext::default();
        for (arg, exp) in cases {
            test_ok_case_one_arg(
                &mut ctx,
                ScalarFuncSig::WeekDay,
                Datum::Time(Time::parse_utc_datetime(arg, 6).unwrap()),
                Datum::I64(exp),
            );
        }
        // test NULL case
        test_err_case_one_arg(&mut ctx, ScalarFuncSig::WeekDay, Datum::Null);
    }

    #[test]
    fn test_week_of_year() {
        let cases = vec![
            ("2018-01-01", 1i64),
            ("2018-02-28", 9i64),
            ("2018-06-01", 22i64),
            ("2018-07-31", 31i64),
            ("2018-11-01", 44i64),
            ("2018-12-30", 52i64),
            ("2018-12-31", 1i64),
            ("2017-01-01", 52i64),
            ("2017-12-31", 52i64),
        ];
        let mut ctx = EvalContext::default();
        for (arg, exp) in cases {
            test_ok_case_one_arg(
                &mut ctx,
                ScalarFuncSig::WeekOfYear,
                Datum::Time(Time::parse_utc_datetime(arg, 6).unwrap()),
                Datum::I64(exp),
            );
        }
        // test NULL case
        test_err_case_one_arg(&mut ctx, ScalarFuncSig::WeekOfYear, Datum::Null);
    }

    #[test]
    fn test_year_week_with_mode() {
        let cases = vec![
            ("1987-01-01", 0, 198652),
            ("2000-01-01", 0, 199952),
            ("0000-01-01", 0, 1),
            ("0000-01-01", 1, 4294967295),
            ("0000-01-01", 2, 1),
            ("0000-01-01", 3, 4294967295),
            ("0000-01-01", 4, 1),
            ("0000-01-01", 5, 4294967295),
            ("0000-01-01", 6, 1),
            ("0000-01-01", 7, 4294967295),
            ("0000-01-01", 15, 4294967295),
        ];
        let mut ctx = EvalContext::default();
        for (arg1, arg2, exp) in cases {
            test_ok_case_two_arg(
                &mut ctx,
                ScalarFuncSig::YearWeekWithMode,
                Datum::Time(Time::parse_utc_datetime(arg1, 6).unwrap()),
                Datum::I64(arg2),
                Datum::I64(exp),
            );
        }

        // test NULL case
        test_err_case_two_arg(
            &mut ctx,
            ScalarFuncSig::YearWeekWithMode,
            Datum::Null,
            Datum::Null,
        );

        // test ZERO case
        test_err_case_two_arg(
            &mut ctx,
            ScalarFuncSig::YearWeekWithMode,
            Datum::Time(Time::parse_utc_datetime("0000-00-00 00:00:00", 6).unwrap()),
            Datum::I64(0),
        );
    }

    #[test]
    fn test_year_week_without_mode() {
        let cases = vec![
            ("1987-01-01", 198652),
            ("2000-01-01", 199952),
            ("0000-01-01", 1),
        ];
        let mut ctx = EvalContext::default();
        for (arg, exp) in cases {
            test_ok_case_one_arg(
                &mut ctx,
                ScalarFuncSig::YearWeekWithoutMode,
                Datum::Time(Time::parse_utc_datetime(arg, 6).unwrap()),
                Datum::I64(exp),
            );
        }

        // test NULL case
        test_err_case_one_arg(&mut ctx, ScalarFuncSig::YearWeekWithoutMode, Datum::Null);

        // test ZERO case
        test_err_case_one_arg(
            &mut ctx,
            ScalarFuncSig::YearWeekWithoutMode,
            Datum::Time(Time::parse_utc_datetime("0000-00-00 00:00:00", 6).unwrap()),
        );
    }

    #[test]
    fn test_add_sub_datetime_and_duration() {
        let cases = vec![
            (
                "2018-01-01",
                "11:30:45.123456",
                "2018-01-01 11:30:45.123456",
            ),
            (
                "2018-02-28 23:00:00",
                "01:30:30.123456",
                "2018-03-01 00:30:30.123456",
            ),
            ("2016-02-28 23:00:00", "01:30:30", "2016-02-29 00:30:30"),
            ("2018-12-31 23:00:00", "01:30:30", "2019-01-01 00:30:30"),
            ("2018-12-31 23:00:00", "1 01:30:30", "2019-01-02 00:30:30"),
        ];
        let mut ctx = EvalContext::default();
        for (arg1, arg2, exp) in cases {
            test_ok_case_two_arg(
                &mut ctx,
                ScalarFuncSig::AddDatetimeAndDuration,
                Datum::Time(Time::parse_utc_datetime(arg1, 6).unwrap()),
                Datum::Dur(Duration::parse(arg2.as_bytes(), 6).unwrap()),
                Datum::Time(Time::parse_utc_datetime(exp, 6).unwrap()),
            );
            test_ok_case_two_arg(
                &mut ctx,
                ScalarFuncSig::SubDatetimeAndDuration,
                Datum::Time(Time::parse_utc_datetime(exp, 6).unwrap()),
                Datum::Dur(Duration::parse(arg2.as_bytes(), 6).unwrap()),
                Datum::Time(Time::parse_utc_datetime(arg1, 6).unwrap()),
            );
        }

        let cases = vec![
            (
                Datum::Time(Time::parse_utc_datetime("2018-01-01", 6).unwrap()),
                Datum::Null,
                Datum::Null,
            ),
            (
                Datum::Null,
                Datum::Dur(Duration::parse(b"11:30:45.123456", 6).unwrap()),
                Datum::Null,
            ),
            (Datum::Null, Datum::Null, Datum::Null),
        ];
        for (arg1, arg2, exp) in cases {
            test_ok_case_two_arg(
                &mut ctx,
                ScalarFuncSig::AddDatetimeAndDuration,
                arg1.clone(),
                arg2.clone(),
                exp.clone(),
            );
            test_ok_case_two_arg(
                &mut ctx,
                ScalarFuncSig::SubDatetimeAndDuration,
                arg1,
                arg2,
                exp,
            );
        }

        let cases = vec![
            (
                Datum::Time(Time::parse_utc_datetime("2019-01-01 01:00:00", 6).unwrap()),
                Datum::Dur(Duration::zero()),
                Datum::Time(Time::parse_utc_datetime("2019-01-01 01:00:00", 6).unwrap()),
            ),
            (
                Datum::Time(Time::parse_utc_datetime("2019-01-01 01:00:00", 6).unwrap()),
                Datum::Dur(Duration::parse(b"-01:01:00", 6).unwrap()),
                Datum::Time(Time::parse_utc_datetime("2018-12-31 23:59:00", 6).unwrap()),
            ),
        ];
        for (arg1, arg2, exp) in cases {
            test_ok_case_two_arg(
                &mut ctx,
                ScalarFuncSig::AddDatetimeAndDuration,
                arg1.clone(),
                arg2.clone(),
                exp.clone(),
            );
            test_ok_case_two_arg(
                &mut ctx,
                ScalarFuncSig::SubDatetimeAndDuration,
                exp,
                arg2,
                arg1,
            );
        }
    }

    #[test]
    fn test_add_sub_datetime_and_string() {
        let cases = vec![
            (
                "2018-01-01",
                "11:30:45.123456",
                "2018-01-01 11:30:45.123456",
            ),
            (
                "2018-02-28 23:00:00",
                "01:30:30.123456",
                "2018-03-01 00:30:30.123456",
            ),
            ("2016-02-28 23:00:00", "01:30:30", "2016-02-29 00:30:30"),
            ("2018-12-31 23:00:00", "01:30:30", "2019-01-01 00:30:30"),
            ("2018-12-31 23:00:00", "1 01:30:30", "2019-01-02 00:30:30"),
        ];
        let mut ctx = EvalContext::default();
        for (arg1, arg2, exp) in cases {
            test_ok_case_two_arg(
                &mut ctx,
                ScalarFuncSig::AddDatetimeAndString,
                Datum::Time(Time::parse_utc_datetime(arg1, 6).unwrap()),
                Datum::Bytes(arg2.as_bytes().to_vec()),
                Datum::Time(Time::parse_utc_datetime(exp, 6).unwrap()),
            );
            test_ok_case_two_arg(
                &mut ctx,
                ScalarFuncSig::SubDatetimeAndString,
                Datum::Time(Time::parse_utc_datetime(exp, 6).unwrap()),
                Datum::Bytes(arg2.as_bytes().to_vec()),
                Datum::Time(Time::parse_utc_datetime(arg1, 6).unwrap()),
            );
        }

        let cases = vec![
            (
                Datum::Time(Time::parse_utc_datetime("2018-01-01", 6).unwrap()),
                Datum::Null,
                Datum::Null,
            ),
            (
                Datum::Null,
                Datum::Dur(Duration::parse(b"11:30:45.123456", 6).unwrap()),
                Datum::Null,
            ),
            (Datum::Null, Datum::Null, Datum::Null),
        ];
        for (arg1, arg2, exp) in cases {
            test_ok_case_two_arg(
                &mut ctx,
                ScalarFuncSig::AddDatetimeAndString,
                arg1.clone(),
                arg2.clone(),
                exp.clone(),
            );
            test_ok_case_two_arg(
                &mut ctx,
                ScalarFuncSig::SubDatetimeAndString,
                arg1,
                arg2,
                exp,
            );
        }

        let cases = vec![
            (
                Datum::Time(Time::parse_utc_datetime("2019-01-01 01:00:00", 6).unwrap()),
                Datum::Bytes(Vec::new()),
                Datum::Time(Time::parse_utc_datetime("2019-01-01 01:00:00", 6).unwrap()),
            ),
            (
                Datum::Time(Time::parse_utc_datetime("2019-01-01 01:00:00", 6).unwrap()),
                Datum::Bytes(b"-01:01:00".to_vec()),
                Datum::Time(Time::parse_utc_datetime("2018-12-31 23:59:00", 6).unwrap()),
            ),
        ];
        for (arg1, arg2, exp) in cases {
            test_ok_case_two_arg(
                &mut ctx,
                ScalarFuncSig::AddDatetimeAndString,
                arg1.clone(),
                arg2.clone(),
                exp.clone(),
            );
            test_ok_case_two_arg(
                &mut ctx,
                ScalarFuncSig::SubDatetimeAndString,
                exp,
                arg2,
                arg1,
            );
        }
    }

    #[test]
    fn test_add_sub_time_datetime_null() {
        let mut ctx = EvalContext::default();
        test_ok_case_zero_arg(&mut ctx, ScalarFuncSig::AddTimeDateTimeNull, Datum::Null);
        test_ok_case_zero_arg(&mut ctx, ScalarFuncSig::SubTimeDateTimeNull, Datum::Null);
    }

    #[test]
    fn test_to_days() {
        let cases = vec![
            ("950501", 728779),
            ("2007-10-07", 733321),
            ("2008-10-07", 733687),
            ("08-10-07", 733687),
            ("0000-01-01", 1),
            ("2007-10-07 00:00:59", 733321),
        ];
        let mut ctx = EvalContext::default();
        for (arg, exp) in cases {
            test_ok_case_one_arg(
                &mut ctx,
                ScalarFuncSig::ToDays,
                Datum::Time(Time::parse_utc_datetime(arg, 6).unwrap()),
                Datum::I64(exp),
            );
        }

        // test NULL case
        test_err_case_one_arg(&mut ctx, ScalarFuncSig::ToDays, Datum::Null);

        // test ZERO case
        test_err_case_one_arg(
            &mut ctx,
            ScalarFuncSig::ToDays,
            Datum::Time(Time::parse_utc_datetime("0000-00-00 00:00:00", 6).unwrap()),
        );
    }

    #[test]
    fn test_date_diff() {
        let cases = vec![
            (
                "0000-01-01 00:00:00.000000",
                "0000-01-01 00:00:00.000000",
                0,
            ),
            (
                "2018-02-01 00:00:00.000000",
                "2018-02-01 00:00:00.000000",
                0,
            ),
            (
                "2018-02-02 00:00:00.000000",
                "2018-02-01 00:00:00.000000",
                1,
            ),
            (
                "2018-02-01 00:00:00.000000",
                "2018-02-02 00:00:00.000000",
                -1,
            ),
            (
                "2018-02-02 00:00:00.000000",
                "2018-02-01 23:59:59.999999",
                1,
            ),
            (
                "2018-02-01 23:59:59.999999",
                "2018-02-02 00:00:00.000000",
                -1,
            ),
        ];
        let mut ctx = EvalContext::default();
        for (arg1, arg2, exp) in cases {
            test_ok_case_two_arg(
                &mut ctx,
                ScalarFuncSig::DateDiff,
                Datum::Time(Time::parse_utc_datetime(arg1, 6).unwrap()),
                Datum::Time(Time::parse_utc_datetime(arg2, 6).unwrap()),
                Datum::I64(exp),
            );
        }

        let mut cfg = EvalConfig::new();
        cfg.set_by_flags(FLAG_IN_UPDATE_OR_DELETE_STMT)
            .set_sql_mode(MODE_ERROR_FOR_DIVISION_BY_ZERO)
            .set_strict_sql_mode(true);

        test_err_case_two_arg(&mut ctx, ScalarFuncSig::DateDiff, Datum::Null, Datum::Null);
    }

    #[test]
    fn test_add_duration_and_duration() {
        let cases = vec![
            ("01:00:00.999999", "02:00:00.999998", "03:00:01.999997"),
            ("23:59:59", "00:00:01", "24:00:00"),
            ("235959", "00:00:01", "24:00:00"),
            ("110:00:00", "1 02:00:00", "136:00:00"),
            ("-110:00:00", "1 02:00:00", "-84:00:00"),
            ("00:00:01", "-00:00:01", "00:00:00"),
            ("00:00:03", "-00:00:01", "00:00:02"),
        ];
        let mut ctx = EvalContext::default();
        for (arg1, arg2, exp) in cases {
            test_ok_case_two_arg(
                &mut ctx,
                ScalarFuncSig::AddDurationAndDuration,
                Datum::Dur(Duration::parse(arg1.as_ref(), 6).unwrap()),
                Datum::Dur(Duration::parse(arg2.as_ref(), 6).unwrap()),
                Datum::Dur(Duration::parse(exp.as_ref(), 6).unwrap()),
            );
        }

        let zero_duration = Datum::Dur(Duration::zero());
        let cases = vec![
            (
                Datum::Dur(Duration::parse(b"1 01:00:00", 6).unwrap()),
                Datum::Null,
                Datum::Null,
            ),
            (
                Datum::Null,
                Datum::Dur(Duration::parse(b"11:30:45.123456", 6).unwrap()),
                Datum::Null,
            ),
            (Datum::Null, Datum::Null, Datum::Null),
            (
                zero_duration.clone(),
                zero_duration.clone(),
                zero_duration.clone(),
            ),
            (
                Datum::Dur(Duration::parse(b"01:00:00", 6).unwrap()),
                zero_duration.clone(),
                Datum::Dur(Duration::parse(b"01:00:00", 6).unwrap()),
            ),
            (
                Datum::Dur(Duration::parse(b"01:00:00", 6).unwrap()),
                Datum::Dur(Duration::parse(b"-01:00:00", 6).unwrap()),
                zero_duration.clone(),
            ),
        ];
        for (arg1, arg2, exp) in cases {
            test_ok_case_two_arg(
                &mut ctx,
                ScalarFuncSig::AddDurationAndDuration,
                arg1,
                arg2,
                exp,
            );
        }
    }

    #[test]
    fn test_duration_and_string() {
        let cases = vec![
            ("01:00:00.999999", "02:00:00.999998", "03:00:01.999997"),
            ("23:59:59", "00:00:01", "24:00:00"),
            ("235959", "00:00:01", "24:00:00"),
            ("110:00:00", "1 02:00:00", "136:00:00"),
            ("-110:00:00", "1 02:00:00", "-84:00:00"),
            ("00:00:01", "-00:00:01", "00:00:00"),
            ("00:00:03", "-00:00:01", "00:00:02"),
        ];
        let mut ctx = EvalContext::default();
        for (arg1, arg2, exp) in cases {
            test_ok_case_two_arg(
                &mut ctx,
                ScalarFuncSig::AddDurationAndString,
                Datum::Dur(Duration::parse(arg1.as_ref(), 6).unwrap()),
                Datum::Bytes(arg2.as_bytes().to_vec()),
                Datum::Dur(Duration::parse(exp.as_ref(), 6).unwrap()),
            );
        }

        let zero_duration = Datum::Dur(Duration::zero());
        let zero_duration_string = Datum::Bytes(Vec::new());
        let cases = vec![
            (
                Datum::Dur(Duration::parse(b"1 01:00:00", 6).unwrap()),
                Datum::Null,
                Datum::Null,
            ),
            (
                Datum::Null,
                Datum::Bytes(b"11:30:45.123456".to_vec()),
                Datum::Null,
            ),
            (Datum::Null, Datum::Null, Datum::Null),
            (
                zero_duration.clone(),
                zero_duration_string.clone(),
                zero_duration.clone(),
            ),
            (
                zero_duration.clone(),
                Datum::Bytes(b"01:00:00".to_vec()),
                Datum::Dur(Duration::parse(b"01:00:00", 6).unwrap()),
            ),
            (
                Datum::Dur(Duration::parse(b"01:00:00", 6).unwrap()),
                zero_duration_string.clone(),
                Datum::Dur(Duration::parse(b"01:00:00", 6).unwrap()),
            ),
            (
                Datum::Dur(Duration::parse(b"01:00:00", 6).unwrap()),
                Datum::Bytes(b"-01:00:00".to_vec()),
                zero_duration.clone(),
            ),
        ];
        for (arg1, arg2, exp) in cases {
            test_ok_case_two_arg(
                &mut ctx,
                ScalarFuncSig::AddDurationAndString,
                arg1,
                arg2,
                exp,
            );
        }
    }

    #[test]
    fn test_add_time_duration_null() {
        let mut ctx = EvalContext::default();
        test_ok_case_zero_arg(&mut ctx, ScalarFuncSig::AddTimeDurationNull, Datum::Null);
    }

    #[test]
    fn test_sub_duration_and_duration() {
        let cases = vec![
            ("03:00:01.999997", "02:00:00.999998", "01:00:00.999999"),
            ("24:00:00", "00:00:01", "23:59:59"),
            ("24:00:00", "235959", "00:00:01"),
            ("136:00:00", "1 02:00:00", "110:00:00"),
            ("-84:00:00", "1 02:00:00", "-110:00:00"),
            ("00:00:00", "-00:00:01", "00:00:01"),
            ("00:00:02", "00:00:03", "-00:00:01"),
        ];
        let mut ctx = EvalContext::default();
        for (arg1, arg2, exp) in cases {
            test_ok_case_two_arg(
                &mut ctx,
                ScalarFuncSig::SubDurationAndDuration,
                Datum::Dur(Duration::parse(arg1.as_ref(), 6).unwrap()),
                Datum::Dur(Duration::parse(arg2.as_ref(), 6).unwrap()),
                Datum::Dur(Duration::parse(exp.as_ref(), 6).unwrap()),
            );
        }

        let zero_duration = Datum::Dur(Duration::zero());
        let cases = vec![
            (
                Datum::Dur(Duration::parse(b"1 01:00:00", 6).unwrap()),
                Datum::Null,
                Datum::Null,
            ),
            (
                Datum::Null,
                Datum::Dur(Duration::parse(b"11:30:45.123456", 6).unwrap()),
                Datum::Null,
            ),
            (Datum::Null, Datum::Null, Datum::Null),
            (
                zero_duration.clone(),
                zero_duration.clone(),
                zero_duration.clone(),
            ),
            (
                zero_duration.clone(),
                Datum::Dur(Duration::parse(b"01:00:00", 6).unwrap()),
                Datum::Dur(Duration::parse(b"-01:00:00", 6).unwrap()),
            ),
            (
                Datum::Dur(Duration::parse(b"01:00:00", 6).unwrap()),
                zero_duration.clone(),
                Datum::Dur(Duration::parse(b"01:00:00", 6).unwrap()),
            ),
            (
                Datum::Dur(Duration::parse(b"01:00:00", 6).unwrap()),
                Datum::Dur(Duration::parse(b"01:00:00", 6).unwrap()),
                zero_duration.clone(),
            ),
        ];
        for (arg1, arg2, exp) in cases {
            test_ok_case_two_arg(
                &mut ctx,
                ScalarFuncSig::SubDurationAndDuration,
                arg1,
                arg2,
                exp,
            );
        }
    }
}
