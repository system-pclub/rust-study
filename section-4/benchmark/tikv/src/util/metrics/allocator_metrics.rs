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

use prometheus::core::{Collector, Desc};
use prometheus::proto::MetricFamily;
use prometheus::{IntGaugeVec, Opts, Result};

use tikv_alloc;

pub fn monitor_allocator_stats<S: Into<String>>(namespace: S) -> Result<()> {
    prometheus::register(Box::new(AllocStatsCollector::new(namespace)?))
}

struct AllocStatsCollector {
    descs: Vec<Desc>,
    metrics: IntGaugeVec,
}

impl AllocStatsCollector {
    fn new<S: Into<String>>(namespace: S) -> Result<AllocStatsCollector> {
        let stats = IntGaugeVec::new(
            Opts::new("allocator_stats", "Allocator stats").namespace(namespace.into()),
            &["type"],
        )?;
        Ok(AllocStatsCollector {
            descs: stats.desc().into_iter().cloned().collect(),
            metrics: stats,
        })
    }
}

impl Collector for AllocStatsCollector {
    fn desc(&self) -> Vec<&Desc> {
        self.descs.iter().collect()
    }

    fn collect(&self) -> Vec<MetricFamily> {
        if let Ok(Some(stats)) = tikv_alloc::fetch_stats() {
            for stat in stats {
                self.metrics.with_label_values(&[stat.0]).set(stat.1 as i64);
            }
        }
        self.metrics.collect()
    }
}
