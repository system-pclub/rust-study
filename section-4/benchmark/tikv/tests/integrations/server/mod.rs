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

mod kv_service;
mod raft_client;

use std::sync::Arc;

use crate::grpc::*;
use futures::Future;
use kvproto::coprocessor::*;
use kvproto::kvrpcpb::*;
use kvproto::raft_serverpb::{Done, RaftMessage, SnapshotChunk};
use kvproto::tikvpb::{BatchCommandsRequest, BatchCommandsResponse, BatchRaftMessage};
use kvproto::tikvpb_grpc::{create_tikv, Tikv};
use tikv::util::security::{SecurityConfig, SecurityManager};

macro_rules! unary_call {
    ($name:tt, $req_name:tt, $resp_name:tt) => {
        fn $name(&mut self, ctx: RpcContext<'_>, _: $req_name, sink: UnarySink<$resp_name>) {
            let status = RpcStatus::new(RpcStatusCode::Unimplemented, None);
            ctx.spawn(sink.fail(status).map_err(|_| ()));
        }
    }
}

macro_rules! sstream_call {
    ($name:tt, $req_name:tt, $resp_name:tt) => {
        fn $name(&mut self, ctx: RpcContext<'_>, _: $req_name, sink: ServerStreamingSink<$resp_name>) {
            let status = RpcStatus::new(RpcStatusCode::Unimplemented, None);
            ctx.spawn(sink.fail(status).map_err(|_| ()));
        }
    }
}

macro_rules! cstream_call {
    ($name:tt, $req_name:tt, $resp_name:tt) => {
        fn $name(&mut self, ctx: RpcContext<'_>, _: RequestStream<$req_name>, sink: ClientStreamingSink<$resp_name>) {
            let status = RpcStatus::new(RpcStatusCode::Unimplemented, None);
            ctx.spawn(sink.fail(status).map_err(|_| ()));
        }
    }
}

macro_rules! bstream_call {
    ($name:tt, $req_name:tt, $resp_name:tt) => {
        fn $name(&mut self, ctx: RpcContext<'_>, _: RequestStream<$req_name>, sink: DuplexSink<$resp_name>) {
            let status = RpcStatus::new(RpcStatusCode::Unimplemented, None);
            ctx.spawn(sink.fail(status).map_err(|_| ()));
        }
    }
}

macro_rules! unary_call_dispatch {
    ($name:tt, $req_name:tt, $resp_name:tt) => {
        fn $name(&mut self, ctx: RpcContext<'_>, req: $req_name, sink: UnarySink<$resp_name>) {
            (self.0).$name(ctx, req, sink)
        }
    }
}

macro_rules! sstream_call_dispatch {
    ($name:tt, $req_name:tt, $resp_name:tt) => {
        fn $name(&mut self, ctx: RpcContext<'_>, req: $req_name, sink: ServerStreamingSink<$resp_name>) {
            (self.0).$name(ctx, req, sink)
        }
    }
}

macro_rules! cstream_call_dispatch {
    ($name:tt, $req_name:tt, $resp_name:tt) => {
        fn $name(&mut self, ctx: RpcContext<'_>, req: RequestStream<$req_name>, sink: ClientStreamingSink<$resp_name>) {
            (self.0).$name(ctx, req, sink)
        }
    }
}

macro_rules! bstream_call_dispatch {
    ($name:tt, $req_name:tt, $resp_name:tt) => {
        fn $name(&mut self, ctx: RpcContext<'_>, req: RequestStream<$req_name>, sink: DuplexSink<$resp_name>) {
            (self.0).$name(ctx, req, sink)
        }
    }
}

#[derive(Clone)]
struct MockKv<T>(pub T);

trait MockKvService {
    unary_call!(kv_get, GetRequest, GetResponse);
    unary_call!(kv_scan, ScanRequest, ScanResponse);
    unary_call!(kv_prewrite, PrewriteRequest, PrewriteResponse);
    unary_call!(kv_commit, CommitRequest, CommitResponse);
    unary_call!(kv_import, ImportRequest, ImportResponse);
    unary_call!(kv_cleanup, CleanupRequest, CleanupResponse);
    unary_call!(kv_batch_get, BatchGetRequest, BatchGetResponse);
    unary_call!(
        kv_batch_rollback,
        BatchRollbackRequest,
        BatchRollbackResponse
    );
    unary_call!(kv_scan_lock, ScanLockRequest, ScanLockResponse);
    unary_call!(kv_resolve_lock, ResolveLockRequest, ResolveLockResponse);
    unary_call!(kv_gc, GCRequest, GCResponse);
    unary_call!(kv_delete_range, DeleteRangeRequest, DeleteRangeResponse);
    unary_call!(raw_get, RawGetRequest, RawGetResponse);
    unary_call!(raw_batch_get, RawBatchGetRequest, RawBatchGetResponse);
    unary_call!(raw_scan, RawScanRequest, RawScanResponse);
    unary_call!(raw_batch_scan, RawBatchScanRequest, RawBatchScanResponse);
    unary_call!(raw_put, RawPutRequest, RawPutResponse);
    unary_call!(raw_batch_put, RawBatchPutRequest, RawBatchPutResponse);
    unary_call!(raw_delete, RawDeleteRequest, RawDeleteResponse);
    unary_call!(
        raw_batch_delete,
        RawBatchDeleteRequest,
        RawBatchDeleteResponse
    );
    unary_call!(
        raw_delete_range,
        RawDeleteRangeRequest,
        RawDeleteRangeResponse
    );
    unary_call!(
        unsafe_destroy_range,
        UnsafeDestroyRangeRequest,
        UnsafeDestroyRangeResponse
    );
    unary_call!(coprocessor, Request, Response);
    sstream_call!(coprocessor_stream, Request, Response);
    cstream_call!(raft, RaftMessage, Done);
    cstream_call!(batch_raft, BatchRaftMessage, Done);
    cstream_call!(snapshot, SnapshotChunk, Done);
    unary_call!(
        mvcc_get_by_start_ts,
        MvccGetByStartTsRequest,
        MvccGetByStartTsResponse
    );
    unary_call!(mvcc_get_by_key, MvccGetByKeyRequest, MvccGetByKeyResponse);
    unary_call!(split_region, SplitRegionRequest, SplitRegionResponse);
    bstream_call!(batch_commands, BatchCommandsRequest, BatchCommandsResponse);
}

