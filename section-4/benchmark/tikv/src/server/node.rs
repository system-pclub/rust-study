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

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use super::transport::RaftStoreRouter;
use super::Result;
use crate::import::SSTImporter;
use crate::pd::{Error as PdError, PdClient, PdTask, INVALID_ID};
use crate::raftstore::coprocessor::dispatcher::CoprocessorHost;
use crate::raftstore::store::fsm::{RaftBatchSystem, RaftRouter};
use crate::raftstore::store::{
    self, initial_region, keys, Config as StoreConfig, Engines, Peekable, ReadTask, SnapManager,
    Transport,
};
use crate::server::readpool::ReadPool;
use crate::server::Config as ServerConfig;
use crate::server::ServerRaftStoreRouter;
use crate::storage::engine::DB;
use crate::storage::{self, Config as StorageConfig, RaftKv, Storage};
use crate::util::worker::{FutureWorker, Worker};
use kvproto::metapb;
use kvproto::raft_serverpb::StoreIdent;
use protobuf::RepeatedField;

const MAX_CHECK_CLUSTER_BOOTSTRAPPED_RETRY_COUNT: u64 = 60;
const CHECK_CLUSTER_BOOTSTRAPPED_RETRY_SECONDS: u64 = 3;

/// Creates a new storage engine which is backed by the Raft consensus
/// protocol.
pub fn create_raft_storage<S>(
    router: S,
    cfg: &StorageConfig,
    read_pool: ReadPool<storage::ReadPoolContext>,
    local_storage: Option<Arc<DB>>,
    raft_store_router: Option<ServerRaftStoreRouter>,
) -> Result<Storage<RaftKv<S>>>
where
    S: RaftStoreRouter + 'static,
{
    let engine = RaftKv::new(router);
    let store = Storage::from_engine(engine, cfg, read_pool, local_storage, raft_store_router)?;
    Ok(store)
}

/// A wrapper for the raftstore which runs Multi-Raft.
// TODO: we will rename another better name like RaftStore later.
pub struct Node<C: PdClient + 'static> {
    cluster_id: u64,
    store: metapb::Store,
    store_cfg: StoreConfig,
    store_handle: Option<thread::JoinHandle<()>>,
    system: RaftBatchSystem,

    pd_client: Arc<C>,
}

