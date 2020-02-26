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

use std::sync::Arc;

use crate::grpc::{ChannelBuilder, Environment, Error, RpcStatusCode};
use futures::{future, Future, Stream};
use tikv::storage::engine::Writable;

use kvproto::coprocessor::*;
use kvproto::debugpb_grpc::DebugClient;
use kvproto::kvrpcpb::*;
use kvproto::raft_serverpb::*;
use kvproto::tikvpb_grpc::TikvClient;
use kvproto::{debugpb, metapb, raft_serverpb};
use raft::eraftpb;

use test_raftstore::*;
use tikv::coprocessor::REQ_TYPE_DAG;
use tikv::raftstore::store::{keys, Mutable, Peekable};
use tikv::storage::mvcc::{Lock, LockType};
use tikv::storage::{Key, CF_DEFAULT, CF_LOCK, CF_RAFT};
use tikv::util::HandyRwLock;

fn must_new_cluster() -> (Cluster<ServerCluster>, metapb::Peer, Context) {
    let count = 1;
    let mut cluster = new_server_cluster(0, count);
    cluster.run();

    let region_id = 1;
    let leader = cluster.leader_of_region(region_id).unwrap();
    let epoch = cluster.get_region_epoch(region_id);
    let mut ctx = Context::new();
    ctx.set_region_id(region_id);
    ctx.set_peer(leader.clone());
    ctx.set_region_epoch(epoch);

    (cluster, leader, ctx)
}

fn must_new_cluster_and_kv_client() -> (Cluster<ServerCluster>, TikvClient, Context) {
    let (cluster, leader, ctx) = must_new_cluster();

    let env = Arc::new(Environment::new(1));
    let channel =
        ChannelBuilder::new(env).connect(cluster.sim.rl().get_addr(leader.get_store_id()));
    let client = TikvClient::new(channel);

    (cluster, client, ctx)
}

#[test]
fn test_rawkv() {
    let (_cluster, client, ctx) = must_new_cluster_and_kv_client();
    let (k, v) = (b"key".to_vec(), b"value".to_vec());

    // Raw put
    let mut put_req = RawPutRequest::new();
    put_req.set_context(ctx.clone());
    put_req.key = k.clone();
    put_req.value = v.clone();
    let put_resp = client.raw_put(&put_req).unwrap();
    assert!(!put_resp.has_region_error());
    assert!(put_resp.error.is_empty());

    // Raw get
    let mut get_req = RawGetRequest::new();
    get_req.set_context(ctx.clone());
    get_req.key = k.clone();
    let get_resp = client.raw_get(&get_req).unwrap();
    assert!(!get_resp.has_region_error());
    assert!(get_resp.error.is_empty());
    assert_eq!(get_resp.value, v);

    // Raw scan
    let mut scan_req = RawScanRequest::new();
    scan_req.set_context(ctx.clone());
    scan_req.start_key = k.clone();
    scan_req.limit = 1;
    let scan_resp = client.raw_scan(&scan_req).unwrap();
    assert!(!scan_resp.has_region_error());
    assert_eq!(scan_resp.kvs.len(), 1);
    for kv in scan_resp.kvs.into_iter() {
        assert!(!kv.has_error());
        assert_eq!(kv.key, k);
        assert_eq!(kv.value, v);
    }

    // Raw delete
    let mut delete_req = RawDeleteRequest::new();
    delete_req.set_context(ctx.clone());
    delete_req.key = k.clone();
    let delete_resp = client.raw_delete(&delete_req).unwrap();
    assert!(!delete_resp.has_region_error());
    assert!(delete_resp.error.is_empty());
}