impl<T: MockKvService + Clone + Send + 'static> Tikv for MockKv<T> {
    unary_call_dispatch!(kv_get, GetRequest, GetResponse);
    unary_call_dispatch!(kv_scan, ScanRequest, ScanResponse);
    unary_call_dispatch!(kv_prewrite, PrewriteRequest, PrewriteResponse);
    unary_call_dispatch!(kv_commit, CommitRequest, CommitResponse);
    unary_call_dispatch!(kv_import, ImportRequest, ImportResponse);
    unary_call_dispatch!(kv_cleanup, CleanupRequest, CleanupResponse);
    unary_call_dispatch!(kv_batch_get, BatchGetRequest, BatchGetResponse);
    unary_call_dispatch!(
        kv_batch_rollback,
        BatchRollbackRequest,
        BatchRollbackResponse
    );
    unary_call_dispatch!(kv_scan_lock, ScanLockRequest, ScanLockResponse);
    unary_call_dispatch!(kv_resolve_lock, ResolveLockRequest, ResolveLockResponse);
    unary_call_dispatch!(kv_gc, GCRequest, GCResponse);
    unary_call_dispatch!(kv_delete_range, DeleteRangeRequest, DeleteRangeResponse);
    unary_call_dispatch!(raw_get, RawGetRequest, RawGetResponse);
    unary_call_dispatch!(raw_batch_get, RawBatchGetRequest, RawBatchGetResponse);
    unary_call_dispatch!(raw_scan, RawScanRequest, RawScanResponse);
    unary_call_dispatch!(raw_batch_scan, RawBatchScanRequest, RawBatchScanResponse);
    unary_call_dispatch!(raw_put, RawPutRequest, RawPutResponse);
    unary_call_dispatch!(raw_batch_put, RawBatchPutRequest, RawBatchPutResponse);
    unary_call_dispatch!(raw_delete, RawDeleteRequest, RawDeleteResponse);
    unary_call_dispatch!(
        raw_batch_delete,
        RawBatchDeleteRequest,
        RawBatchDeleteResponse
    );
    unary_call_dispatch!(
        raw_delete_range,
        RawDeleteRangeRequest,
        RawDeleteRangeResponse
    );
    unary_call_dispatch!(
        unsafe_destroy_range,
        UnsafeDestroyRangeRequest,
        UnsafeDestroyRangeResponse
    );
    unary_call_dispatch!(coprocessor, Request, Response);
    sstream_call_dispatch!(coprocessor_stream, Request, Response);
    cstream_call_dispatch!(raft, RaftMessage, Done);
    cstream_call_dispatch!(batch_raft, BatchRaftMessage, Done);
    cstream_call_dispatch!(snapshot, SnapshotChunk, Done);
    unary_call_dispatch!(
        mvcc_get_by_start_ts,
        MvccGetByStartTsRequest,
        MvccGetByStartTsResponse
    );
    unary_call!(mvcc_get_by_key, MvccGetByKeyRequest, MvccGetByKeyResponse);
    unary_call_dispatch!(split_region, SplitRegionRequest, SplitRegionResponse);
    bstream_call_dispatch!(batch_commands, BatchCommandsRequest, BatchCommandsResponse);
}

fn mock_kv_service<T>(kv: MockKv<T>, ip: &str, port: u16) -> Result<Server>
where
    T: MockKvService + Clone + Send + 'static,
{
    let env = Arc::new(Environment::new(2));
    let security_mgr = Arc::new(SecurityManager::new(&SecurityConfig::default()).unwrap());

    let channel_args = ChannelBuilder::new(Arc::clone(&env))
        .max_concurrent_stream(2)
        .max_receive_message_len(-1)
        .max_send_message_len(-1)
        .build_args();

    let mut sb = ServerBuilder::new(Arc::clone(&env))
        .channel_args(channel_args)
        .register_service(create_tikv(kv));
    sb = security_mgr.bind(sb, ip, port);
    sb.build()
}
