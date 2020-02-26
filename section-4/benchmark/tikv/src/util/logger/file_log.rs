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

use chrono::{DateTime, Duration, Utc};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Adds `Duration` to the initial date and time.
fn compute_rotation_time(initial: &DateTime<Utc>, timespan: Duration) -> DateTime<Utc> {
    *initial + timespan
}

/// Rotates file path with given timestamp.
fn rotation_file_path_with_timestamp(
    file_path: impl AsRef<Path>,
    timestamp: &DateTime<Utc>,
) -> PathBuf {
    let file_path = file_path.as_ref();
    let file_name = file_path
        .file_name()
        .and_then(|x| x.to_str())
        .expect("Log file name was not valid.");
    file_path.with_file_name(format!(
        "{}.{}",
        file_name,
        timestamp.format("%Y-%m-%d-%H:%M:%S")
    ))
}

/// Opens log file with append mode. Creates a new log file if it doesn't exist.
fn open_log_file(path: impl AsRef<Path>) -> io::Result<File> {
    let path = path.as_ref();
    let parent = path
        .parent()
        .expect("Unable to get parent directory of log file");
    if !parent.is_dir() {
        fs::create_dir_all(parent)?
    }
    OpenOptions::new().append(true).create(true).open(path)
}

/// This FileLogger rotates logs according to a time span.
/// After rotating, the original log file would be renamed to "{original name}.{%Y-%m-%d-%H:%M:%S}"
/// Note: log file will *not* be compressed or otherwise modified.
pub struct RotatingFileLogger {
    rotation_timespan: Duration,
    next_rotation_time: DateTime<Utc>,
    file_path: PathBuf,
    file: Option<File>,
}

impl RotatingFileLogger {
    pub fn new(file_path: impl AsRef<Path>, rotation_timespan: Duration) -> io::Result<Self> {
        let file_path = file_path.as_ref().to_path_buf();
        let file = Some(open_log_file(&file_path)?);
        let file_attr = fs::metadata(&file_path)?;
        let file_modified_time = file_attr.modified().unwrap().into();
        let next_rotation_time = compute_rotation_time(&file_modified_time, rotation_timespan);
        Ok(Self {
            next_rotation_time,
            file_path,
            rotation_timespan,
            file,
        })
    }

    /// Opens log file with append mode. Creates a new file if it doesn't exist.
    fn open(&mut self) -> io::Result<()> {
        self.file = Some(open_log_file(&self.file_path)?);
        Ok(())
    }

    fn should_rotate(&mut self) -> bool {
        Utc::now() > self.next_rotation_time
    }

    /// Rotates the current file and updates the next rotation time.
    fn rotate(&mut self) -> io::Result<()> {
        self.close()?;
        let new_path = rotation_file_path_with_timestamp(&self.file_path, &Utc::now());
        fs::rename(&self.file_path, &new_path)?;
        self.update_rotation_time();
        self.open()
    }

    /// Updates the next rotation time.
    fn update_rotation_time(&mut self) {
        let now = Utc::now();
        self.next_rotation_time = compute_rotation_time(&now, self.rotation_timespan);
    }

    /// Flushes and closes log file, without rotation.
    fn close(&mut self) -> io::Result<()> {
        assert!(self.file.is_some());
        self.file.take().unwrap().flush()
    }
}

impl Write for RotatingFileLogger {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        assert!(self.file.is_some());
        self.file.as_mut().unwrap().write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.should_rotate() {
            self.rotate()?;
        };
        assert!(self.file.is_some());
        self.file.as_mut().unwrap().flush()
    }
}

impl Drop for RotatingFileLogger {
    fn drop(&mut self) {
        self.close().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;
    use std::io::prelude::*;
    use std::path::Path;

    use chrono::{Duration, Utc};
    use tempdir::TempDir;
    use utime;

    use super::{rotation_file_path_with_timestamp, RotatingFileLogger};

    fn file_exists(file: impl AsRef<Path>) -> bool {
        let path = file.as_ref();
        path.exists() && path.is_file()
    }

    #[test]
    fn test_rotating_file_logger() {
        let tmp_dir = TempDir::new("").unwrap();
        let log_file = tmp_dir
            .path()
            .join("test_rotating_file_logger.log")
            .to_str()
            .unwrap()
            .to_string();
        // create a file with mtime == one day ago
        {
            let mut file = OpenOptions::new()
                .append(true)
                .create(true)
                .open(&log_file)
                .unwrap();
            file.write_all(b"hello world!").unwrap();
        }
        let now = Utc::now();
        let one_day = Duration::days(1);
        let one_day_ago = now - one_day;
        let one_day_ago_ts = one_day_ago.timestamp() as u64;
        utime::set_file_times(&log_file, one_day_ago_ts, one_day_ago_ts).unwrap();
        // initialize the logger
        let mut logger = RotatingFileLogger::new(&log_file, one_day).unwrap();
        assert!(logger.should_rotate());
        logger.rotate().unwrap();
        // check the rotated file exist
        let rotated_file = rotation_file_path_with_timestamp(&log_file, &now);
        assert!(file_exists(&rotated_file));
        assert!(!logger.should_rotate());
    }

    #[test]
    fn test_close_file_logger() {
        let tmp_dir = TempDir::new("").unwrap();
        let log_file = tmp_dir
            .path()
            .join("test_close_file_logger.log")
            .to_str()
            .unwrap()
            .to_string();
        let one_day = Duration::days(1);

        let mut logger = RotatingFileLogger::new(&log_file, one_day).unwrap();
        // Handles written amount returned.
        let _ = logger.write(b"write before close").unwrap();
        logger.flush().unwrap();
        logger.close().unwrap();
        assert!(::panic_hook::recover_safe(|| logger.write(b"write after close")).is_err());
        assert!(::panic_hook::recover_safe(|| logger.flush()).is_err());
        assert!(::panic_hook::recover_safe(|| logger.close()).is_err());
        // Reopens file, otherwise `close()` will fail in assertion when `drop()`.
        logger.open().unwrap();
        drop(logger);
    }
}