fn must_kv_prewrite(client: &TikvClient, ctx: Context, muts: Vec<Mutation>, pk: Vec<u8>, ts: u64) {
    let mut prewrite_req = PrewriteRequest::new();
    prewrite_req.set_context(ctx);
    prewrite_req.set_mutations(muts.into_iter().collect());
    prewrite_req.primary_lock = pk;
    prewrite_req.start_version = ts;
    prewrite_req.lock_ttl = prewrite_req.start_version + 1;
    let prewrite_resp = client.kv_prewrite(&prewrite_req).unwrap();
    assert!(
        !prewrite_resp.has_region_error(),
        "{:?}",
        prewrite_resp.get_region_error()
    );
    assert!(
        prewrite_resp.errors.is_empty(),
        "{:?}",
        prewrite_resp.get_errors()
    );
}

fn must_kv_commit(
    client: &TikvClient,
    ctx: Context,
    keys: Vec<Vec<u8>>,
    start_ts: u64,
    commit_ts: u64,
) {
    let mut commit_req = CommitRequest::new();
    commit_req.set_context(ctx);
    commit_req.start_version = start_ts;
    commit_req.set_keys(keys.into_iter().collect());
    commit_req.commit_version = commit_ts;
    let commit_resp = client.kv_commit(&commit_req).unwrap();
    assert!(
        !commit_resp.has_region_error(),
        "{:?}",
        commit_resp.get_region_error()
    );
    assert!(!commit_resp.has_error(), "{:?}", commit_resp.get_error());
}

#[test]
fn test_mvcc_basic() {
    let (_cluster, client, ctx) = must_new_cluster_and_kv_client();
    let (k, v) = (b"key".to_vec(), b"value".to_vec());

    let mut ts = 0;

    // Prewrite
    ts += 1;
    let prewrite_start_version = ts;
    let mut mutation = Mutation::new();
    mutation.op = Op::Put;
    mutation.key = k.clone();
    mutation.value = v.clone();
    must_kv_prewrite(
        &client,
        ctx.clone(),
        vec![mutation],
        k.clone(),
        prewrite_start_version,
    );

    // Commit
    ts += 1;
    let commit_version = ts;
    must_kv_commit(
        &client,
        ctx.clone(),
        vec![k.clone()],
        prewrite_start_version,
        commit_version,
    );

    // Get
    ts += 1;
    let get_version = ts;
    let mut get_req = GetRequest::new();
    get_req.set_context(ctx.clone());
    get_req.key = k.clone();
    get_req.version = get_version;
    let get_resp = client.kv_get(&get_req).unwrap();
    assert!(!get_resp.has_region_error());
    assert!(!get_resp.has_error());
    assert_eq!(get_resp.value, v);

    // Scan
    ts += 1;
    let scan_version = ts;
    let mut scan_req = ScanRequest::new();
    scan_req.set_context(ctx.clone());
    scan_req.start_key = k.clone();
    scan_req.limit = 1;
    scan_req.version = scan_version;
    let scan_resp = client.kv_scan(&scan_req).unwrap();
    assert!(!scan_resp.has_region_error());
    assert_eq!(scan_resp.pairs.len(), 1);
    for kv in scan_resp.pairs.into_iter() {
        assert!(!kv.has_error());
        assert_eq!(kv.key, k);
        assert_eq!(kv.value, v);
    }

    // Batch get
    ts += 1;
    let batch_get_version = ts;
    let mut batch_get_req = BatchGetRequest::new();
    batch_get_req.set_context(ctx.clone());
    batch_get_req.set_keys(vec![k.clone()].into_iter().collect());
    batch_get_req.version = batch_get_version;
    let batch_get_resp = client.kv_batch_get(&batch_get_req).unwrap();
    assert_eq!(batch_get_resp.pairs.len(), 1);
    for kv in batch_get_resp.pairs.into_iter() {
        assert!(!kv.has_error());
        assert_eq!(kv.key, k);
        assert_eq!(kv.value, v);
    }
}

