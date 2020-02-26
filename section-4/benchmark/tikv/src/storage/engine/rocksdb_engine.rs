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

use std::fmt::{self, Debug, Display, Formatter};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[cfg(not(feature = "no-fail"))]
use kvproto::errorpb::Error as ErrorHeader;
pub use rocksdb_do_not_use::{
    load_latest_options,
    rocksdb::supported_compression,
    // NOTE(yu): maybe this should be an explicit import
    //
    // Like crate::storage::engine::options::UnsafeSnap?
    rocksdb_options::UnsafeSnap,

    run_ldb_tool,
    set_external_sst_file_global_seq_no,

    BlockBasedOptions,
    CColumnFamilyDescriptor,
    CFHandle,
    ColumnFamilyOptions,
    CompactOptions,
    CompactionJobInfo,
    CompactionOptions,
    CompactionPriority,
    DBBottommostLevelCompaction,
    DBCompactionStyle,
    DBCompressionType,
    DBEntryType,
    DBIterator,
    DBOptions,
    DBRateLimiterMode,
    DBRecoveryMode,
    DBStatisticsHistogramType,
    DBStatisticsTickerType,
    DBVector,
    Env,
    EnvOptions,
    EventListener,
    ExternalSstFileInfo,
    FlushJobInfo,
    HistogramData,
    IngestExternalFileOptions,
    IngestionInfo,
    Kv,
    PerfContext,
    Range,
    RateLimiter,
    ReadOptions,
    SeekKey,
    SequentialFile,
    SliceTransform,
    SstFileWriter,
    TablePropertiesCollection,
    TablePropertiesCollector,
    TablePropertiesCollectorFactory,
    TitanBlobIndex,
    TitanDBOptions,
    UserCollectedProperties,
    Writable,
    WriteBatch,
    WriteOptions,
    WriteStallCondition,
    WriteStallInfo,
    DB,
};

use kvproto::kvrpcpb::Context;
use tempdir::TempDir;

use crate::raftstore::store::engine::{IterOption, Peekable};
use crate::storage::{CfName, Key, Value, CF_DEFAULT, CF_LOCK, CF_RAFT, CF_WRITE};

use crate::util::escape;
use crate::util::rocksdb_util::{self, CFOptions};
use crate::util::worker::{Runnable, Scheduler, Worker};

use super::{
    Callback, CbContext, Cursor, Engine, Error, Iterator as EngineIterator, Modify, Result,
    ScanMode, Snapshot,
};

pub use crate::raftstore::store::engine::SyncSnapshot as RocksSnapshot;

const TEMP_DIR: &str = "";

enum Task {
    Write(Vec<Modify>, Callback<()>),
    Snapshot(Callback<RocksSnapshot>),
}

impl Display for Task {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match *self {
            Task::Write(..) => write!(f, "write task"),
            Task::Snapshot(_) => write!(f, "snapshot task"),
        }
    }
}

struct Runner(Arc<DB>);

impl Runnable<Task> for Runner {
    fn run(&mut self, t: Task) {
        match t {
            Task::Write(modifies, cb) => cb((CbContext::new(), write_modifies(&self.0, modifies))),
            Task::Snapshot(cb) => cb((
                CbContext::new(),
                Ok(RocksSnapshot::new(Arc::clone(&self.0))),
            )),
        }
    }
}

struct RocksEngineCore {
    // only use for memory mode
    temp_dir: Option<TempDir>,
    worker: Worker<Task>,
}

impl Drop for RocksEngineCore {
    fn drop(&mut self) {
        if let Some(h) = self.worker.stop() {
            h.join().unwrap();
        }
    }
}

#[derive(Clone)]
pub struct RocksEngine {
    core: Arc<Mutex<RocksEngineCore>>,
    sched: Scheduler<Task>,
    db: Arc<DB>,
}

