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

use std::fmt::{self, Display, Formatter};
use std::sync::Arc;

use futures::Future;
use tokio_core::reactor::Handle;

use fs2;
use kvproto::metapb;
use kvproto::pdpb;
use kvproto::raft_cmdpb::{AdminCmdType, AdminRequest, RaftCmdRequest, SplitRequest};
use kvproto::raft_serverpb::RaftMessage;
use protobuf::RepeatedField;
use raft::eraftpb::ConfChangeType;

use super::metrics::*;
use crate::pd::{Error, PdClient, RegionStat};
use crate::raftstore::store::cmd_resp::new_error;
use crate::raftstore::store::util::KeysInfoFormatter;
use crate::raftstore::store::util::{
    get_region_approximate_keys, get_region_approximate_size, is_epoch_stale,
};
use crate::raftstore::store::Callback;
use crate::raftstore::store::StoreInfo;
use crate::raftstore::store::{CasualMessage, PeerMsg, RaftCommand, RaftRouter};
use crate::storage::engine::DB;
use crate::storage::FlowStatistics;
use crate::util::collections::HashMap;
use crate::util::escape;
use crate::util::rocksdb_util::*;
use crate::util::time::time_now_sec;
use crate::util::worker::{FutureRunnable as Runnable, FutureScheduler as Scheduler, Stopped};
use prometheus::local::LocalHistogram;

/// Uses an asynchronous thread to tell PD something.
pub enum Task {
    AskSplit {
        region: metapb::Region,
        split_key: Vec<u8>,
        peer: metapb::Peer,
        // If true, right Region derives origin region_id.
        right_derive: bool,
        callback: Callback,
    },
    AskBatchSplit {
        region: metapb::Region,
        split_keys: Vec<Vec<u8>>,
        peer: metapb::Peer,
        // If true, right Region derives origin region_id.
        right_derive: bool,
        callback: Callback,
    },
    Heartbeat {
        region: metapb::Region,
        peer: metapb::Peer,
        down_peers: Vec<pdpb::PeerStats>,
        pending_peers: Vec<metapb::Peer>,
        written_bytes: u64,
        written_keys: u64,
        approximate_size: Option<u64>,
        approximate_keys: Option<u64>,
    },
    StoreHeartbeat {
        stats: pdpb::StoreStats,
        store_info: StoreInfo,
    },
    ReportBatchSplit {
        regions: Vec<metapb::Region>,
    },
    ValidatePeer {
        region: metapb::Region,
        peer: metapb::Peer,
        merge_source: Option<u64>,
    },
    ReadStats {
        read_stats: HashMap<u64, FlowStatistics>,
    },
    DestroyPeer {
        region_id: u64,
    },
}

pub struct StoreStat {
    pub engine_total_bytes_read: u64,
    pub engine_total_keys_read: u64,
    pub engine_last_total_bytes_read: u64,
    pub engine_last_total_keys_read: u64,
    pub last_report_ts: u64,

    pub region_bytes_read: LocalHistogram,
    pub region_keys_read: LocalHistogram,
    pub region_bytes_written: LocalHistogram,
    pub region_keys_written: LocalHistogram,
}

impl Default for StoreStat {
    fn default() -> StoreStat {
        StoreStat {
            region_bytes_read: REGION_READ_BYTES_HISTOGRAM.local(),
            region_keys_read: REGION_READ_KEYS_HISTOGRAM.local(),
            region_bytes_written: REGION_WRITTEN_BYTES_HISTOGRAM.local(),
            region_keys_written: REGION_WRITTEN_KEYS_HISTOGRAM.local(),

            last_report_ts: 0,
            engine_total_bytes_read: 0,
            engine_total_keys_read: 0,
            engine_last_total_bytes_read: 0,
            engine_last_total_keys_read: 0,
        }
    }
}