#[test]
fn test_mvcc_rollback_and_cleanup() {
    let (_cluster, client, ctx) = must_new_cluster_and_kv_client();
    let (k, v) = (b"key".to_vec(), b"value".to_vec());

    let mut ts = 0;

    // Prewrite
    ts += 1;
    let prewrite_start_version = ts;
    let mut mutation = Mutation::new();
    mutation.op = Op::Put;
    mutation.key = k.clone();
    mutation.value = v.clone();
    must_kv_prewrite(
        &client,
        ctx.clone(),
        vec![mutation],
        k.clone(),
        prewrite_start_version,
    );

    // Commit
    ts += 1;
    let commit_version = ts;
    must_kv_commit(
        &client,
        ctx.clone(),
        vec![k.clone()],
        prewrite_start_version,
        commit_version,
    );

    // Prewrite puts some locks.
    ts += 1;
    let prewrite_start_version2 = ts;
    let (k2, v2) = (b"key2".to_vec(), b"value2".to_vec());
    let mut mut_pri = Mutation::new();
    mut_pri.op = Op::Put;
    mut_pri.key = k2.clone();
    mut_pri.value = v2.clone();
    let mut mut_sec = Mutation::new();
    mut_sec.op = Op::Put;
    mut_sec.key = k.clone();
    mut_sec.value = b"foo".to_vec();
    must_kv_prewrite(
        &client,
        ctx.clone(),
        vec![mut_pri, mut_sec],
        k2.clone(),
        prewrite_start_version2,
    );

    // Scan lock, expects locks
    ts += 1;
    let scan_lock_max_version = ts;
    let mut scan_lock_req = ScanLockRequest::new();
    scan_lock_req.set_context(ctx.clone());
    scan_lock_req.max_version = scan_lock_max_version;
    let scan_lock_resp = client.kv_scan_lock(&scan_lock_req).unwrap();
    assert!(!scan_lock_resp.has_region_error());
    assert_eq!(scan_lock_resp.locks.len(), 2);
    for (lock, key) in scan_lock_resp
        .locks
        .into_iter()
        .zip(vec![k.clone(), k2.clone()])
    {
        assert_eq!(lock.primary_lock, k2);
        assert_eq!(lock.key, key);
        assert_eq!(lock.lock_version, prewrite_start_version2);
    }

    // Rollback
    let rollback_start_version = prewrite_start_version2;
    let mut rollback_req = BatchRollbackRequest::new();
    rollback_req.set_context(ctx.clone());
    rollback_req.start_version = rollback_start_version;
    rollback_req.set_keys(vec![k2.clone()].into_iter().collect());
    let rollback_resp = client.kv_batch_rollback(&rollback_req.clone()).unwrap();
    assert!(!rollback_resp.has_region_error());
    assert!(!rollback_resp.has_error());
    rollback_req.set_keys(vec![k.clone()].into_iter().collect());
    let rollback_resp2 = client.kv_batch_rollback(&rollback_req.clone()).unwrap();
    assert!(!rollback_resp2.has_region_error());
    assert!(!rollback_resp2.has_error());

    // Cleanup
    let cleanup_start_version = prewrite_start_version2;
    let mut cleanup_req = CleanupRequest::new();
    cleanup_req.set_context(ctx.clone());
    cleanup_req.start_version = cleanup_start_version;
    cleanup_req.set_key(k2.clone());
    let cleanup_resp = client.kv_cleanup(&cleanup_req).unwrap();
    assert!(!cleanup_resp.has_region_error());
    assert!(!cleanup_resp.has_error());

    // There should be no locks
    ts += 1;
    let scan_lock_max_version2 = ts;
    let mut scan_lock_req = ScanLockRequest::new();
    scan_lock_req.set_context(ctx.clone());
    scan_lock_req.max_version = scan_lock_max_version2;
    let scan_lock_resp = client.kv_scan_lock(&scan_lock_req).unwrap();
    assert!(!scan_lock_resp.has_region_error());
    assert_eq!(scan_lock_resp.locks.len(), 0);
}