impl RocksEngine {
    pub fn new(
        path: &str,
        cfs: &[CfName],
        cfs_opts: Option<Vec<CFOptions<'_>>>,
    ) -> Result<RocksEngine> {
        info!("RocksEngine: creating for path"; "path" => path);
        let (path, temp_dir) = match path {
            TEMP_DIR => {
                let td = TempDir::new("temp-rocksdb").unwrap();
                (td.path().to_str().unwrap().to_owned(), Some(td))
            }
            _ => (path.to_owned(), None),
        };
        let mut worker = Worker::new("engine-rocksdb");
        let db = Arc::new(rocksdb_util::new_engine(&path, None, cfs, cfs_opts)?);
        box_try!(worker.start(Runner(Arc::clone(&db))));
        Ok(RocksEngine {
            sched: worker.scheduler(),
            core: Arc::new(Mutex::new(RocksEngineCore { temp_dir, worker })),
            db,
        })
    }

    pub fn get_rocksdb(&self) -> Arc<DB> {
        Arc::clone(&self.db)
    }

    pub fn stop(&self) {
        let mut core = self.core.lock().unwrap();
        if let Some(h) = core.worker.stop() {
            h.join().unwrap();
        }
    }
}

impl Display for RocksEngine {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "RocksDB")
    }
}

impl Debug for RocksEngine {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "RocksDB [is_temp: {}]",
            self.core.lock().unwrap().temp_dir.is_some()
        )
    }
}

/// A builder to build a temporary `RocksEngine`.
///
/// Only used for test purpose.
#[must_use]
pub struct TestEngineBuilder {
    path: Option<PathBuf>,
    cfs: Option<Vec<CfName>>,
}

impl TestEngineBuilder {
    pub fn new() -> Self {
        Self {
            path: None,
            cfs: None,
        }
    }

    /// Customize the data directory of the temporary engine.
    ///
    /// By default, TEMP_DIR will be used.
    pub fn path(mut self, path: impl AsRef<Path>) -> Self {
        self.path = Some(path.as_ref().to_path_buf());
        self
    }

    /// Customize the CFs that engine will have.
    ///
    /// By default, engine will have all CFs.
    pub fn cfs(mut self, cfs: impl AsRef<[CfName]>) -> Self {
        self.cfs = Some(cfs.as_ref().to_vec());
        self
    }

    /// Build a `RocksEngine`.
    pub fn build(self) -> Result<RocksEngine> {
        let path = match self.path {
            None => TEMP_DIR.to_owned(),
            Some(p) => p.to_str().unwrap().to_owned(),
        };
        let cfs = self.cfs.unwrap_or_else(|| crate::storage::ALL_CFS.to_vec());
        let cfg_rocksdb = crate::config::DbConfig::default();
        let cfs_opts = cfs
            .iter()
            .map(|cf| match *cf {
                CF_DEFAULT => CFOptions::new(CF_DEFAULT, cfg_rocksdb.defaultcf.build_opt()),
                CF_LOCK => CFOptions::new(CF_LOCK, cfg_rocksdb.lockcf.build_opt()),
                CF_WRITE => CFOptions::new(CF_WRITE, cfg_rocksdb.writecf.build_opt()),
                CF_RAFT => CFOptions::new(CF_RAFT, cfg_rocksdb.raftcf.build_opt()),
                _ => CFOptions::new(*cf, ColumnFamilyOptions::new()),
            })
            .collect();
        RocksEngine::new(&path, &cfs, Some(cfs_opts))
    }
}

