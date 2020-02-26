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

use crate::storage::engine::DB;
use kvproto::import_sstpb::*;

use super::Result;

pub struct ImportModeSwitcher {
    mode: SwitchMode,
    backup_db_options: ImportModeDBOptions,
    backup_cf_options: Vec<(String, ImportModeCFOptions)>,
}

impl ImportModeSwitcher {
    pub fn new() -> ImportModeSwitcher {
        ImportModeSwitcher {
            mode: SwitchMode::Normal,
            backup_db_options: ImportModeDBOptions::new(),
            backup_cf_options: Vec::new(),
        }
    }

    pub fn enter_normal_mode(&mut self, db: &DB) -> Result<()> {
        if self.mode == SwitchMode::Normal {
            return Ok(());
        }

        self.backup_db_options.set_options(db)?;
        for (cf_name, cf_opts) in &self.backup_cf_options {
            cf_opts.set_options(db, cf_name)?;
        }

        self.mode = SwitchMode::Normal;
        Ok(())
    }

    pub fn enter_import_mode(&mut self, db: &DB) -> Result<()> {
        if self.mode == SwitchMode::Import {
            return Ok(());
        }

        self.backup_db_options = ImportModeDBOptions::new_options(db);
        self.backup_cf_options.clear();

        let import_db_options = ImportModeDBOptions::new();
        import_db_options.set_options(db)?;
        let import_cf_options = ImportModeCFOptions::new();
        for cf_name in db.cf_names() {
            let cf_opts = ImportModeCFOptions::new_options(db, cf_name);
            self.backup_cf_options.push((cf_name.to_owned(), cf_opts));
            import_cf_options.set_options(db, cf_name)?;
        }

        self.mode = SwitchMode::Import;
        Ok(())
    }
}

struct ImportModeDBOptions {
    max_background_jobs: i32,
}

impl ImportModeDBOptions {
    fn new() -> ImportModeDBOptions {
        ImportModeDBOptions {
            max_background_jobs: 32,
        }
    }

    fn new_options(db: &DB) -> ImportModeDBOptions {
        let db_opts = db.get_db_options();
        ImportModeDBOptions {
            max_background_jobs: db_opts.get_max_background_jobs(),
        }
    }

    fn set_options(&self, db: &DB) -> Result<()> {
        let opts = [(
            "max_background_jobs".to_string(),
            self.max_background_jobs.to_string(),
        )];
        let tmp_opts: Vec<_> = opts.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        db.set_db_options(&tmp_opts)?;
        Ok(())
    }
}

struct ImportModeCFOptions {
    block_cache_size: u64,
    level0_stop_writes_trigger: u32,
    level0_slowdown_writes_trigger: u32,
    soft_pending_compaction_bytes_limit: u64,
    hard_pending_compaction_bytes_limit: u64,
}

impl ImportModeCFOptions {
    fn new() -> ImportModeCFOptions {
        ImportModeCFOptions {
            block_cache_size: 4 << 30,
            level0_stop_writes_trigger: 1 << 30,
            level0_slowdown_writes_trigger: 1 << 30,
            soft_pending_compaction_bytes_limit: 0,
            hard_pending_compaction_bytes_limit: 0,
        }
    }

    fn new_options(db: &DB, cf_name: &str) -> ImportModeCFOptions {
        let cf = db.cf_handle(cf_name).unwrap();
        let cf_opts = db.get_options_cf(cf);

        ImportModeCFOptions {
            block_cache_size: cf_opts.get_block_cache_capacity(),
            level0_stop_writes_trigger: cf_opts.get_level_zero_stop_writes_trigger(),
            level0_slowdown_writes_trigger: cf_opts.get_level_zero_slowdown_writes_trigger(),
            soft_pending_compaction_bytes_limit: cf_opts.get_soft_pending_compaction_bytes_limit(),
            hard_pending_compaction_bytes_limit: cf_opts.get_hard_pending_compaction_bytes_limit(),
        }
    }