#[test]
fn test_mvcc_resolve_lock_gc_and_delete() {
    use kvproto::kvrpcpb::*;
    use protobuf;
    let (_cluster, client, ctx) = must_new_cluster_and_kv_client();
    let (k, v) = (b"key".to_vec(), b"value".to_vec());

    let mut ts = 0;

    // Prewrite
    ts += 1;
    let prewrite_start_version = ts;
    let mut mutation = Mutation::new();
    mutation.op = Op::Put;
    mutation.key = k.clone();
    mutation.value = v.clone();
    must_kv_prewrite(
        &client,
        ctx.clone(),
        vec![mutation],
        k.clone(),
        prewrite_start_version,
    );

    // Commit
    ts += 1;
    let commit_version = ts;
    must_kv_commit(
        &client,
        ctx.clone(),
        vec![k.clone()],
        prewrite_start_version,
        commit_version,
    );

    // Prewrite puts some locks.
    ts += 1;
    let prewrite_start_version2 = ts;
    let (k2, v2) = (b"key2".to_vec(), b"value2".to_vec());
    let new_v = b"new value".to_vec();
    let mut mut_pri = Mutation::new();
    mut_pri.op = Op::Put;
    mut_pri.key = k.clone();
    mut_pri.value = new_v.clone();
    let mut mut_sec = Mutation::new();
    mut_sec.op = Op::Put;
    mut_sec.key = k2.clone();
    mut_sec.value = v2.to_vec();
    must_kv_prewrite(
        &client,
        ctx.clone(),
        vec![mut_pri, mut_sec],
        k.clone(),
        prewrite_start_version2,
    );

    // Resolve lock
    ts += 1;
    let resolve_lock_commit_version = ts;
    let mut resolve_lock_req = ResolveLockRequest::new();
    let mut temp_txninfo = TxnInfo::new();
    temp_txninfo.txn = prewrite_start_version2;
    temp_txninfo.status = resolve_lock_commit_version;
    let vec_txninfo = vec![temp_txninfo];
    resolve_lock_req.set_context(ctx.clone());
    resolve_lock_req.set_txn_infos(protobuf::RepeatedField::from_vec(vec_txninfo));
    let resolve_lock_resp = client.kv_resolve_lock(&resolve_lock_req).unwrap();
    assert!(!resolve_lock_resp.has_region_error());
    assert!(!resolve_lock_resp.has_error());

    // Get `k` at the latest ts.
    ts += 1;
    let get_version1 = ts;
    let mut get_req1 = GetRequest::new();
    get_req1.set_context(ctx.clone());
    get_req1.key = k.clone();
    get_req1.version = get_version1;
    let get_resp1 = client.kv_get(&get_req1).unwrap();
    assert!(!get_resp1.has_region_error());
    assert!(!get_resp1.has_error());
    assert_eq!(get_resp1.value, new_v);

    // GC `k` at the latest ts.
    ts += 1;
    let gc_safe_ponit = ts;
    let mut gc_req = GCRequest::new();
    gc_req.set_context(ctx.clone());
    gc_req.safe_point = gc_safe_ponit;
    let gc_resp = client.kv_gc(&gc_req).unwrap();
    assert!(!gc_resp.has_region_error());
    assert!(!gc_resp.has_error());

    // the `k` at the old ts should be none.
    let get_version2 = commit_version + 1;
    let mut get_req2 = GetRequest::new();
    get_req2.set_context(ctx.clone());
    get_req2.key = k.clone();
    get_req2.version = get_version2;
    let get_resp2 = client.kv_get(&get_req2).unwrap();
    assert!(!get_resp2.has_region_error());
    assert!(!get_resp2.has_error());
    assert_eq!(get_resp2.value, b"".to_vec());

    // Transaction debugger commands
    // MvccGetByKey
    let mut mvcc_get_by_key_req = MvccGetByKeyRequest::new();
    mvcc_get_by_key_req.set_context(ctx.clone());
    mvcc_get_by_key_req.key = k.clone();
    let mvcc_get_by_key_resp = client.mvcc_get_by_key(&mvcc_get_by_key_req).unwrap();
    assert!(!mvcc_get_by_key_resp.has_region_error());
    assert!(mvcc_get_by_key_resp.error.is_empty());
    assert!(mvcc_get_by_key_resp.has_info());
    // MvccGetByStartTs
    let mut mvcc_get_by_start_ts_req = MvccGetByStartTsRequest::new();
    mvcc_get_by_start_ts_req.set_context(ctx.clone());
    mvcc_get_by_start_ts_req.start_ts = prewrite_start_version2;
    let mvcc_get_by_start_ts_resp = client
        .mvcc_get_by_start_ts(&mvcc_get_by_start_ts_req)
        .unwrap();
    assert!(!mvcc_get_by_start_ts_resp.has_region_error());
    assert!(mvcc_get_by_start_ts_resp.error.is_empty());
    assert!(mvcc_get_by_start_ts_resp.has_info());
    assert_eq!(mvcc_get_by_start_ts_resp.key, k);

    // Delete range
    let mut del_req = DeleteRangeRequest::new();
    del_req.set_context(ctx.clone());
    del_req.start_key = b"a".to_vec();
    del_req.end_key = b"z".to_vec();
    let del_resp = client.kv_delete_range(&del_req).unwrap();
    assert!(!del_resp.has_region_error());
    assert!(del_resp.error.is_empty());
}