impl<C> Node<C>
where
    C: PdClient,
{
    /// Creates a new Node.
    pub fn new(
        system: RaftBatchSystem,
        cfg: &ServerConfig,
        store_cfg: &StoreConfig,
        pd_client: Arc<C>,
    ) -> Node<C> {
        let mut store = metapb::Store::new();
        store.set_id(INVALID_ID);
        if cfg.advertise_addr.is_empty() {
            store.set_address(cfg.addr.clone());
        } else {
            store.set_address(cfg.advertise_addr.clone())
        }
        store.set_version(env!("CARGO_PKG_VERSION").to_string());

        let mut labels = Vec::new();
        for (k, v) in &cfg.labels {
            let mut label = metapb::StoreLabel::new();
            label.set_key(k.to_owned());
            label.set_value(v.to_owned());
            labels.push(label);
        }
        store.set_labels(RepeatedField::from_vec(labels));

        Node {
            cluster_id: cfg.cluster_id,
            store,
            store_cfg: store_cfg.clone(),
            store_handle: None,
            pd_client,
            system,
        }
    }

    /// Starts the Node. It tries to bootstrap cluster if the cluster is not
    /// bootstrapped yet. Then it spawns a thread to run the raftstore in
    /// background.
    #[allow(clippy::too_many_arguments)]
    pub fn start<T>(
        &mut self,
        engines: Engines,
        trans: T,
        snap_mgr: SnapManager,
        pd_worker: FutureWorker<PdTask>,
        local_read_worker: Worker<ReadTask>,
        coprocessor_host: CoprocessorHost,
        importer: Arc<SSTImporter>,
    ) -> Result<()>
    where
        T: Transport + 'static,
    {
        let mut store_id = self.check_store(&engines)?;
        if store_id == INVALID_ID {
            store_id = self.bootstrap_store(&engines)?;
            fail_point!("node_after_bootstrap_store", |_| Err(box_err!(
                "injected error: node_after_bootstrap_store"
            )));
        }
        self.store.set_id(store_id);

        if let Some(first_region) = self.check_or_prepare_bootstrap_cluster(&engines, store_id)? {
            info!("try bootstrap cluster"; "store_id" => store_id, "region" => ?first_region);
            // cluster is not bootstrapped, and we choose first store to bootstrap
            fail_point!("node_after_prepare_bootstrap_cluster", |_| Err(box_err!(
                "injected error: node_after_prepare_bootstrap_cluster"
            )));
            self.bootstrap_cluster(&engines, first_region)?;
        }

        // Put store only if the cluster is bootstrapped.
        self.pd_client.put_store(self.store.clone())?;

        self.start_store(
            store_id,
            engines,
            trans,
            snap_mgr,
            pd_worker,
            local_read_worker,
            coprocessor_host,
            importer,
        )?;
        Ok(())
    }

    /// Gets the store id.
    pub fn id(&self) -> u64 {
        self.store.get_id()
    }

    /// Gets a transmission end of a channel which is used to send `Msg` to the
    /// raftstore.
    pub fn get_router(&self) -> RaftRouter {
        self.system.router()
    }

    // check store, return store id for the engine.
    // If the store is not bootstrapped, use INVALID_ID.
    fn check_store(&self, engines: &Engines) -> Result<u64> {
        let res = engines.kv.get_msg::<StoreIdent>(keys::STORE_IDENT_KEY)?;
        if res.is_none() {
            return Ok(INVALID_ID);
        }

        let ident = res.unwrap();
        if ident.get_cluster_id() != self.cluster_id {
            return Err(box_err!(
                "cluster ID mismatch, local {} != remote {}, \
                 you are trying to connect to another cluster, please reconnect to the correct PD",
                ident.get_cluster_id(),
                self.cluster_id
            ));
        }

        let store_id = ident.get_store_id();
        if store_id == INVALID_ID {
            return Err(box_err!("invalid store ident {:?}", ident));
        }
        Ok(store_id)
    }

    fn alloc_id(&self) -> Result<u64> {
        let id = self.pd_client.alloc_id()?;
        Ok(id)
    }

    fn bootstrap_store(&self, engines: &Engines) -> Result<u64> {
        let store_id = self.alloc_id()?;
        info!("alloc store id"; "store_id" => store_id);

        store::bootstrap_store(engines, self.cluster_id, store_id)?;

        Ok(store_id)
    }

    // Exported for tests.
    #[doc(hidden)]
    pub fn prepare_bootstrap_cluster(
        &self,
        engines: &Engines,
        store_id: u64,
    ) -> Result<metapb::Region> {
        let region_id = self.alloc_id()?;
        info!(
            "alloc first region id";
            "region_id" => region_id,
            "cluster_id" => self.cluster_id,
            "store_id" => store_id
        );
        let peer_id = self.alloc_id()?;
        info!(
            "alloc first peer id for first region";
            "peer_id" => peer_id,
            "region_id" => region_id,
        );

        let region = initial_region(store_id, region_id, peer_id);
        store::prepare_bootstrap_cluster(engines, &region)?;
        Ok(region)
    }

    fn check_or_prepare_bootstrap_cluster(
        &self,
        engines: &Engines,
        store_id: u64,
    ) -> Result<Option<metapb::Region>> {
        if let Some(first_region) = engines.kv.get_msg(keys::PREPARE_BOOTSTRAP_KEY)? {
            Ok(Some(first_region))
        } else {
            if self.check_cluster_bootstrapped()? {
                Ok(None)
            } else {
                self.prepare_bootstrap_cluster(engines, store_id).map(Some)
            }
        }
    }

    fn bootstrap_cluster(&mut self, engines: &Engines, first_region: metapb::Region) -> Result<()> {
        let region_id = first_region.get_id();
        let mut retry = 0;
        while retry < MAX_CHECK_CLUSTER_BOOTSTRAPPED_RETRY_COUNT {
            match self
                .pd_client
                .bootstrap_cluster(self.store.clone(), first_region.clone())
            {
                Ok(_) => {
                    info!("bootstrap cluster ok"; "cluster_id" => self.cluster_id);
                    fail_point!("node_after_bootstrap_cluster", |_| Err(box_err!(
                        "injected error: node_after_prepare_bootstrap_cluster"
                    )));
                    store::clear_prepare_bootstrap_key(engines)?;
                    return Ok(());
                }
                Err(PdError::ClusterBootstrapped(_)) => match self.pd_client.get_region(b"") {
                    Ok(region) => {
                        if region == first_region {
                            store::clear_prepare_bootstrap_key(engines)?;
                            return Ok(());
                        } else {
                            info!("cluster is already bootstrapped"; "cluster_id" => self.cluster_id);
                            store::clear_prepare_bootstrap_cluster(engines, region_id)?;
                            return Ok(());
                        }
                    }
                    Err(e) => {
                        warn!("get the first region failed"; "err" => ?e);
                    }
                },
                // TODO: should we clean region for other errors too?
                Err(e) => {
                    error!("bootstrap cluster"; "cluster_id" => self.cluster_id, "error" => ?e)
                }
            }
            retry += 1;
            thread::sleep(Duration::from_secs(
                CHECK_CLUSTER_BOOTSTRAPPED_RETRY_SECONDS,
            ));
        }
        Err(box_err!("bootstrapped cluster failed"))
    }

    fn check_cluster_bootstrapped(&self) -> Result<bool> {
        for _ in 0..MAX_CHECK_CLUSTER_BOOTSTRAPPED_RETRY_COUNT {
            match self.pd_client.is_cluster_bootstrapped() {
                Ok(b) => return Ok(b),
                Err(e) => {
                    warn!("check cluster bootstrapped failed"; "err" => ?e);
                }
            }
            thread::sleep(Duration::from_secs(
                CHECK_CLUSTER_BOOTSTRAPPED_RETRY_SECONDS,
            ));
        }
        Err(box_err!("check cluster bootstrapped failed"))
    }

    #[allow(clippy::too_many_arguments)]
    fn start_store<T>(
        &mut self,
        store_id: u64,
        engines: Engines,
        trans: T,
        snap_mgr: SnapManager,
        pd_worker: FutureWorker<PdTask>,
        local_read_worker: Worker<ReadTask>,
        coprocessor_host: CoprocessorHost,
        importer: Arc<SSTImporter>,
    ) -> Result<()>
    where
        T: Transport + 'static,
    {
        info!("start raft store thread"; "store_id" => store_id);

        if self.store_handle.is_some() {
            return Err(box_err!("{} is already started", store_id));
        }

        let cfg = self.store_cfg.clone();
        let pd_client = Arc::clone(&self.pd_client);
        let store = self.store.clone();
        self.system.spawn(
            store,
            cfg,
            engines,
            trans,
            pd_client,
            snap_mgr,
            pd_worker,
            local_read_worker,
            coprocessor_host,
            importer,
        )?;
        Ok(())
    }

    fn stop_store(&mut self, store_id: u64) {
        info!("stop raft store thread"; "store_id" => store_id);
        self.system.shutdown();
    }

    /// Stops the Node.
    pub fn stop(&mut self) {
        let store_id = self.store.get_id();
        self.stop_store(store_id)
    }
}
