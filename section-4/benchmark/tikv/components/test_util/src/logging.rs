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

use std::env;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::sync::Mutex;

use slog::{self, Drain, OwnedKVList, Record};
use time;

use tikv;

struct Serializer<'a>(&'a mut dyn std::io::Write);

impl<'a> slog::ser::Serializer for Serializer<'a> {
    fn emit_arguments(&mut self, key: slog::Key, val: &std::fmt::Arguments<'_>) -> slog::Result {
        write!(self.0, ", {}: {}", key, val)?;
        Ok(())
    }
}

/// A logger that add a test case tag before each line of log.
struct CaseTraceLogger {
    f: Option<Mutex<File>>,
}

impl CaseTraceLogger {
    fn write_log(
        w: &mut dyn ::std::io::Write,
        record: &Record<'_>,
        values: &OwnedKVList,
    ) -> Result<(), ::std::io::Error> {
        use slog::KV;

        let tag = tikv::util::get_tag_from_thread_name().map_or_else(|| "".to_owned(), |s| s + " ");
        let t = time::now();
        let time_str = time::strftime("%Y/%m/%d %H:%M:%S.%f", &t).unwrap();
        write!(
            w,
            "{}{} {}:{}: [{}] {}",
            tag,
            &time_str[..time_str.len() - 6],
            record.file().rsplit('/').nth(0).unwrap(),
            record.line(),
            record.level(),
            record.msg(),
        )?;
        {
            let mut s = Serializer(w);
            record.kv().serialize(record, &mut s)?;
            values.serialize(record, &mut s)?;
        }
        writeln!(w)?;
        w.flush()?;
        Ok(())
    }
}

impl Drain for CaseTraceLogger {
    type Ok = ();
    type Err = slog::Never;
    fn log(&self, record: &Record<'_>, values: &OwnedKVList) -> Result<Self::Ok, Self::Err> {
        if let Some(ref out) = self.f {
            let mut w = out.lock().unwrap();
            let _ = Self::write_log(&mut *w, record, values);
        } else {
            let mut w = io::stderr();
            let _ = Self::write_log(&mut w, record, values);
        }
        Ok(())
    }
}

impl Drop for CaseTraceLogger {
    fn drop(&mut self) {
        if let Some(ref w) = self.f {
            w.lock().unwrap().flush().unwrap();
        }
    }
}

// A help function to initial logger.
pub fn init_log_for_test() {
    let output = env::var("LOG_FILE").ok();
    let level = tikv::util::logger::get_level_by_string(
        &env::var("LOG_LEVEL").unwrap_or_else(|_| "debug".to_owned()),
    )
    .unwrap();
    let writer = output.map(|f| Mutex::new(File::create(f).unwrap()));
    // we don't mind set it multiple times.
    let drain = CaseTraceLogger { f: writer };

    // Collects following targets.
    const ENABLED_TARGETS: &[&str] = &[
        "tikv::",
        "tests::",
        "benches::",
        "integrations::",
        "failpoints::",
        "raft::",
        // Collects logs for test components.
        "test_",
    ];
    let filtered = drain.filter(|record| {
        ENABLED_TARGETS
            .iter()
            .any(|target| record.module().starts_with(target))
    });

    // CaseTraceLogger relies on test's thread name, however slog_async has
    // its own thread, and the name is "".
    // TODO: Enable the slog_async when the [Custom test frameworks][1] is mature,
    //       and hook the slog_async logger to every test cases.
    //
    // [1]: https://github.com/rust-lang/rfcs/blob/master/text/2318-custom-test-frameworks.md
    tikv::util::logger::init_log(
        filtered, level, false, // disable async drainer
        true,  // init std log
    )
    .unwrap()
}
