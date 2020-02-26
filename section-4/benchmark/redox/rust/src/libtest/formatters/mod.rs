use std::{
    io,
    io::prelude::Write,
};

use crate::{
    types::{TestDesc, TestName},
    time,
    test_result::TestResult,
    console::{ConsoleTestState},
};

mod pretty;
mod json;
mod terse;

pub(crate) use self::pretty::PrettyFormatter;
pub(crate) use self::json::JsonFormatter;
pub(crate) use self::terse::TerseFormatter;

pub(crate) trait OutputFormatter {
    fn write_run_start(&mut self, test_count: usize) -> io::Result<()>;
    fn write_test_start(&mut self, desc: &TestDesc) -> io::Result<()>;
    fn write_timeout(&mut self, desc: &TestDesc) -> io::Result<()>;
    fn write_result(
        &mut self,
        desc: &TestDesc,
        result: &TestResult,
        exec_time: Option<&time::TestExecTime>,
        stdout: &[u8],
        state: &ConsoleTestState,
    ) -> io::Result<()>;
    fn write_run_finish(&mut self, state: &ConsoleTestState) -> io::Result<bool>;
}

pub(crate) fn write_stderr_delimiter(test_output: &mut Vec<u8>, test_name: &TestName) {
    match test_output.last() {
        Some(b'\n') => (),
        Some(_) => test_output.push(b'\n'),
        None => (),
    }
    write!(test_output, "---- {} stderr ----\n", test_name).unwrap();
}