#[derive(Default)]
pub struct PeerStat {
    pub read_bytes: u64,
    pub read_keys: u64,
    pub last_read_bytes: u64,
    pub last_read_keys: u64,
    pub last_written_bytes: u64,
    pub last_written_keys: u64,
    pub last_report_ts: u64,
}

impl Display for Task {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match *self {
            Task::AskSplit {
                ref region,
                ref split_key,
                ..
            } => write!(
                f,
                "ask split region {} with key {}",
                region.get_id(),
                escape(split_key)
            ),
            Task::AskBatchSplit {
                ref region,
                ref split_keys,
                ..
            } => write!(
                f,
                "ask split region {} with {}",
                region.get_id(),
                KeysInfoFormatter(&split_keys)
            ),
            Task::Heartbeat {
                ref region,
                ref peer,
                ..
            } => write!(
                f,
                "heartbeat for region {:?}, leader {}",
                region,
                peer.get_id()
            ),
            Task::StoreHeartbeat { ref stats, .. } => {
                write!(f, "store heartbeat stats: {:?}", stats)
            }
            Task::ReportBatchSplit { ref regions } => write!(f, "report split {:?}", regions),
            Task::ValidatePeer {
                ref region,
                ref peer,
                ref merge_source,
            } => write!(
                f,
                "validate peer {:?} with region {:?}, merge_source {:?}",
                peer, region, merge_source
            ),
            Task::ReadStats { ref read_stats } => {
                write!(f, "get the read statistics {:?}", read_stats)
            }
            Task::DestroyPeer { ref region_id } => {
                write!(f, "destroy peer of region {}", region_id)
            }
        }
    }
}

pub struct Runner<T: PdClient> {
    store_id: u64,
    pd_client: Arc<T>,
    router: RaftRouter,
    db: Arc<DB>,
    region_peers: HashMap<u64, PeerStat>,
    store_stat: StoreStat,
    is_hb_receiver_scheduled: bool,

    // use for Runner inner handle function to send Task to itself
    // actually it is the sender connected to Runner's Worker which
    // calls Runner's run() on Task received.
    scheduler: Scheduler<Task>,
}

impl<T: PdClient> Runner<T> {
    pub fn new(
        store_id: u64,
        pd_client: Arc<T>,
        router: RaftRouter,
        db: Arc<DB>,
        scheduler: Scheduler<Task>,
    ) -> Runner<T> {
        Runner {
            store_id,
            pd_client,
            router,
            db,
            is_hb_receiver_scheduled: false,
            region_peers: HashMap::default(),
            store_stat: StoreStat::default(),
            scheduler,
        }
    }

    fn handle_ask_split(
        &self,
        handle: &Handle,
        mut region: metapb::Region,
        split_key: Vec<u8>,
        peer: metapb::Peer,
        right_derive: bool,
        callback: Callback,
    ) {
        let router = self.router.clone();
        let f = self.pd_client.ask_split(region.clone()).then(move |resp| {
            match resp {
                Ok(mut resp) => {
                    info!(
                        "try to split region";
                        "region_id" => region.get_id(),
                        "new_region_id" => resp.get_new_region_id(),
                        "region" => ?region
                    );

                    let req = new_split_region_request(
                        split_key,
                        resp.get_new_region_id(),
                        resp.take_new_peer_ids(),
                        right_derive,
                    );
                    let region_id = region.get_id();
                    let epoch = region.take_region_epoch();
                    send_admin_request(&router, region_id, epoch, peer, req, callback)
                }
                Err(e) => {
                    debug!("failed to ask split";
                    "region_id" => region.get_id(),
                    "err" => ?e);
                }
            }
            Ok(())
        });
        handle.spawn(f)
    }