    fn set_options(&self, db: &DB, cf_name: &str) -> Result<()> {
        use crate::server::CONFIG_ROCKSDB_GAUGE;
        let cf = db.cf_handle(cf_name).unwrap();
        let cf_opts = db.get_options_cf(cf);
        cf_opts.set_block_cache_capacity(self.block_cache_size)?;
        CONFIG_ROCKSDB_GAUGE
            .with_label_values(&[cf_name, "block_cache_size"])
            .set(self.block_cache_size as f64);
        let opts = [
            (
                "level0_stop_writes_trigger".to_owned(),
                self.level0_stop_writes_trigger.to_string(),
            ),
            (
                "level0_slowdown_writes_trigger".to_owned(),
                self.level0_slowdown_writes_trigger.to_string(),
            ),
            (
                "soft_pending_compaction_bytes_limit".to_owned(),
                self.soft_pending_compaction_bytes_limit.to_string(),
            ),
            (
                "hard_pending_compaction_bytes_limit".to_owned(),
                self.hard_pending_compaction_bytes_limit.to_string(),
            ),
        ];

        let tmp_opts: Vec<_> = opts.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        db.set_options_cf(cf, tmp_opts.as_slice())?;
        for (key, value) in &opts {
            if let Ok(v) = value.parse::<f64>() {
                CONFIG_ROCKSDB_GAUGE
                    .with_label_values(&[cf_name, key])
                    .set(v);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::util::rocksdb_util::new_engine;
    use tempdir::TempDir;

    fn check_import_options(
        db: &DB,
        expected_db_opts: &ImportModeDBOptions,
        expected_cf_opts: &ImportModeCFOptions,
    ) {
        let db_opts = db.get_db_options();
        assert_eq!(
            db_opts.get_max_background_jobs(),
            expected_db_opts.max_background_jobs,
        );

        for cf_name in db.cf_names() {
            let cf = db.cf_handle(cf_name).unwrap();
            let cf_opts = db.get_options_cf(cf);
            assert_eq!(
                cf_opts.get_block_cache_capacity(),
                expected_cf_opts.block_cache_size
            );
            assert_eq!(
                cf_opts.get_level_zero_stop_writes_trigger(),
                expected_cf_opts.level0_stop_writes_trigger
            );
            assert_eq!(
                cf_opts.get_level_zero_slowdown_writes_trigger(),
                expected_cf_opts.level0_slowdown_writes_trigger
            );
            assert_eq!(
                cf_opts.get_soft_pending_compaction_bytes_limit(),
                expected_cf_opts.soft_pending_compaction_bytes_limit
            );
            assert_eq!(
                cf_opts.get_hard_pending_compaction_bytes_limit(),
                expected_cf_opts.hard_pending_compaction_bytes_limit
            );
        }
    }

    #[test]
    fn test_import_mode_switcher() {
        let temp_dir = TempDir::new("test_import_mode_switcher").unwrap();
        let db = new_engine(temp_dir.path().to_str().unwrap(), None, &["a", "b"], None).unwrap();

        let import_db_options = ImportModeDBOptions::new();
        let normal_db_options = ImportModeDBOptions::new_options(&db);
        let import_cf_options = ImportModeCFOptions::new();
        let normal_cf_options = ImportModeCFOptions::new_options(&db, "default");

        let mut switcher = ImportModeSwitcher::new();
        check_import_options(&db, &normal_db_options, &normal_cf_options);
        switcher.enter_import_mode(&db).unwrap();
        check_import_options(&db, &import_db_options, &import_cf_options);
        switcher.enter_import_mode(&db).unwrap();
        check_import_options(&db, &import_db_options, &import_cf_options);
        switcher.enter_normal_mode(&db).unwrap();
        check_import_options(&db, &normal_db_options, &normal_cf_options);
        switcher.enter_normal_mode(&db).unwrap();
        check_import_options(&db, &normal_db_options, &normal_cf_options);
    }
}
