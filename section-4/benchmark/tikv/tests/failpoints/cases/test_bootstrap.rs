// Copyright 2019 PingCAP, Inc.
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

use std::sync::{Arc, RwLock};

use kvproto::{metapb, raft_serverpb};
use test_raftstore::*;
use tikv::raftstore::store::{keys, Peekable};

fn test_boostrap_half_way_failure(fp: &str) {
    let pd_client = Arc::new(TestPdClient::new(0, false));
    let sim = Arc::new(RwLock::new(NodeCluster::new(pd_client.clone())));
    let mut cluster = Cluster::new(0, 5, sim.clone(), pd_client.clone());

    // Try to start this node, return after persisted some keys.
    fail::cfg(fp, "return").unwrap();
    cluster.start().unwrap_err();

    let engines = cluster.dbs[0].clone();
    let ident = engines
        .kv
        .get_msg::<raft_serverpb::StoreIdent>(keys::STORE_IDENT_KEY)
        .unwrap()
        .unwrap();
    let store_id = ident.get_store_id();
    debug!("store id {:?}", store_id);
    assert!(cluster.engines.insert(store_id, engines.clone()).is_none());

    // Check whether it can bootstrap cluster successfully.
    fail::remove(fp);
    cluster.start().unwrap();

    assert!(engines
        .kv
        .get_msg::<metapb::Region>(keys::PREPARE_BOOTSTRAP_KEY)
        .unwrap()
        .is_none());

    let k = b"k1";
    let v = b"v1";
    cluster.must_put(k, v);
    must_get_equal(&cluster.get_engine(store_id), k, v);
    for id in cluster.engines.keys() {
        must_get_equal(&cluster.get_engine(*id), k, v);
    }
}

#[test]
fn test_boostrap_half_way_failure_after_bootstrap_store() {
    let _guard = crate::setup();

    let fp = "node_after_bootstrap_store";
    test_boostrap_half_way_failure(fp);
}

#[test]
fn test_boostrap_half_way_failure_after_prepare_bootstrap_cluster() {
    let _guard = crate::setup();

    let fp = "node_after_prepare_bootstrap_cluster";
    test_boostrap_half_way_failure(fp);
}

#[test]
fn test_boostrap_half_way_failure_after_bootstrap_cluster() {
    let _guard = crate::setup();

    let fp = "node_after_bootstrap_cluster";
    test_boostrap_half_way_failure(fp);
}