    fn handle_ask_batch_split(
        &self,
        handle: &Handle,
        mut region: metapb::Region,
        mut split_keys: Vec<Vec<u8>>,
        peer: metapb::Peer,
        right_derive: bool,
        callback: Callback,
    ) {
        let router = self.router.clone();
        let scheduler = self.scheduler.clone();
        let f = self
            .pd_client
            .ask_batch_split(region.clone(), split_keys.len())
            .then(move |resp| {
                match resp {
                    Ok(mut resp) => {
                        info!(
                            "try to batch split region";
                            "region_id" => region.get_id(),
                            "new_region_ids" => ?resp.get_ids(),
                            "region" => ?region,
                        );

                        let req = new_batch_split_region_request(
                            split_keys,
                            resp.take_ids().into_vec(),
                            right_derive,
                        );
                        let region_id = region.get_id();
                        let epoch = region.take_region_epoch();
                        send_admin_request(&router, region_id, epoch, peer, req, callback)
                    }
                    // When rolling update, there might be some old version tikvs that don't support batch split in cluster.
                    // In this situation, PD version check would refuse `ask_batch_split`.
                    // But if update time is long, it may cause large Regions, so call `ask_split` instead.
                    Err(Error::Incompatible) => {
                        let (region_id, peer_id) = (region.id, peer.id);
                        info!(
                            "ask_batch_split is incompatible, use ask_split instead";
                            "region_id" => region_id
                        );
                        let task = Task::AskSplit {
                            region,
                            split_key: split_keys.pop().unwrap(),
                            peer,
                            right_derive,
                            callback,
                        };
                        if let Err(Stopped(t)) = scheduler.schedule(task) {
                            error!(
                                "failed to notify pd to split: Stopped";
                                "region_id" => region_id,
                                "peer_id" =>  peer_id
                            );
                            match t {
                                Task::AskSplit { callback, .. } => {
                                    callback.invoke_with_response(new_error(box_err!(
                                        "failed to split: Stopped"
                                    )));
                                }
                                _ => unreachable!(),
                            }
                        }
                    }
                    Err(e) => {
                        debug!(
                            "ask batch split failed";
                            "region_id" => region.get_id(),
                            "err" => ?e,
                        );
                    }
                }
                Ok(())
            });
        handle.spawn(f)
    }

    fn handle_heartbeat(
        &self,
        handle: &Handle,
        region: metapb::Region,
        peer: metapb::Peer,
        region_stat: RegionStat,
    ) {
        self.store_stat
            .region_bytes_written
            .observe(region_stat.written_bytes as f64);
        self.store_stat
            .region_keys_written
            .observe(region_stat.written_keys as f64);
        self.store_stat
            .region_bytes_read
            .observe(region_stat.read_bytes as f64);
        self.store_stat
            .region_keys_read
            .observe(region_stat.read_keys as f64);

        let f = self
            .pd_client
            .region_heartbeat(region.clone(), peer.clone(), region_stat)
            .map_err(move |e| {
                debug!(
                    "failed to send heartbeat";
                    "region_id" => region.get_id(),
                    "err" => ?e
                );
            });
        handle.spawn(f);
    }