// raft related RPC is tested as parts of test_snapshot.rs, so skip here.

#[test]
fn test_coprocessor() {
    let (_cluster, client, _) = must_new_cluster_and_kv_client();
    // SQL push down commands
    let mut req = Request::new();
    req.set_tp(REQ_TYPE_DAG);
    client.coprocessor(&req).unwrap();
}

#[test]
fn test_split_region() {
    let (_cluster, client, ctx) = must_new_cluster_and_kv_client();

    // Split region commands
    let key = b"b";
    let mut req = SplitRegionRequest::new();
    req.set_context(ctx);
    req.set_split_key(key.to_vec());
    let resp = client.split_region(&req).unwrap();
    assert_eq!(
        Key::from_encoded(resp.get_left().get_end_key().to_vec())
            .truncate_ts()
            .unwrap()
            .as_encoded()
            .as_slice(),
        key
    );
    assert_eq!(
        resp.get_left().get_end_key(),
        resp.get_right().get_start_key()
    );
}

fn must_new_cluster_and_debug_client() -> (Cluster<ServerCluster>, DebugClient, u64) {
    let (cluster, leader, _) = must_new_cluster();

    let env = Arc::new(Environment::new(1));
    let channel =
        ChannelBuilder::new(env).connect(cluster.sim.rl().get_addr(leader.get_store_id()));
    let client = DebugClient::new(channel);

    (cluster, client, leader.get_store_id())
}

#[test]
fn test_debug_get() {
    let (cluster, debug_client, store_id) = must_new_cluster_and_debug_client();
    let (k, v) = (b"key", b"value");

    // Put some data.
    let engine = cluster.get_engine(store_id);
    let key = keys::data_key(k);
    engine.put(&key, v).unwrap();
    assert_eq!(engine.get(&key).unwrap().unwrap(), v);

    // Debug get
    let mut req = debugpb::GetRequest::new();
    req.set_cf(CF_DEFAULT.to_owned());
    req.set_db(debugpb::DB::KV);
    req.set_key(key);
    let mut resp = debug_client.get(&req.clone()).unwrap();
    assert_eq!(resp.take_value(), v);

    req.set_key(b"foo".to_vec());
    match debug_client.get(&req).unwrap_err() {
        Error::RpcFailure(status) => {
            assert_eq!(status.status, RpcStatusCode::NotFound);
        }
        _ => panic!("expect NotFound"),
    }
}