fn write_modifies(db: &DB, modifies: Vec<Modify>) -> Result<()> {
    let wb = WriteBatch::new();
    for rev in modifies {
        let res = match rev {
            Modify::Delete(cf, k) => {
                if cf == CF_DEFAULT {
                    trace!("RocksEngine: delete"; "key" => %k);
                    wb.delete(k.as_encoded())
                } else {
                    trace!("RocksEngine: delete_cf"; "cf" => cf, "key" => %k);
                    let handle = rocksdb_util::get_cf_handle(db, cf)?;
                    wb.delete_cf(handle, k.as_encoded())
                }
            }
            Modify::Put(cf, k, v) => {
                if cf == CF_DEFAULT {
                    trace!("RocksEngine: put"; "key" => %k, "value" => escape(&v));
                    wb.put(k.as_encoded(), &v)
                } else {
                    trace!("RocksEngine: put_cf"; "cf" => cf, "key" => %k, "value" => escape(&v));
                    let handle = rocksdb_util::get_cf_handle(db, cf)?;
                    wb.put_cf(handle, k.as_encoded(), &v)
                }
            }
            Modify::DeleteRange(cf, start_key, end_key) => {
                trace!(
                    "RocksEngine: delete_range_cf";
                    "cf" => cf,
                    "start_key" => %start_key,
                    "end_key" => %end_key
                );
                let handle = rocksdb_util::get_cf_handle(db, cf)?;
                wb.delete_range_cf(handle, start_key.as_encoded(), end_key.as_encoded())
            }
        };
        if let Err(msg) = res {
            return Err(Error::RocksDb(msg));
        }
    }
    if let Err(msg) = db.write(wb) {
        return Err(Error::RocksDb(msg));
    }
    Ok(())
}

impl Engine for RocksEngine {
    type Snap = RocksSnapshot;

    fn async_write(&self, _: &Context, modifies: Vec<Modify>, cb: Callback<()>) -> Result<()> {
        if modifies.is_empty() {
            return Err(Error::EmptyRequest);
        }
        box_try!(self.sched.schedule(Task::Write(modifies, cb)));
        Ok(())
    }

    fn async_snapshot(&self, _: &Context, cb: Callback<Self::Snap>) -> Result<()> {
        fail_point!("rockskv_async_snapshot", |_| Err(box_err!(
            "snapshot failed"
        )));
        fail_point!("rockskv_async_snapshot_not_leader", |_| {
            let mut header = ErrorHeader::new();
            header.mut_not_leader().set_region_id(100);
            Err(Error::Request(header))
        });
        box_try!(self.sched.schedule(Task::Snapshot(cb)));
        Ok(())
    }
}

impl Snapshot for RocksSnapshot {
    type Iter = DBIterator<Arc<DB>>;

    fn get(&self, key: &Key) -> Result<Option<Value>> {
        trace!("RocksSnapshot: get"; "key" => %key);
        let v = box_try!(self.get_value(key.as_encoded()));
        Ok(v.map(|v| v.to_vec()))
    }

    fn get_cf(&self, cf: CfName, key: &Key) -> Result<Option<Value>> {
        trace!("RocksSnapshot: get_cf"; "cf" => cf, "key" => %key);
        let v = box_try!(self.get_value_cf(cf, key.as_encoded()));
        Ok(v.map(|v| v.to_vec()))
    }

    fn iter(&self, iter_opt: IterOption, mode: ScanMode) -> Result<Cursor<Self::Iter>> {
        trace!("RocksSnapshot: create iterator");
        let iter = self.db_iterator(iter_opt);
        Ok(Cursor::new(iter, mode))
    }

    fn iter_cf(
        &self,
        cf: CfName,
        iter_opt: IterOption,
        mode: ScanMode,
    ) -> Result<Cursor<Self::Iter>> {
        trace!("RocksSnapshot: create cf iterator");
        let iter = self.db_iterator_cf(cf, iter_opt)?;
        Ok(Cursor::new(iter, mode))
    }
}

impl<D: Deref<Target = DB> + Send> EngineIterator for DBIterator<D> {
    fn next(&mut self) -> bool {
        DBIterator::next(self)
    }

    fn prev(&mut self) -> bool {
        DBIterator::prev(self)
    }

    fn seek(&mut self, key: &Key) -> Result<bool> {
        Ok(DBIterator::seek(self, key.as_encoded().as_slice().into()))
    }

    fn seek_for_prev(&mut self, key: &Key) -> Result<bool> {
        Ok(DBIterator::seek_for_prev(
            self,
            key.as_encoded().as_slice().into(),
        ))
    }