    fn handle_store_heartbeat(
        &mut self,
        handle: &Handle,
        mut stats: pdpb::StoreStats,
        store_info: StoreInfo,
    ) {
        let disk_stats = match fs2::statvfs(store_info.engine.path()) {
            Err(e) => {
                error!(
                    "get disk stat for rocksdb failed";
                    "engine_path" => store_info.engine.path(),
                    "err" => ?e
                );
                return;
            }
            Ok(stats) => stats,
        };

        let disk_cap = disk_stats.total_space();
        let capacity = if store_info.capacity == 0 || disk_cap < store_info.capacity {
            disk_cap
        } else {
            store_info.capacity
        };
        stats.set_capacity(capacity);

        let used_size =
            stats.get_used_size() + get_engine_used_size(Arc::clone(&store_info.engine));
        stats.set_used_size(used_size);

        let mut available = if capacity > used_size {
            capacity - used_size
        } else {
            warn!("no available space");
            0
        };

        // We only care about rocksdb SST file size, so we should check disk available here.
        if available > disk_stats.free_space() {
            available = disk_stats.free_space();
        }

        stats.set_available(available);
        stats.set_bytes_read(
            self.store_stat.engine_total_bytes_read - self.store_stat.engine_last_total_bytes_read,
        );
        stats.set_keys_read(
            self.store_stat.engine_total_keys_read - self.store_stat.engine_last_total_keys_read,
        );
        let mut interval = pdpb::TimeInterval::new();
        interval.set_start_timestamp(self.store_stat.last_report_ts);
        stats.set_interval(interval);
        self.store_stat.engine_last_total_bytes_read = self.store_stat.engine_total_bytes_read;
        self.store_stat.engine_last_total_keys_read = self.store_stat.engine_total_keys_read;
        self.store_stat.last_report_ts = time_now_sec();
        self.store_stat.region_bytes_written.flush();
        self.store_stat.region_keys_written.flush();
        self.store_stat.region_bytes_read.flush();
        self.store_stat.region_keys_read.flush();

        STORE_SIZE_GAUGE_VEC
            .with_label_values(&["capacity"])
            .set(capacity as i64);
        STORE_SIZE_GAUGE_VEC
            .with_label_values(&["available"])
            .set(available as i64);

        let f = self.pd_client.store_heartbeat(stats).map_err(|e| {
            error!("store heartbeat failed"; "err" => ?e);
        });
        handle.spawn(f);
    }

    fn handle_report_batch_split(&self, handle: &Handle, regions: Vec<metapb::Region>) {
        let f = self.pd_client.report_batch_split(regions).map_err(|e| {
            debug!("report split failed"; "err" => ?e);
        });
        handle.spawn(f);
    }

    fn handle_validate_peer(
        &self,
        handle: &Handle,
        local_region: metapb::Region,
        peer: metapb::Peer,
        merge_source: Option<u64>,
    ) {
        let router = self.router.clone();
        let f = self
            .pd_client
            .get_region_by_id(local_region.get_id())
            .then(move |resp| {
                match resp {
                    Ok(Some(pd_region)) => {
                        if is_epoch_stale(
                            pd_region.get_region_epoch(),
                            local_region.get_region_epoch(),
                        ) {
                            // The local Region epoch is fresher than Region epoch in PD
                            // This means the Region info in PD is not updated to the latest even
                            // after `max_leader_missing_duration`. Something is wrong in the system.
                            // Just add a log here for this situation.
                            info!(
                                "local region epoch is greater the \
                                 region epoch in PD ignore validate peer";
                                "region_id" => local_region.get_id(),
                                "peer_id" => peer.get_id(),
                                "local_region_epoch" => ?local_region.get_region_epoch(),
                                "pd_region_epoch" => ?pd_region.get_region_epoch()
                            );
                            PD_VALIDATE_PEER_COUNTER_VEC
                                .with_label_values(&["region epoch error"])
                                .inc();
                            return Ok(());
                        }

                        if pd_region
                            .get_peers()
                            .iter()
                            .all(|p| p.get_id() != peer.get_id())
                        {
                            // Peer is not a member of this Region anymore. Probably it's removed out.
                            // Send it a raft massage to destroy it since it's obsolete.
                            info!(
                                "peer is not a valid member of region, to be \
                                 destroyed soon";
                                "region_id" => local_region.get_id(),
                                "peer_id" => peer.get_id(),
                                "pd_region" => ?pd_region
                            );
                            PD_VALIDATE_PEER_COUNTER_VEC
                                .with_label_values(&["peer stale"])
                                .inc();
                            if let Some(source) = merge_source {
                                send_merge_fail(&router, source, peer);
                            } else {
                                send_destroy_peer_message(&router, local_region, peer, pd_region);
                            }
                            return Ok(());
                        }
                        info!(
                            "peer is still valid a memeber of region";
                            "region_id" => local_region.get_id(),
                            "peer_id" => peer.get_id(),
                            "pd_region" => ?pd_region
                        );
                        PD_VALIDATE_PEER_COUNTER_VEC
                            .with_label_values(&["peer valid"])
                            .inc();
                    }
                    Ok(None) => {
                        // splitted Region has not yet reported to PD.
                        // TODO: handle merge
                    }
                    Err(e) => {
                        error!("get region failed"; "err" => ?e);
                    }
                }
                Ok(())
            });
        handle.spawn(f);
    }