#[test]
fn test_debug_raft_log() {
    let (cluster, debug_client, store_id) = must_new_cluster_and_debug_client();

    // Put some data.
    let engine = cluster.get_raft_engine(store_id);
    let (region_id, log_index) = (200, 200);
    let key = keys::raft_log_key(region_id, log_index);
    let mut entry = eraftpb::Entry::new();
    entry.set_term(1);
    entry.set_index(1);
    entry.set_entry_type(eraftpb::EntryType::EntryNormal);
    entry.set_data(vec![42]);
    engine.put_msg(&key, &entry).unwrap();
    assert_eq!(
        engine.get_msg::<eraftpb::Entry>(&key).unwrap().unwrap(),
        entry
    );

    // Debug raft_log
    let mut req = debugpb::RaftLogRequest::new();
    req.set_region_id(region_id);
    req.set_log_index(log_index);
    let resp = debug_client.raft_log(&req).unwrap();
    assert_ne!(resp.get_entry(), &eraftpb::Entry::new());

    let mut req = debugpb::RaftLogRequest::new();
    req.set_region_id(region_id + 1);
    req.set_log_index(region_id + 1);
    match debug_client.raft_log(&req).unwrap_err() {
        Error::RpcFailure(status) => {
            assert_eq!(status.status, RpcStatusCode::NotFound);
        }
        _ => panic!("expect NotFound"),
    }
}

#[test]
fn test_debug_region_info() {
    let (cluster, debug_client, store_id) = must_new_cluster_and_debug_client();

    let raft_engine = cluster.get_raft_engine(store_id);
    let kv_engine = cluster.get_engine(store_id);
    let raft_cf = kv_engine.cf_handle(CF_RAFT).unwrap();

    let region_id = 100;
    let raft_state_key = keys::raft_state_key(region_id);
    let mut raft_state = raft_serverpb::RaftLocalState::new();
    raft_state.set_last_index(42);
    raft_engine.put_msg(&raft_state_key, &raft_state).unwrap();
    assert_eq!(
        raft_engine
            .get_msg::<raft_serverpb::RaftLocalState>(&raft_state_key)
            .unwrap()
            .unwrap(),
        raft_state
    );

    let apply_state_key = keys::apply_state_key(region_id);
    let mut apply_state = raft_serverpb::RaftApplyState::new();
    apply_state.set_applied_index(42);
    kv_engine
        .put_msg_cf(raft_cf, &apply_state_key, &apply_state)
        .unwrap();
    assert_eq!(
        kv_engine
            .get_msg_cf::<raft_serverpb::RaftApplyState>(CF_RAFT, &apply_state_key)
            .unwrap()
            .unwrap(),
        apply_state
    );

    let region_state_key = keys::region_state_key(region_id);
    let mut region_state = raft_serverpb::RegionLocalState::new();
    region_state.set_state(raft_serverpb::PeerState::Tombstone);
    kv_engine
        .put_msg_cf(raft_cf, &region_state_key, &region_state)
        .unwrap();
    assert_eq!(
        kv_engine
            .get_msg_cf::<raft_serverpb::RegionLocalState>(CF_RAFT, &region_state_key)
            .unwrap()
            .unwrap(),
        region_state
    );

    // Debug region_info
    let mut req = debugpb::RegionInfoRequest::new();
    req.set_region_id(region_id);
    let mut resp = debug_client.region_info(&req.clone()).unwrap();
    assert_eq!(resp.take_raft_local_state(), raft_state);
    assert_eq!(resp.take_raft_apply_state(), apply_state);
    assert_eq!(resp.take_region_local_state(), region_state);

    req.set_region_id(region_id + 1);
    match debug_client.region_info(&req).unwrap_err() {
        Error::RpcFailure(status) => {
            assert_eq!(status.status, RpcStatusCode::NotFound);
        }
        _ => panic!("expect NotFound"),
    }
}

