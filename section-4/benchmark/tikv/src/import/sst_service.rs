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

use std::sync::{Arc, Mutex};

use crate::grpc::{ClientStreamingSink, RequestStream, RpcContext, UnarySink};
use futures::sync::mpsc;
use futures::{future, Future, Stream};
use futures_cpupool::{Builder, CpuPool};
use kvproto::import_sstpb::*;
use kvproto::import_sstpb_grpc::*;
use kvproto::raft_cmdpb::*;

use crate::raftstore::store::Callback;
use crate::server::transport::RaftStoreRouter;
use crate::storage::engine::DB;
use crate::util::future::paired_future_callback;
use crate::util::rocksdb_util::compact_files_in_range;
use crate::util::time::Instant;

use super::import_mode::*;
use super::metrics::*;
use super::service::*;
use super::{Config, Error, SSTImporter};

/// ImportSSTService provides tikv-server with the ability to ingest SST files.
///
/// It saves the SST sent from client to a file and then sends a command to
/// raftstore to trigger the ingest process.
#[derive(Clone)]
pub struct ImportSSTService<Router> {
    cfg: Config,
    router: Router,
    engine: Arc<DB>,
    threads: CpuPool,
    importer: Arc<SSTImporter>,
    switcher: Arc<Mutex<ImportModeSwitcher>>,
}

impl<Router: RaftStoreRouter> ImportSSTService<Router> {
    pub fn new(
        cfg: Config,
        router: Router,
        engine: Arc<DB>,
        importer: Arc<SSTImporter>,
    ) -> ImportSSTService<Router> {
        let threads = Builder::new()
            .name_prefix("sst-importer")
            .pool_size(cfg.num_threads)
            .create();
        ImportSSTService {
            cfg,
            router,
            engine,
            threads,
            importer,
            switcher: Arc::new(Mutex::new(ImportModeSwitcher::new())),
        }
    }
}

impl<Router: RaftStoreRouter> ImportSst for ImportSSTService<Router> {
    fn switch_mode(
        &mut self,
        ctx: RpcContext<'_>,
        req: SwitchModeRequest,
        sink: UnarySink<SwitchModeResponse>,
    ) {
        let label = "switch_mode";
        let timer = Instant::now_coarse();

        let res = {
            let mut switcher = self.switcher.lock().unwrap();
            match req.get_mode() {
                SwitchMode::Normal => switcher.enter_normal_mode(&self.engine),
                SwitchMode::Import => switcher.enter_import_mode(&self.engine),
            }
        };
        match res {
            Ok(_) => info!("switch mode"; "mode" => ?req.get_mode()),
            Err(ref e) => error!("switch mode failed"; "mode" => ?req.get_mode(), "err" => %e),
        }

        ctx.spawn(
            future::result(res)
                .map(|_| SwitchModeResponse::new())
                .then(move |res| send_rpc_response!(res, sink, label, timer)),
        )
    }

    /// Receive SST from client and save the file for later ingesting.
    fn upload(
        &mut self,
        ctx: RpcContext<'_>,
        stream: RequestStream<UploadRequest>,
        sink: ClientStreamingSink<UploadResponse>,
    ) {
        let label = "upload";
        let timer = Instant::now_coarse();
        let import = Arc::clone(&self.importer);
        let bounded_stream = mpsc::spawn(stream, &self.threads, self.cfg.stream_channel_window);

        ctx.spawn(
            self.threads.spawn(
                bounded_stream
                    .into_future()
                    .map_err(|(e, _)| Error::from(e))
                    .and_then(move |(chunk, stream)| {
                        // The first message of the stream contains metadata
                        // of the file.
                        let meta = match chunk {
                            Some(ref chunk) if chunk.has_meta() => chunk.get_meta(),
                            _ => return Err(Error::InvalidChunk),
                        };
                        let file = import.create(meta)?;
                        Ok((file, stream))
                    })
                    .and_then(move |(file, stream)| {
                        stream
                            .map_err(Error::from)
                            .fold(file, |mut file, chunk| {
                                let start = Instant::now_coarse();
                                let data = chunk.get_data();
                                if data.is_empty() {
                                    return future::err(Error::InvalidChunk);
                                }
                                if let Err(e) = file.append(data) {
                                    return future::err(e);
                                }
                                IMPORT_UPLOAD_CHUNK_BYTES.observe(data.len() as f64);
                                IMPORT_UPLOAD_CHUNK_DURATION.observe(start.elapsed_secs());
                                future::ok(file)
                            })
                            .and_then(|mut file| file.finish())
                    })
                    .map(|_| UploadResponse::new())
                    .then(move |res| send_rpc_response!(res, sink, label, timer)),
            ),
        )
    }