    fn schedule_heartbeat_receiver(&mut self, handle: &Handle) {
        let router = self.router.clone();
        let store_id = self.store_id;
        let f = self
            .pd_client
            .handle_region_heartbeat_response(self.store_id, move |mut resp| {
                let region_id = resp.get_region_id();
                let epoch = resp.take_region_epoch();
                let peer = resp.take_target_peer();

                if resp.has_change_peer() {
                    PD_HEARTBEAT_COUNTER_VEC
                        .with_label_values(&["change peer"])
                        .inc();

                    let mut change_peer = resp.take_change_peer();
                    info!(
                        "try to change peer";
                        "region_id" => region_id,
                        "change_type" => ?change_peer.get_change_type(),
                        "peer" => ?change_peer.get_peer()
                    );
                    let req = new_change_peer_request(
                        change_peer.get_change_type(),
                        change_peer.take_peer(),
                    );
                    send_admin_request(&router, region_id, epoch, peer, req, Callback::None);
                } else if resp.has_transfer_leader() {
                    PD_HEARTBEAT_COUNTER_VEC
                        .with_label_values(&["transfer leader"])
                        .inc();

                    let mut transfer_leader = resp.take_transfer_leader();
                    info!(
                        "try to transfer leader";
                        "region_id" => region_id,
                        "from_peer" => ?peer,
                        "to_peer" => ?transfer_leader.get_peer()
                    );
                    let req = new_transfer_leader_request(transfer_leader.take_peer());
                    send_admin_request(&router, region_id, epoch, peer, req, Callback::None);
                } else if resp.has_split_region() {
                    PD_HEARTBEAT_COUNTER_VEC
                        .with_label_values(&["split region"])
                        .inc();

                    let split_region = resp.take_split_region();
                    info!("try to split"; "region_id" => region_id, "region_epoch" => ?epoch);
                    let msg = CasualMessage::HalfSplitRegion {
                        region_epoch: epoch,
                        policy: split_region.get_policy(),
                    };
                    if let Err(e) = router.send(region_id, PeerMsg::CasualMessage(msg)) {
                        error!("send halfsplit request failed"; "region_id" => region_id, "err" => ?e);
                    }
                } else if resp.has_merge() {
                    PD_HEARTBEAT_COUNTER_VEC.with_label_values(&["merge"]).inc();

                    let merge = resp.take_merge();
                    info!("try to merge"; "region_id" => region_id, "merge" => ?merge);
                    let req = new_merge_request(merge);
                    send_admin_request(&router, region_id, epoch, peer, req, Callback::None)
                } else {
                    PD_HEARTBEAT_COUNTER_VEC.with_label_values(&["noop"]).inc();
                }
            })
            .map_err(|e| panic!("unexpected error: {:?}", e))
            .map(move |_| {
                info!(
                    "region heartbeat response handler exit";
                    "store_id" => store_id,
                )
            });
        handle.spawn(f);
        self.is_hb_receiver_scheduled = true;
    }

    fn handle_read_stats(&mut self, read_stats: HashMap<u64, FlowStatistics>) {
        for (region_id, stats) in read_stats {
            let peer_stat = self
                .region_peers
                .entry(region_id)
                .or_insert_with(PeerStat::default);
            peer_stat.read_bytes += stats.read_bytes as u64;
            peer_stat.read_keys += stats.read_keys as u64;
            self.store_stat.engine_total_bytes_read += stats.read_bytes as u64;
            self.store_stat.engine_total_keys_read += stats.read_keys as u64;
        }
    }