    fn seek_to_first(&mut self) -> bool {
        DBIterator::seek(self, SeekKey::Start)
    }

    fn seek_to_last(&mut self) -> bool {
        DBIterator::seek(self, SeekKey::End)
    }

    fn valid(&self) -> bool {
        DBIterator::valid(self)
    }

    fn key(&self) -> &[u8] {
        DBIterator::key(self)
    }

    fn value(&self) -> &[u8] {
        DBIterator::value(self)
    }
}

#[cfg(test)]
mod tests {
    pub use super::super::perf_context::{PerfStatisticsDelta, PerfStatisticsInstant};
    use super::super::tests::*;
    use super::super::CFStatistics;
    use super::*;
    use tempdir::TempDir;

    #[test]
    fn test_rocksdb() {
        let engine = TestEngineBuilder::new()
            .cfs(TEST_ENGINE_CFS)
            .build()
            .unwrap();
        test_base_curd_options(&engine)
    }

    #[test]
    fn test_rocksdb_linear() {
        let engine = TestEngineBuilder::new()
            .cfs(TEST_ENGINE_CFS)
            .build()
            .unwrap();
        test_linear(&engine);
    }

    #[test]
    fn test_rocksdb_statistic() {
        let engine = TestEngineBuilder::new()
            .cfs(TEST_ENGINE_CFS)
            .build()
            .unwrap();
        test_cfs_statistics(&engine);
    }

    #[test]
    fn rocksdb_reopen() {
        let dir = TempDir::new("rocksdb_test").unwrap();
        {
            let engine = TestEngineBuilder::new()
                .path(dir.path())
                .cfs(TEST_ENGINE_CFS)
                .build()
                .unwrap();
            must_put_cf(&engine, "cf", b"k", b"v1");
        }
        {
            let engine = TestEngineBuilder::new()
                .path(dir.path())
                .cfs(TEST_ENGINE_CFS)
                .build()
                .unwrap();
            assert_has_cf(&engine, "cf", b"k", b"v1");
        }
    }

    #[test]
    fn test_rocksdb_perf_statistics() {
        let engine = TestEngineBuilder::new()
            .cfs(TEST_ENGINE_CFS)
            .build()
            .unwrap();
        test_perf_statistics(&engine);
    }

    pub fn test_perf_statistics<E: Engine>(engine: &E) {
        must_put(engine, b"foo", b"bar1");
        must_put(engine, b"foo2", b"bar2");
        must_put(engine, b"foo3", b"bar3"); // deleted
        must_put(engine, b"foo4", b"bar4");
        must_put(engine, b"foo42", b"bar42"); // deleted
        must_put(engine, b"foo5", b"bar5"); // deleted
        must_put(engine, b"foo6", b"bar6");
        must_delete(engine, b"foo3");
        must_delete(engine, b"foo42");
        must_delete(engine, b"foo5");

        let snapshot = engine.snapshot(&Context::new()).unwrap();
        let mut iter = snapshot
            .iter(IterOption::default(), ScanMode::Forward)
            .unwrap();

        let mut statistics = CFStatistics::default();

        let perf_statistics = PerfStatisticsInstant::new();
        iter.seek(&Key::from_raw(b"foo30"), &mut statistics)
            .unwrap();
        assert_eq!(perf_statistics.delta().internal_delete_skipped_count, 0);

        let perf_statistics = PerfStatisticsInstant::new();
        iter.near_seek(&Key::from_raw(b"foo55"), &mut statistics)
            .unwrap();
        assert_eq!(perf_statistics.delta().internal_delete_skipped_count, 2);

        let perf_statistics = PerfStatisticsInstant::new();
        iter.prev(&mut statistics);
        assert_eq!(perf_statistics.delta().internal_delete_skipped_count, 2);

        iter.prev(&mut statistics);
        assert_eq!(perf_statistics.delta().internal_delete_skipped_count, 3);

        iter.prev(&mut statistics);
        assert_eq!(perf_statistics.delta().internal_delete_skipped_count, 3);
    }

}