    /// Ingest the file by sending a raft command to raftstore.
    ///
    /// If the ingestion fails because the region is not found or the epoch does
    /// not match, the remaining files will eventually be cleaned up by
    /// CleanupSSTWorker.
    fn ingest(
        &mut self,
        ctx: RpcContext<'_>,
        mut req: IngestRequest,
        sink: UnarySink<IngestResponse>,
    ) {
        let label = "ingest";
        let timer = Instant::now_coarse();

        // Make ingest command.
        let mut ingest = Request::new();
        ingest.set_cmd_type(CmdType::IngestSST);
        ingest.mut_ingest_sst().set_sst(req.take_sst());
        let mut context = req.take_context();
        let mut header = RaftRequestHeader::new();
        header.set_peer(context.take_peer());
        header.set_region_id(context.get_region_id());
        header.set_region_epoch(context.take_region_epoch());
        let mut cmd = RaftCmdRequest::new();
        cmd.set_header(header);
        cmd.mut_requests().push(ingest);

        let (cb, future) = paired_future_callback();
        if let Err(e) = self.router.send_command(cmd, Callback::Write(cb)) {
            return send_rpc_error(ctx, sink, e);
        }

        ctx.spawn(
            future
                .map_err(Error::from)
                .then(|res| match res {
                    Ok(mut res) => {
                        let mut resp = IngestResponse::new();
                        let mut header = res.response.take_header();
                        if header.has_error() {
                            resp.set_error(header.take_error());
                        }
                        future::ok(resp)
                    }
                    Err(e) => future::err(e),
                })
                .then(move |res| send_rpc_response!(res, sink, label, timer)),
        )
    }

    fn compact(
        &mut self,
        ctx: RpcContext<'_>,
        req: CompactRequest,
        sink: UnarySink<CompactResponse>,
    ) {
        let label = "compact";
        let timer = Instant::now_coarse();
        let engine = Arc::clone(&self.engine);

        ctx.spawn(self.threads.spawn_fn(move || {
            let (start, end) = if !req.has_range() {
                (None, None)
            } else {
                (
                    Some(req.get_range().get_start()),
                    Some(req.get_range().get_end()),
                )
            };
            let output_level = if req.get_output_level() == -1 {
                None
            } else {
                Some(req.get_output_level())
            };

            let res = compact_files_in_range(&engine, start, end, output_level);
            match res {
                Ok(_) => info!(
                    "compact files in range";
                    "start" => start.map(log_wrappers::Key),
                    "end" => end.map(log_wrappers::Key),
                    "output_level" => ?output_level, "takes" => ?timer.elapsed()
                ),
                Err(ref e) => error!(
                    "compact files in range failed";
                    "start" => start.map(log_wrappers::Key),
                    "end" => end.map(log_wrappers::Key),
                    "output_level" => ?output_level, "err" => %e
                ),
            }

            future::result(res)
                .map_err(Error::from)
                .map(|_| CompactResponse::new())
                .then(move |res| send_rpc_response!(res, sink, label, timer))
        }))
    }
}