    fn handle_destroy_peer(&mut self, region_id: u64) {
        match self.region_peers.remove(&region_id) {
            None => return,
            Some(_) => info!("remove peer statistic record in pd"; "region_id" => region_id),
        }
    }
}

impl<T: PdClient> Runnable<Task> for Runner<T> {
    fn run(&mut self, task: Task, handle: &Handle) {
        debug!("executing task"; "task" => %task);

        if !self.is_hb_receiver_scheduled {
            self.schedule_heartbeat_receiver(handle);
        }

        match task {
            Task::AskSplit {
                region,
                split_key,
                peer,
                right_derive,
                callback,
            } => self.handle_ask_split(handle, region, split_key, peer, right_derive, callback),
            Task::AskBatchSplit {
                region,
                split_keys,
                peer,
                right_derive,
                callback,
            } => self.handle_ask_batch_split(
                handle,
                region,
                split_keys,
                peer,
                right_derive,
                callback,
            ),
            Task::Heartbeat {
                region,
                peer,
                down_peers,
                pending_peers,
                written_bytes,
                written_keys,
                approximate_size,
                approximate_keys,
            } => {
                let approximate_size = approximate_size.unwrap_or_else(|| {
                    get_region_approximate_size(&self.db, &region).unwrap_or_default()
                });
                let approximate_keys = approximate_keys.unwrap_or_else(|| {
                    get_region_approximate_keys(&self.db, &region).unwrap_or_default()
                });
                let (
                    read_bytes_delta,
                    read_keys_delta,
                    written_bytes_delta,
                    written_keys_delta,
                    last_report_ts,
                ) = {
                    let peer_stat = self
                        .region_peers
                        .entry(region.get_id())
                        .or_insert_with(PeerStat::default);
                    let read_bytes_delta = peer_stat.read_bytes - peer_stat.last_read_bytes;
                    let read_keys_delta = peer_stat.read_keys - peer_stat.last_read_keys;
                    let written_bytes_delta = written_bytes - peer_stat.last_written_bytes;
                    let written_keys_delta = written_keys - peer_stat.last_written_keys;
                    let last_report_ts = peer_stat.last_report_ts;
                    peer_stat.last_written_bytes = written_bytes;
                    peer_stat.last_written_keys = written_keys;
                    peer_stat.last_read_bytes = peer_stat.read_bytes;
                    peer_stat.last_read_keys = peer_stat.read_keys;
                    peer_stat.last_report_ts = time_now_sec();
                    (
                        read_bytes_delta,
                        read_keys_delta,
                        written_bytes_delta,
                        written_keys_delta,
                        last_report_ts,
                    )
                };
                self.handle_heartbeat(
                    handle,
                    region,
                    peer,
                    RegionStat {
                        down_peers,
                        pending_peers,
                        written_bytes: written_bytes_delta,
                        written_keys: written_keys_delta,
                        read_bytes: read_bytes_delta,
                        read_keys: read_keys_delta,
                        approximate_size,
                        approximate_keys,
                        last_report_ts,
                    },
                )
            }
            Task::StoreHeartbeat { stats, store_info } => {
                self.handle_store_heartbeat(handle, stats, store_info)
            }
            Task::ReportBatchSplit { regions } => self.handle_report_batch_split(handle, regions),
            Task::ValidatePeer {
                region,
                peer,
                merge_source,
            } => self.handle_validate_peer(handle, region, peer, merge_source),
            Task::ReadStats { read_stats } => self.handle_read_stats(read_stats),
            Task::DestroyPeer { region_id } => self.handle_destroy_peer(region_id),
        };
    }
}

fn new_change_peer_request(change_type: ConfChangeType, peer: metapb::Peer) -> AdminRequest {
    let mut req = AdminRequest::new();
    req.set_cmd_type(AdminCmdType::ChangePeer);
    req.mut_change_peer().set_change_type(change_type);
    req.mut_change_peer().set_peer(peer);
    req
}