#[test]
fn test_debug_region_size() {
    let (cluster, debug_client, store_id) = must_new_cluster_and_debug_client();
    let engine = cluster.get_engine(store_id);

    // Put some data.
    let region_id = 100;
    let region_state_key = keys::region_state_key(region_id);
    let mut region = metapb::Region::new();
    region.set_id(region_id);
    region.set_start_key(b"a".to_vec());
    region.set_end_key(b"z".to_vec());
    let mut state = RegionLocalState::new();
    state.set_region(region);
    let cf_raft = engine.cf_handle(CF_RAFT).unwrap();
    engine
        .put_msg_cf(cf_raft, &region_state_key, &state)
        .unwrap();

    let cfs = vec![CF_DEFAULT, CF_LOCK, CF_RAFT];
    let (k, v) = (keys::data_key(b"k"), b"v");
    for cf in &cfs {
        let cf_handle = engine.cf_handle(cf).unwrap();
        engine.put_cf(cf_handle, k.as_slice(), v).unwrap();
    }

    let mut req = debugpb::RegionSizeRequest::new();
    req.set_region_id(region_id);
    req.set_cfs(cfs.iter().map(|s| s.to_string()).collect());
    let entries = debug_client.region_size(&req).unwrap().take_entries();
    assert_eq!(entries.len(), 3);
    for e in entries.into_vec() {
        cfs.iter().find(|&&c| c == e.cf).unwrap();
        assert!(e.size > 0);
    }

    req.set_region_id(region_id + 1);
    match debug_client.region_size(&req).unwrap_err() {
        Error::RpcFailure(status) => {
            assert_eq!(status.status, RpcStatusCode::NotFound);
        }
        _ => panic!("expect NotFound"),
    }
}

#[test]
#[cfg(not(feature = "no-fail"))]
fn test_debug_fail_point() {
    let (_cluster, debug_client, _) = must_new_cluster_and_debug_client();

    let (fp, act) = ("raft_between_save", "off");

    let mut inject_req = debugpb::InjectFailPointRequest::new();
    inject_req.set_name(fp.to_owned());
    inject_req.set_actions(act.to_owned());
    debug_client.inject_fail_point(&inject_req).unwrap();

    let resp = debug_client
        .list_fail_points(&debugpb::ListFailPointsRequest::new())
        .unwrap();
    let entries = resp.get_entries();
    assert_eq!(entries.len(), 1);
    for e in entries {
        assert_eq!(e.get_name(), fp);
        assert_eq!(e.get_actions(), act);
    }

    let mut recover_req = debugpb::RecoverFailPointRequest::new();
    recover_req.set_name(fp.to_owned());
    debug_client.recover_fail_point(&recover_req).unwrap();

    let resp = debug_client
        .list_fail_points(&debugpb::ListFailPointsRequest::new())
        .unwrap();
    let entries = resp.get_entries();
    assert_eq!(entries.len(), 0);
}

#[test]
fn test_debug_scan_mvcc() {
    let (cluster, debug_client, store_id) = must_new_cluster_and_debug_client();
    let engine = cluster.get_engine(store_id);

    // Put some data.
    let keys = [
        keys::data_key(b"meta_lock_1"),
        keys::data_key(b"meta_lock_2"),
    ];
    for k in &keys {
        let v = Lock::new(LockType::Put, b"pk".to_vec(), 1, 10, None).to_bytes();
        let cf_handle = engine.cf_handle(CF_LOCK).unwrap();
        engine.put_cf(cf_handle, k.as_slice(), &v).unwrap();
    }

    let mut req = debugpb::ScanMvccRequest::new();
    req.set_from_key(keys::data_key(b"m"));
    req.set_to_key(keys::data_key(b"n"));
    req.set_limit(1);

    let receiver = debug_client.scan_mvcc(&req).unwrap();
    let future = receiver.fold(Vec::new(), |mut keys, mut resp| {
        let key = resp.take_key();
        keys.push(key);
        future::ok::<_, Error>(keys)
    });
    let keys = future.wait().unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0], keys::data_key(b"meta_lock_1"));
}