fn new_split_region_request(
    split_key: Vec<u8>,
    new_region_id: u64,
    peer_ids: Vec<u64>,
    right_derive: bool,
) -> AdminRequest {
    let mut req = AdminRequest::new();
    req.set_cmd_type(AdminCmdType::Split);
    req.mut_split().set_split_key(split_key);
    req.mut_split().set_new_region_id(new_region_id);
    req.mut_split().set_new_peer_ids(peer_ids);
    req.mut_split().set_right_derive(right_derive);
    req
}

fn new_batch_split_region_request(
    split_keys: Vec<Vec<u8>>,
    ids: Vec<pdpb::SplitID>,
    right_derive: bool,
) -> AdminRequest {
    let mut req = AdminRequest::new();
    req.set_cmd_type(AdminCmdType::BatchSplit);
    req.mut_splits().set_right_derive(right_derive);
    let mut requests = Vec::with_capacity(ids.len());
    for (mut id, key) in ids.into_iter().zip(split_keys) {
        let mut split = SplitRequest::new();
        split.set_split_key(key);
        split.set_new_region_id(id.get_new_region_id());
        split.set_new_peer_ids(id.take_new_peer_ids());
        requests.push(split);
    }
    req.mut_splits()
        .set_requests(RepeatedField::from_vec(requests));
    req
}

fn new_transfer_leader_request(peer: metapb::Peer) -> AdminRequest {
    let mut req = AdminRequest::new();
    req.set_cmd_type(AdminCmdType::TransferLeader);
    req.mut_transfer_leader().set_peer(peer);
    req
}

fn new_merge_request(merge: pdpb::Merge) -> AdminRequest {
    let mut req = AdminRequest::new();
    req.set_cmd_type(AdminCmdType::PrepareMerge);
    req.mut_prepare_merge()
        .set_target(merge.get_target().to_owned());
    req
}

fn send_admin_request(
    router: &RaftRouter,
    region_id: u64,
    epoch: metapb::RegionEpoch,
    peer: metapb::Peer,
    request: AdminRequest,
    callback: Callback,
) {
    let cmd_type = request.get_cmd_type();

    let mut req = RaftCmdRequest::new();
    req.mut_header().set_region_id(region_id);
    req.mut_header().set_region_epoch(epoch);
    req.mut_header().set_peer(peer);

    req.set_admin_request(request);

    if let Err(e) = router.send_raft_command(RaftCommand::new(req, callback)) {
        error!(
            "send request failed";
            "region_id" => region_id, "cmd_type" => ?cmd_type, "err" => ?e,
        );
    }
}

/// Sends merge fail message to gc merge source.
fn send_merge_fail(router: &RaftRouter, source_region_id: u64, target: metapb::Peer) {
    let target_id = target.get_id();
    if let Err(e) = router.send(
        source_region_id,
        PeerMsg::CasualMessage(CasualMessage::MergeResult {
            target,
            stale: true,
        }),
    ) {
        error!(
            "source region report merge failed";
            "region_id" => source_region_id, "targe_region_id" => target_id, "err" => ?e,
        );
    }
}

/// Sends a raft message to destroy the specified stale Peer
fn send_destroy_peer_message(
    router: &RaftRouter,
    local_region: metapb::Region,
    peer: metapb::Peer,
    pd_region: metapb::Region,
) {
    let mut message = RaftMessage::new();
    message.set_region_id(local_region.get_id());
    message.set_from_peer(peer.clone());
    message.set_to_peer(peer.clone());
    message.set_region_epoch(pd_region.get_region_epoch().clone());
    message.set_is_tombstone(true);
    if let Err(e) = router.send_raft_message(message) {
        error!(
            "send gc peer request failed";
            "region_id" => local_region.get_id(),
            "err" => ?e
        )
    }
}
