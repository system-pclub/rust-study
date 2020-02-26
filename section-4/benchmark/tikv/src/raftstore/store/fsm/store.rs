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

use crossbeam::channel::{TryRecvError, TrySendError};
use futures::Future;
use kvproto::import_sstpb::SSTMeta;
use kvproto::metapb::{self, Region, RegionEpoch};
use kvproto::pdpb::StoreStats;
use kvproto::raft_cmdpb::{AdminCmdType, AdminRequest};
use kvproto::raft_serverpb::{PeerState, RaftMessage, RegionLocalState};
use raft::{Ready, StateRole};
use std::collections::BTreeMap;
use std::collections::Bound::{Excluded, Included, Unbounded};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{mem, thread, u64};
use time::{self, Timespec};
use tokio_threadpool::{Sender as ThreadPoolSender, ThreadPool};

use crate::pd::{PdClient, PdRunner, PdTask};
use crate::raftstore::coprocessor::split_observer::SplitObserver;
use crate::raftstore::coprocessor::{CoprocessorHost, RegionChangeEvent};
use crate::raftstore::store::util::is_initial_msg;
use crate::raftstore::Result;
use crate::storage::engine::{CompactionJobInfo, WriteBatch, WriteOptions, DB};
use crate::storage::{CF_DEFAULT, CF_LOCK, CF_RAFT, CF_WRITE};
use crate::util::collections::{HashMap, HashSet};
use crate::util::mpsc::{self, LooseBoundedSender, Receiver};
use crate::util::rocksdb_util::{self, CompactedEvent, CompactionListener};
use crate::util::time::{duration_to_sec, SlowTimer};
use crate::util::timer::SteadyTimer;
use crate::util::worker::{FutureScheduler, FutureWorker, Scheduler, Worker};
use crate::util::{is_zero_duration, sys as sys_util, Either, RingQueue};

use crate::import::SSTImporter;
use crate::raftstore::store::config::Config;
use crate::raftstore::store::engine::{Iterable, Mutable, Peekable};
use crate::raftstore::store::fsm::metrics::*;
use crate::raftstore::store::fsm::peer::{
    maybe_destroy_source, new_admin_request, PeerFsm, PeerFsmDelegate,
};
use crate::raftstore::store::fsm::{
    batch, create_apply_batch_system, ApplyBatchSystem, ApplyPollerBuilder, ApplyRouter, ApplyTask,
    BasicMailbox, BatchRouter, BatchSystem, HandlerBuilder,
};
use crate::raftstore::store::fsm::{ApplyNotifier, Fsm, PollHandler, RegionProposal};
use crate::raftstore::store::keys::{self, data_end_key, data_key, enc_end_key, enc_start_key};
use crate::raftstore::store::local_metrics::RaftMetrics;
use crate::raftstore::store::metrics::*;
use crate::raftstore::store::peer_storage::{self, HandleRaftReadyContext, InvokeContext};
use crate::raftstore::store::transport::Transport;
use crate::raftstore::store::worker::{
    CleanupSSTRunner, CleanupSSTTask, CompactRunner, CompactTask, ConsistencyCheckRunner,
    ConsistencyCheckTask, LocalReader, RaftlogGcRunner, RaftlogGcTask, ReadTask, RegionRunner,
    RegionTask, SplitCheckRunner, SplitCheckTask,
};
use crate::raftstore::store::{
    util, Callback, CasualMessage, Engines, PeerMsg, RaftCommand, SnapManager, SnapshotDeleter,
    StoreMsg, StoreTick,
};

type Key = Vec<u8>;

const PENDING_VOTES_CAP: usize = 20;

pub struct StoreInfo {
    pub engine: Arc<DB>,
    pub capacity: u64,
}

pub struct StoreMeta {
    /// region_end_key -> region_id
    pub region_ranges: BTreeMap<Vec<u8>, u64>,
    /// region_id -> region
    pub regions: HashMap<u64, Region>,
    /// `MsgRequestPreVote` or `MsgRequestVote` messages from newly split Regions shouldn't be dropped if there is no
    /// such Region in this store now. So the messages are recorded temporarily and will be handled later.
    pub pending_votes: RingQueue<RaftMessage>,
    /// The regions with pending snapshots.
    pub pending_snapshot_regions: Vec<Region>,
    /// A marker used to indicate the peer of a Region has received a merge target message and waits to be destroyed.
    /// target_region_id -> (source_region_id -> merge_target_epoch)
    pub pending_merge_targets: HashMap<u64, HashMap<u64, RegionEpoch>>,
    /// An inverse mapping of `pending_merge_targets` used to let source peer help target peer to clean up related entry.
    /// source_region_id -> target_region_id
    pub targets_map: HashMap<u64, u64>,
    /// In raftstore, the execute order of `PrepareMerge` and `CommitMerge` is not certain because of the messages
    /// belongs two regions. To make them in order, `PrepareMerge` will set this structure and `CommitMerge` will retry
    /// later if there is no related lock.
    /// source_region_id -> (version, BiLock).
    pub merge_locks: HashMap<u64, (u64, Option<Arc<AtomicBool>>)>,
}

impl StoreMeta {
    fn new(vote_capacity: usize) -> StoreMeta {
        StoreMeta {
            region_ranges: BTreeMap::default(),
            regions: HashMap::default(),
            pending_votes: RingQueue::with_capacity(vote_capacity),
            pending_snapshot_regions: Vec::default(),
            pending_merge_targets: HashMap::default(),
            targets_map: HashMap::default(),
            merge_locks: HashMap::default(),
        }
    }

    #[inline]
    pub fn set_region(
        &mut self,
        host: &CoprocessorHost,
        reader: &Scheduler<ReadTask>,
        region: Region,
        peer: &mut crate::raftstore::store::Peer,
    ) {
        let prev = self.regions.insert(region.get_id(), region.clone());
        if prev.map_or(true, |r| r.get_id() != region.get_id()) {
            // TODO: may not be a good idea to panic when holding a lock.
            panic!("{} region corrupted", peer.tag);
        }
        peer.set_region(host, reader, region);
    }
}

pub type RaftRouter = BatchRouter<PeerFsm, StoreFsm>;

impl RaftRouter {
    pub fn send_raft_message(
        &self,
        mut msg: RaftMessage,
    ) -> std::result::Result<(), TrySendError<RaftMessage>> {
        let id = msg.get_region_id();
        match self.try_send(id, PeerMsg::RaftMessage(msg)) {
            Either::Left(Ok(())) => return Ok(()),
            Either::Left(Err(TrySendError::Full(PeerMsg::RaftMessage(m)))) => {
                return Err(TrySendError::Full(m));
            }
            Either::Left(Err(TrySendError::Disconnected(PeerMsg::RaftMessage(m)))) => {
                return Err(TrySendError::Disconnected(m));
            }
            Either::Right(PeerMsg::RaftMessage(m)) => msg = m,
            _ => unreachable!(),
        }
        match self.send_control(StoreMsg::RaftMessage(msg)) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(StoreMsg::RaftMessage(m))) => Err(TrySendError::Full(m)),
            Err(TrySendError::Disconnected(StoreMsg::RaftMessage(m))) => {
                Err(TrySendError::Disconnected(m))
            }
            _ => unreachable!(),
        }
    }

    #[inline]
    pub fn send_raft_command(
        &self,
        cmd: RaftCommand,
    ) -> std::result::Result<(), TrySendError<RaftCommand>> {
        let region_id = cmd.request.get_header().get_region_id();
        match self.send(region_id, PeerMsg::RaftCommand(cmd)) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(PeerMsg::RaftCommand(cmd))) => Err(TrySendError::Full(cmd)),
            Err(TrySendError::Disconnected(PeerMsg::RaftCommand(cmd))) => {
                Err(TrySendError::Disconnected(cmd))
            }
            _ => unreachable!(),
        }
    }
}

pub struct PollContext<T, C: 'static> {
    pub cfg: Arc<Config>,
    pub store: metapb::Store,
    pub pd_scheduler: FutureScheduler<PdTask>,
    pub raftlog_gc_scheduler: Scheduler<RaftlogGcTask>,
    pub consistency_check_scheduler: Scheduler<ConsistencyCheckTask>,
    pub split_check_scheduler: Scheduler<SplitCheckTask>,
    pub cleanup_sst_scheduler: Scheduler<CleanupSSTTask>,
    pub local_reader: Scheduler<ReadTask>,
    pub region_scheduler: Scheduler<RegionTask>,
    pub apply_router: ApplyRouter,
    pub router: RaftRouter,
    pub compact_scheduler: Scheduler<CompactTask>,
    pub importer: Arc<SSTImporter>,
    pub store_meta: Arc<Mutex<StoreMeta>>,
    pub future_poller: ThreadPoolSender,
    pub raft_metrics: RaftMetrics,
    pub snap_mgr: SnapManager,
    pub applying_snap_count: Arc<AtomicUsize>,
    pub coprocessor_host: Arc<CoprocessorHost>,
    pub timer: SteadyTimer,
    pub trans: T,
    pub pd_client: Arc<C>,
    pub global_stat: GlobalStoreStat,
    pub store_stat: LocalStoreStat,
    pub engines: Engines,
    pub kv_wb: WriteBatch,
    pub raft_wb: WriteBatch,
    pub pending_count: usize,
    pub sync_log: bool,
    pub is_busy: bool,
    pub has_ready: bool,
    pub ready_res: Vec<(Ready, InvokeContext)>,
    pub need_flush_trans: bool,
    pub queued_snapshot: HashSet<u64>,
}

impl<T, C> HandleRaftReadyContext for PollContext<T, C> {
    #[inline]
    fn kv_wb(&self) -> &WriteBatch {
        &self.kv_wb
    }

    #[inline]
    fn kv_wb_mut(&mut self) -> &mut WriteBatch {
        &mut self.kv_wb
    }

    #[inline]
    fn raft_wb(&self) -> &WriteBatch {
        &self.raft_wb
    }

    #[inline]
    fn raft_wb_mut(&mut self) -> &mut WriteBatch {
        &mut self.raft_wb
    }

    #[inline]
    fn sync_log(&self) -> bool {
        self.sync_log
    }

    #[inline]
    fn set_sync_log(&mut self, sync: bool) {
        self.sync_log = sync;
    }
}

impl<T, C> PollContext<T, C> {
    #[inline]
    pub fn store_id(&self) -> u64 {
        self.store.get_id()
    }
}

impl<T: Transport, C> PollContext<T, C> {
    #[inline]
    fn schedule_store_tick(&self, tick: StoreTick, timeout: Duration) {
        if !is_zero_duration(&timeout) {
            let mb = self.router.control_mailbox();
            let f = self
                .timer
                .delay(timeout)
                .map(move |_| {
                    if let Err(e) = mb.force_send(StoreMsg::Tick(tick)) {
                        info!(
                            "failed to schedule store tick, are we shutting down?";
                            "tick" => ?tick,
                            "err" => ?e
                        );
                    }
                })
                .map_err(move |e| {
                    panic!("tick {:?} is lost due to timeout error: {:?}", tick, e);
                });
            self.future_poller.spawn(f).unwrap();
        }
    }

    pub fn handle_stale_msg(
        &mut self,
        msg: &RaftMessage,
        cur_epoch: RegionEpoch,
        need_gc: bool,
        target_region: Option<metapb::Region>,
    ) {
        let region_id = msg.get_region_id();
        let from_peer = msg.get_from_peer();
        let to_peer = msg.get_to_peer();
        let msg_type = msg.get_message().get_msg_type();

        if !need_gc {
            info!(
                "raft message is stale, ignore it";
                "region_id" => region_id,
                "current_region_epoch" => ?cur_epoch,
                "msg_type" => ?msg_type,
            );
            self.raft_metrics.message_dropped.stale_msg += 1;
            return;
        }

        info!(
            "raft message is stale, tell to gc";
            "region_id" => region_id,
            "current_region_epoch" => ?cur_epoch,
            "msg_type" => ?msg_type,
        );

        let mut gc_msg = RaftMessage::new();
        gc_msg.set_region_id(region_id);
        gc_msg.set_from_peer(to_peer.clone());
        gc_msg.set_to_peer(from_peer.clone());
        gc_msg.set_region_epoch(cur_epoch.clone());
        if let Some(r) = target_region {
            gc_msg.set_merge_target(r);
        } else {
            gc_msg.set_is_tombstone(true);
        }
        if let Err(e) = self.trans.send(gc_msg) {
            error!(
                "send gc message failed";
                "region_id" => region_id,
                "err" => ?e
            );
        }
        self.need_flush_trans = true;
    }
}

struct Store {
    // store id, before start the id is 0.
    id: u64,
    last_compact_checked_key: Key,
    stopped: bool,
    start_time: Option<Timespec>,
    consistency_check_time: HashMap<u64, Instant>,
}

pub struct StoreFsm {
    store: Store,
    receiver: Receiver<StoreMsg>,
}

impl StoreFsm {
    pub fn new(cfg: &Config) -> (LooseBoundedSender<StoreMsg>, Box<StoreFsm>) {
        let (tx, rx) = mpsc::loose_bounded(cfg.notify_capacity);
        let fsm = Box::new(StoreFsm {
            store: Store {
                id: 0,
                last_compact_checked_key: keys::DATA_MIN_KEY.to_vec(),
                stopped: false,
                start_time: None,
                consistency_check_time: HashMap::default(),
            },
            receiver: rx,
        });
        (tx, fsm)
    }
}

impl Fsm for StoreFsm {
    type Message = StoreMsg;

    #[inline]
    fn is_stopped(&self) -> bool {
        self.store.stopped
    }
}

struct StoreFsmDelegate<'a, T: 'static, C: 'static> {
    fsm: &'a mut StoreFsm,
    ctx: &'a mut PollContext<T, C>,
}

impl<'a, T: Transport, C: PdClient> StoreFsmDelegate<'a, T, C> {
    fn on_tick(&mut self, tick: StoreTick) {
        let t = SlowTimer::new();
        match tick {
            StoreTick::PdStoreHeartbeat => self.on_pd_store_heartbeat_tick(),
            StoreTick::SnapGc => self.on_snap_mgr_gc(),
            StoreTick::CompactLockCf => self.on_compact_lock_cf(),
            StoreTick::CompactCheck => self.on_compact_check_tick(),
            StoreTick::ConsistencyCheck => self.on_consistency_check_tick(),
            StoreTick::CleanupImportSST => self.on_cleanup_import_sst_tick(),
        }
        RAFT_EVENT_DURATION
            .with_label_values(&[tick.tag()])
            .observe(duration_to_sec(t.elapsed()) as f64);
        slow_log!(t, "[store {}] handle timeout {:?}", self.fsm.store.id, tick);
    }

    fn handle_msgs(&mut self, msgs: &mut Vec<StoreMsg>) {
        for m in msgs.drain(..) {
            match m {
                StoreMsg::Tick(tick) => self.on_tick(tick),
                StoreMsg::RaftMessage(msg) => {
                    if let Err(e) = self.on_raft_message(msg) {
                        error!(
                            "handle raft message failed";
                            "store_id" => self.fsm.store.id,
                            "err" => ?e
                        );
                    }
                }
                StoreMsg::CompactedEvent(event) => self.on_compaction_finished(event),
                StoreMsg::ValidateSSTResult { invalid_ssts } => {
                    self.on_validate_sst_result(invalid_ssts)
                }
                StoreMsg::ClearRegionSizeInRange { start_key, end_key } => {
                    self.clear_region_size_in_range(&start_key, &end_key)
                }
                StoreMsg::SnapshotStats => self.store_heartbeat_pd(),
                StoreMsg::Start { store } => self.start(store),
            }
        }
    }

    fn start(&mut self, store: metapb::Store) {
        if self.fsm.store.start_time.is_some() {
            panic!(
                "[store {}] unable to start again with meta {:?}",
                self.fsm.store.id, store
            );
        }
        self.fsm.store.id = store.get_id();
        self.fsm.store.start_time = Some(time::get_time());
        self.register_cleanup_import_sst_tick();
        self.register_compact_check_tick();
        self.register_pd_store_heartbeat_tick();
        self.register_compact_lock_cf_tick();
        self.register_snap_mgr_gc_tick();
        self.register_consistency_check_tick();
    }
}

pub struct RaftPoller<T: 'static, C: 'static> {
    tag: String,
    store_msg_buf: Vec<StoreMsg>,
    peer_msg_buf: Vec<PeerMsg>,
    previous_metrics: RaftMetrics,
    timer: SlowTimer,
    poll_ctx: PollContext<T, C>,
    pending_proposals: Vec<RegionProposal>,
    messages_per_tick: usize,
}

impl<T: Transport, C: PdClient> RaftPoller<T, C> {
    fn handle_raft_ready(&mut self, peers: &mut [Box<PeerFsm>]) {
        // Only enable the fail point when the store id is equal to 3, which is
        // the id of slow store in tests.
        fail_point!("on_raft_ready", self.poll_ctx.store_id() == 3, |_| {});
        if !self.pending_proposals.is_empty() {
            for prop in self.pending_proposals.drain(..) {
                self.poll_ctx
                    .apply_router
                    .schedule_task(prop.region_id, ApplyTask::Proposal(prop));
            }
        }
        if self.poll_ctx.need_flush_trans {
            self.poll_ctx.trans.flush();
            self.poll_ctx.need_flush_trans = false;
        }
        let ready_cnt = self.poll_ctx.ready_res.len();
        self.poll_ctx.raft_metrics.ready.has_ready_region += ready_cnt as u64;
        fail_point!("raft_before_save");
        if !self.poll_ctx.kv_wb.is_empty() {
            let mut write_opts = WriteOptions::new();
            write_opts.set_sync(true);
            let kv_wb = mem::replace(&mut self.poll_ctx.kv_wb, WriteBatch::new());
            self.poll_ctx
                .engines
                .kv
                .write_opt(kv_wb, &write_opts)
                .unwrap_or_else(|e| {
                    panic!("{} failed to save append state result: {:?}", self.tag, e);
                });
        }
        fail_point!("raft_between_save");
        if !self.poll_ctx.raft_wb.is_empty() {
            let mut write_opts = WriteOptions::new();
            write_opts.set_sync(self.poll_ctx.cfg.sync_log || self.poll_ctx.sync_log);
            let raft_wb = mem::replace(
                &mut self.poll_ctx.raft_wb,
                WriteBatch::with_capacity(4 * 1024),
            );
            self.poll_ctx
                .engines
                .raft
                .write_opt(raft_wb, &write_opts)
                .unwrap_or_else(|e| {
                    panic!("{} failed to save raft append result: {:?}", self.tag, e);
                });
        }
        fail_point!("raft_after_save");
        if ready_cnt != 0 {
            let mut batch_pos = 0;
            let mut ready_res = mem::replace(&mut self.poll_ctx.ready_res, Vec::default());
            for (ready, invoke_ctx) in ready_res.drain(..) {
                let region_id = invoke_ctx.region_id;
                if peers[batch_pos].region_id() == region_id {
                } else {
                    while peers[batch_pos].region_id() != region_id {
                        batch_pos += 1;
                    }
                }
                PeerFsmDelegate::new(&mut peers[batch_pos], &mut self.poll_ctx)
                    .post_raft_ready_append(ready, invoke_ctx);
            }
        }
        let dur = self.timer.elapsed();
        if !self.poll_ctx.is_busy {
            let election_timeout = Duration::from_millis(
                self.poll_ctx.cfg.raft_base_tick_interval.as_millis()
                    * self.poll_ctx.cfg.raft_election_timeout_ticks as u64,
            );
            if dur >= election_timeout {
                self.poll_ctx.is_busy = true;
            }
        }

        self.poll_ctx
            .raft_metrics
            .append_log
            .observe(duration_to_sec(dur) as f64);

        if self.poll_ctx.need_flush_trans {
            self.poll_ctx.trans.flush();
            self.poll_ctx.need_flush_trans = false;
        }

        slow_log!(
            self.timer,
            "{} handle {} pending peers include {} ready, {} entries, {} messages and {} \
             snapshots",
            self.tag,
            self.poll_ctx.pending_count,
            ready_cnt,
            self.poll_ctx.raft_metrics.ready.append - self.previous_metrics.ready.append,
            self.poll_ctx.raft_metrics.ready.message - self.previous_metrics.ready.message,
            self.poll_ctx.raft_metrics.ready.snapshot - self.previous_metrics.ready.snapshot
        );
    }
}

impl<T: Transport, C: PdClient> PollHandler<PeerFsm, StoreFsm> for RaftPoller<T, C> {
    fn begin(&mut self, batch_size: usize) {
        self.previous_metrics = self.poll_ctx.raft_metrics.clone();
        self.poll_ctx.pending_count = 0;
        self.poll_ctx.sync_log = false;
        self.poll_ctx.has_ready = false;
        self.poll_ctx.need_flush_trans = false;
        if self.pending_proposals.capacity() == 0 {
            self.pending_proposals = Vec::with_capacity(batch_size);
        }
        self.timer = SlowTimer::new();
    }

    fn handle_control(&mut self, store: &mut StoreFsm) -> Option<usize> {
        let mut expected_msg_count = None;
        while self.store_msg_buf.len() < self.messages_per_tick {
            match store.receiver.try_recv() {
                Ok(msg) => self.store_msg_buf.push(msg),
                Err(TryRecvError::Empty) => {
                    expected_msg_count = Some(0);
                    break;
                }
                Err(TryRecvError::Disconnected) => {
                    store.store.stopped = true;
                    expected_msg_count = Some(0);
                    break;
                }
            }
        }
        let mut delegate = StoreFsmDelegate {
            fsm: store,
            ctx: &mut self.poll_ctx,
        };
        delegate.handle_msgs(&mut self.store_msg_buf);
        expected_msg_count
    }

    fn handle_normal(&mut self, peer: &mut PeerFsm) -> Option<usize> {
        let mut expected_msg_count = None;
        if peer.have_pending_merge_apply_result() {
            expected_msg_count = Some(peer.receiver.len());
            let mut delegate = PeerFsmDelegate::new(peer, &mut self.poll_ctx);
            if !delegate.resume_handling_pending_apply_result() {
                return expected_msg_count;
            }
            expected_msg_count = None;
        }

        while self.peer_msg_buf.len() < self.messages_per_tick {
            match peer.receiver.try_recv() {
                // TODO: we may need a way to optimize the message copy.
                Ok(msg) => {
                    fail_point!(
                        "pause_on_apply_res_1",
                        peer.peer_id() == 1
                            && match msg {
                                PeerMsg::ApplyRes { .. } => true,
                                _ => false,
                            },
                        |_| unreachable!()
                    );
                    self.peer_msg_buf.push(msg)
                }
                Err(TryRecvError::Empty) => {
                    expected_msg_count = Some(0);
                    break;
                }
                Err(TryRecvError::Disconnected) => {
                    peer.stop();
                    expected_msg_count = Some(0);
                    break;
                }
            }
        }
        let mut delegate = PeerFsmDelegate::new(peer, &mut self.poll_ctx);
        delegate.handle_msgs(&mut self.peer_msg_buf);
        delegate.collect_ready(&mut self.pending_proposals);
        expected_msg_count
    }

    fn end(&mut self, peers: &mut [Box<PeerFsm>]) {
        if self.poll_ctx.has_ready {
            self.handle_raft_ready(peers);
        }
        if self.poll_ctx.need_flush_trans {
            self.poll_ctx.trans.flush();
            self.poll_ctx.need_flush_trans = false;
        }
        if !self.poll_ctx.queued_snapshot.is_empty() {
            let mut meta = self.poll_ctx.store_meta.lock().unwrap();
            meta.pending_snapshot_regions
                .retain(|r| !self.poll_ctx.queued_snapshot.contains(&r.get_id()));
            self.poll_ctx.queued_snapshot.clear();
        }
        self.poll_ctx
            .raft_metrics
            .process_ready
            .observe(duration_to_sec(self.timer.elapsed()) as f64);
        self.poll_ctx.raft_metrics.flush();
        self.poll_ctx.store_stat.flush();
    }
}

pub struct RaftPollerBuilder<T, C> {
    pub cfg: Arc<Config>,
    pub store: metapb::Store,
    pd_scheduler: FutureScheduler<PdTask>,
    raftlog_gc_scheduler: Scheduler<RaftlogGcTask>,
    consistency_check_scheduler: Scheduler<ConsistencyCheckTask>,
    split_check_scheduler: Scheduler<SplitCheckTask>,
    cleanup_sst_scheduler: Scheduler<CleanupSSTTask>,
    local_reader: Scheduler<ReadTask>,
    pub region_scheduler: Scheduler<RegionTask>,
    apply_router: ApplyRouter,
    pub router: RaftRouter,
    compact_scheduler: Scheduler<CompactTask>,
    pub importer: Arc<SSTImporter>,
    store_meta: Arc<Mutex<StoreMeta>>,
    future_poller: ThreadPoolSender,
    snap_mgr: SnapManager,
    pub coprocessor_host: Arc<CoprocessorHost>,
    trans: T,
    pd_client: Arc<C>,
    global_stat: GlobalStoreStat,
    pub engines: Engines,
    applying_snap_count: Arc<AtomicUsize>,
}

impl<T, C> RaftPollerBuilder<T, C> {
    /// Initialize this store. It scans the db engine, loads all regions
    /// and their peers from it, and schedules snapshot worker if necessary.
    /// WARN: This store should not be used before initialized.
    fn init(&mut self) -> Result<Vec<(LooseBoundedSender<PeerMsg>, Box<PeerFsm>)>> {
        // Scan region meta to get saved regions.
        let start_key = keys::REGION_META_MIN_KEY;
        let end_key = keys::REGION_META_MAX_KEY;
        let kv_engine = Arc::clone(&self.engines.kv);
        let store_id = self.store.get_id();
        let mut total_count = 0;
        let mut tombstone_count = 0;
        let mut applying_count = 0;
        let mut region_peers = vec![];

        let t = Instant::now();
        let mut kv_wb = WriteBatch::new();
        let mut raft_wb = WriteBatch::new();
        let mut applying_regions = vec![];
        let mut merging_count = 0;
        let mut meta = self.store_meta.lock().unwrap();
        kv_engine.scan_cf(CF_RAFT, start_key, end_key, false, |key, value| {
            let (region_id, suffix) = keys::decode_region_meta_key(key)?;
            if suffix != keys::REGION_STATE_SUFFIX {
                return Ok(true);
            }

            total_count += 1;

            let local_state = protobuf::parse_from_bytes::<RegionLocalState>(value)?;
            let region = local_state.get_region();
            if local_state.get_state() == PeerState::Tombstone {
                tombstone_count += 1;
                debug!("region is tombstone"; "region" => ?region, "store_id" => store_id);
                self.clear_stale_meta(&mut kv_wb, &mut raft_wb, &local_state);
                return Ok(true);
            }
            if local_state.get_state() == PeerState::Applying {
                // in case of restart happen when we just write region state to Applying,
                // but not write raft_local_state to raft rocksdb in time.
                peer_storage::recover_from_applying_state(&self.engines, &raft_wb, region_id)?;
                applying_count += 1;
                applying_regions.push(region.clone());
                return Ok(true);
            }

            let (tx, mut peer) = PeerFsm::create(
                store_id,
                &self.cfg,
                self.region_scheduler.clone(),
                self.engines.clone(),
                region,
            )?;
            if local_state.get_state() == PeerState::Merging {
                info!("region is merging"; "region" => ?region, "store_id" => store_id);
                merging_count += 1;
                peer.set_pending_merge_state(local_state.get_merge_state().to_owned());
            }
            meta.region_ranges.insert(enc_end_key(region), region_id);
            meta.regions.insert(region_id, region.clone());
            // No need to check duplicated here, because we use region id as the key
            // in DB.
            region_peers.push((tx, peer));
            self.coprocessor_host.on_region_changed(
                region,
                RegionChangeEvent::Create,
                StateRole::Follower,
            );
            Ok(true)
        })?;

        if !kv_wb.is_empty() {
            self.engines.kv.write(kv_wb).unwrap();
            self.engines.kv.sync_wal().unwrap();
        }
        if !raft_wb.is_empty() {
            self.engines.raft.write(raft_wb).unwrap();
            self.engines.raft.sync_wal().unwrap();
        }

        // schedule applying snapshot after raft writebatch were written.
        for region in applying_regions {
            info!("region is applying snapshot"; "region" => ?region, "store_id" => store_id);
            let (tx, mut peer) = PeerFsm::create(
                store_id,
                &self.cfg,
                self.region_scheduler.clone(),
                self.engines.clone(),
                &region,
            )?;
            peer.schedule_applying_snapshot();
            meta.region_ranges
                .insert(enc_end_key(&region), region.get_id());
            meta.regions.insert(region.get_id(), region);
            region_peers.push((tx, peer));
        }

        info!(
            "start store";
            "store_id" => store_id,
            "region_count" => total_count,
            "tombstone_count" => tombstone_count,
            "applying_count" =>  applying_count,
            "merge_count" => merging_count,
            "takes" => ?t.elapsed(),
        );

        self.clear_stale_data(&meta)?;

        Ok(region_peers)
    }

    fn clear_stale_meta(
        &self,
        kv_wb: &mut WriteBatch,
        raft_wb: &mut WriteBatch,
        origin_state: &RegionLocalState,
    ) {
        let region = origin_state.get_region();
        let raft_key = keys::raft_state_key(region.get_id());
        let raft_state = match self.engines.raft.get_msg(&raft_key).unwrap() {
            // it has been cleaned up.
            None => return,
            Some(value) => value,
        };

        peer_storage::clear_meta(&self.engines, kv_wb, raft_wb, region.get_id(), &raft_state)
            .unwrap();
        let key = keys::region_state_key(region.get_id());
        let handle = rocksdb_util::get_cf_handle(&self.engines.kv, CF_RAFT).unwrap();
        kv_wb.put_msg_cf(handle, &key, origin_state).unwrap();
    }

    /// `clear_stale_data` clean up all possible garbage data.
    fn clear_stale_data(&self, meta: &StoreMeta) -> Result<()> {
        let t = Instant::now();

        let mut ranges = Vec::new();
        let mut last_start_key = keys::data_key(b"");
        for region_id in meta.region_ranges.values() {
            let region = &meta.regions[region_id];
            let start_key = keys::enc_start_key(region);
            ranges.push((last_start_key, start_key));
            last_start_key = keys::enc_end_key(region);
        }
        ranges.push((last_start_key, keys::DATA_MAX_KEY.to_vec()));

        rocksdb_util::roughly_cleanup_ranges(&self.engines.kv, &ranges)?;

        info!(
            "cleans up garbage data";
            "store_id" => self.store.get_id(),
            "garbage_range_count" => ranges.len(),
            "takes" => ?t.elapsed()
        );

        Ok(())
    }
}

impl<T, C> HandlerBuilder<PeerFsm, StoreFsm> for RaftPollerBuilder<T, C>
where
    T: Transport + 'static,
    C: PdClient + 'static,
{
    type Handler = RaftPoller<T, C>;

    fn build(&mut self) -> RaftPoller<T, C> {
        let ctx = PollContext {
            cfg: self.cfg.clone(),
            store: self.store.clone(),
            pd_scheduler: self.pd_scheduler.clone(),
            raftlog_gc_scheduler: self.raftlog_gc_scheduler.clone(),
            consistency_check_scheduler: self.consistency_check_scheduler.clone(),
            split_check_scheduler: self.split_check_scheduler.clone(),
            cleanup_sst_scheduler: self.cleanup_sst_scheduler.clone(),
            local_reader: self.local_reader.clone(),
            region_scheduler: self.region_scheduler.clone(),
            apply_router: self.apply_router.clone(),
            router: self.router.clone(),
            compact_scheduler: self.compact_scheduler.clone(),
            importer: self.importer.clone(),
            store_meta: self.store_meta.clone(),
            future_poller: self.future_poller.clone(),
            raft_metrics: RaftMetrics::default(),
            snap_mgr: self.snap_mgr.clone(),
            applying_snap_count: self.applying_snap_count.clone(),
            coprocessor_host: self.coprocessor_host.clone(),
            timer: SteadyTimer::default(),
            trans: self.trans.clone(),
            pd_client: self.pd_client.clone(),
            global_stat: self.global_stat.clone(),
            store_stat: self.global_stat.local(),
            engines: self.engines.clone(),
            kv_wb: WriteBatch::new(),
            raft_wb: WriteBatch::with_capacity(4 * 1024),
            pending_count: 0,
            sync_log: false,
            is_busy: false,
            has_ready: false,
            ready_res: Vec::new(),
            need_flush_trans: false,
            queued_snapshot: HashSet::default(),
        };
        RaftPoller {
            tag: format!("[store {}]", ctx.store.get_id()),
            store_msg_buf: Vec::with_capacity(ctx.cfg.messages_per_tick),
            peer_msg_buf: Vec::with_capacity(ctx.cfg.messages_per_tick),
            previous_metrics: ctx.raft_metrics.clone(),
            timer: SlowTimer::new(),
            messages_per_tick: ctx.cfg.messages_per_tick,
            poll_ctx: ctx,
            pending_proposals: Vec::new(),
        }
    }
}

struct Workers {
    pd_worker: FutureWorker<PdTask>,
    raftlog_gc_worker: Worker<RaftlogGcTask>,
    consistency_check_worker: Worker<ConsistencyCheckTask>,
    split_check_worker: Worker<SplitCheckTask>,
    cleanup_sst_worker: Worker<CleanupSSTTask>,
    local_reader: Worker<ReadTask>,
    region_worker: Worker<RegionTask>,
    compact_worker: Worker<CompactTask>,
    coprocessor_host: Arc<CoprocessorHost>,
    future_poller: ThreadPool,
}

pub struct RaftBatchSystem {
    system: BatchSystem<PeerFsm, StoreFsm>,
    apply_router: ApplyRouter,
    apply_system: ApplyBatchSystem,
    router: RaftRouter,
    workers: Option<Workers>,
}

impl RaftBatchSystem {
    pub fn router(&self) -> RaftRouter {
        self.router.clone()
    }

    pub fn spawn<T: Transport + 'static, C: PdClient + 'static>(
        &mut self,
        meta: metapb::Store,
        mut cfg: Config,
        engines: Engines,
        trans: T,
        pd_client: Arc<C>,
        mgr: SnapManager,
        pd_worker: FutureWorker<PdTask>,
        local_reader: Worker<ReadTask>,
        mut coprocessor_host: CoprocessorHost,
        importer: Arc<SSTImporter>,
    ) -> Result<()> {
        assert!(self.workers.is_none());
        // TODO: we can get cluster meta regularly too later.
        cfg.validate()?;

        // TODO load coprocessors from configuration
        coprocessor_host
            .registry
            .register_admin_observer(100, Box::new(SplitObserver));

        let workers = Workers {
            split_check_worker: Worker::new("split-check"),
            region_worker: Worker::new("snapshot-worker"),
            raftlog_gc_worker: Worker::new("raft-gc-worker"),
            compact_worker: Worker::new("compact-worker"),
            pd_worker,
            local_reader,
            consistency_check_worker: Worker::new("consistency-check"),
            cleanup_sst_worker: Worker::new("cleanup-sst"),
            coprocessor_host: Arc::new(coprocessor_host),
            future_poller: tokio_threadpool::Builder::new()
                .name_prefix("future-poller")
                .pool_size(cfg.future_poll_size)
                .build(),
        };
        let mut builder = RaftPollerBuilder {
            cfg: Arc::new(cfg),
            store: meta,
            engines,
            router: self.router.clone(),
            split_check_scheduler: workers.split_check_worker.scheduler(),
            region_scheduler: workers.region_worker.scheduler(),
            raftlog_gc_scheduler: workers.raftlog_gc_worker.scheduler(),
            compact_scheduler: workers.compact_worker.scheduler(),
            pd_scheduler: workers.pd_worker.scheduler(),
            consistency_check_scheduler: workers.consistency_check_worker.scheduler(),
            cleanup_sst_scheduler: workers.cleanup_sst_worker.scheduler(),
            apply_router: self.apply_router.clone(),
            local_reader: workers.local_reader.scheduler(),
            trans,
            pd_client,
            coprocessor_host: workers.coprocessor_host.clone(),
            importer,
            snap_mgr: mgr,
            global_stat: GlobalStoreStat::default(),
            store_meta: Arc::new(Mutex::new(StoreMeta::new(PENDING_VOTES_CAP))),
            applying_snap_count: Arc::new(AtomicUsize::new(0)),
            future_poller: workers.future_poller.sender().clone(),
        };
        let region_peers = builder.init()?;
        self.start_system(workers, region_peers, builder)?;
        Ok(())
    }

    fn start_system<T: Transport + 'static, C: PdClient + 'static>(
        &mut self,
        mut workers: Workers,
        region_peers: Vec<(LooseBoundedSender<PeerMsg>, Box<PeerFsm>)>,
        builder: RaftPollerBuilder<T, C>,
    ) -> Result<()> {
        builder.snap_mgr.init()?;

        let engines = builder.engines.clone();
        let snap_mgr = builder.snap_mgr.clone();
        let cfg = builder.cfg.clone();
        let store = builder.store.clone();
        let pd_client = builder.pd_client.clone();
        let importer = builder.importer.clone();

        let apply_poller_builder = ApplyPollerBuilder::new(
            &builder,
            ApplyNotifier::Router(self.router.clone()),
            self.apply_router.clone(),
        );
        self.apply_system
            .schedule_all(region_peers.iter().map(|pair| pair.1.get_peer()));

        let reader = LocalReader::new(&builder, region_peers.iter().map(|pair| pair.1.get_peer()));

        let tag = format!("raftstore-{}", store.get_id());
        self.system.spawn(tag, builder);
        let mut mailboxes = Vec::with_capacity(region_peers.len());
        let mut address = Vec::with_capacity(region_peers.len());
        for (tx, fsm) in region_peers {
            address.push(fsm.region_id());
            mailboxes.push((fsm.region_id(), BasicMailbox::new(tx, fsm)));
        }
        self.router.register_all(mailboxes);
        // Make sure Msg::Start is the first message each FSM received.
        self.router
            .send_control(StoreMsg::Start {
                store: store.clone(),
            })
            .unwrap();
        for addr in address {
            self.router.force_send(addr, PeerMsg::Start).unwrap();
        }

        self.apply_system
            .spawn("apply".to_owned(), apply_poller_builder);
        let timer = LocalReader::new_timer();
        box_try!(workers.local_reader.start_with_timer(reader, timer));

        let split_check_runner = SplitCheckRunner::new(
            Arc::clone(&engines.kv),
            self.router.clone(),
            Arc::clone(&workers.coprocessor_host),
        );

        box_try!(workers.split_check_worker.start(split_check_runner));

        let region_runner = RegionRunner::new(
            engines.clone(),
            snap_mgr,
            cfg.snap_apply_batch_size.0 as usize,
            cfg.use_delete_range,
            cfg.clean_stale_peer_delay.0,
        );
        let timer = RegionRunner::new_timer();
        box_try!(workers.region_worker.start_with_timer(region_runner, timer));

        let raftlog_gc_runner = RaftlogGcRunner::new(None);
        box_try!(workers.raftlog_gc_worker.start(raftlog_gc_runner));

        let compact_runner = CompactRunner::new(Arc::clone(&engines.kv));
        box_try!(workers.compact_worker.start(compact_runner));

        let pd_runner = PdRunner::new(
            store.get_id(),
            Arc::clone(&pd_client),
            self.router.clone(),
            Arc::clone(&engines.kv),
            workers.pd_worker.scheduler(),
        );
        box_try!(workers.pd_worker.start(pd_runner));

        let consistency_check_runner = ConsistencyCheckRunner::new(self.router.clone());
        box_try!(workers
            .consistency_check_worker
            .start(consistency_check_runner));

        let cleanup_sst_runner = CleanupSSTRunner::new(
            store.get_id(),
            self.router.clone(),
            Arc::clone(&importer),
            Arc::clone(&pd_client),
        );
        box_try!(workers.cleanup_sst_worker.start(cleanup_sst_runner));

        if let Err(e) = sys_util::thread::set_priority(sys_util::HIGH_PRI) {
            warn!("set thread priority for raftstore failed"; "error" => ?e);
        }
        self.workers = Some(workers);
        Ok(())
    }

    pub fn shutdown(&mut self) {
        if self.workers.is_none() {
            return;
        }
        let mut workers = self.workers.take().unwrap();
        // Wait all workers finish.
        let mut handles: Vec<Option<thread::JoinHandle<()>>> = vec![];
        handles.push(workers.split_check_worker.stop());
        handles.push(workers.region_worker.stop());
        handles.push(workers.raftlog_gc_worker.stop());
        handles.push(workers.compact_worker.stop());
        handles.push(workers.pd_worker.stop());
        handles.push(workers.consistency_check_worker.stop());
        handles.push(workers.cleanup_sst_worker.stop());
        handles.push(workers.local_reader.stop());
        self.apply_system.shutdown();
        self.system.shutdown();
        for h in handles {
            if let Some(h) = h {
                h.join().unwrap();
            }
        }
        workers.coprocessor_host.shutdown();
        workers.future_poller.shutdown_now().wait().unwrap();
    }
}

pub fn create_raft_batch_system(cfg: &Config) -> (RaftRouter, RaftBatchSystem) {
    let (store_tx, store_fsm) = StoreFsm::new(cfg);
    let (apply_router, apply_system) = create_apply_batch_system(&cfg);
    let (router, system) = batch::create_system(
        cfg.store_pool_size,
        cfg.store_max_batch_size,
        store_tx,
        store_fsm,
    );
    let system = RaftBatchSystem {
        system,
        workers: None,
        apply_router,
        apply_system,
        router: router.clone(),
    };
    (router, system)
}

impl<'a, T: Transport, C: PdClient> StoreFsmDelegate<'a, T, C> {
    /// Checks if the message is targeting a stale peer.
    ///
    /// Returns true means the message can be dropped silently.
    fn check_msg(&mut self, msg: &RaftMessage) -> Result<bool> {
        let region_id = msg.get_region_id();
        let from_epoch = msg.get_region_epoch();
        let msg_type = msg.get_message().get_msg_type();
        let is_vote_msg = util::is_vote_msg(msg.get_message());
        let from_store_id = msg.get_from_peer().get_store_id();

        // Check if the target peer is tomebtone.
        let state_key = keys::region_state_key(region_id);
        let local_state: RegionLocalState =
            match self.ctx.engines.kv.get_msg_cf(CF_RAFT, &state_key)? {
                Some(state) => state,
                None => return Ok(false),
            };

        if local_state.get_state() != PeerState::Tombstone {
            // Maybe split, but not registered yet.
            self.ctx.raft_metrics.message_dropped.region_nonexistent += 1;
            if util::is_first_vote_msg(msg.get_message()) {
                let mut meta = self.ctx.store_meta.lock().unwrap();
                // Last check on whether target peer is created, otherwise, the
                // vote message will never be comsumed.
                if meta.regions.contains_key(&region_id) {
                    return Ok(false);
                }
                meta.pending_votes.push(msg.to_owned());
                info!(
                    "region doesn't exist yet, wait for it to be split";
                    "region_id" => region_id
                );
                return Ok(true);
            }
            return Err(box_err!(
                "[region {}] region not exist but not tombstone: {:?}",
                region_id,
                local_state
            ));
        }
        debug!(
            "region is in tombstone state";
            "region_id" => region_id,
            "region_local_state" => ?local_state,
        );
        let region = local_state.get_region();
        let region_epoch = region.get_region_epoch();
        if local_state.has_merge_state() {
            info!(
                "merged peer receives a stale message";
                "region_id" => region_id,
                "current_region_epoch" => ?region_epoch,
                "msg_type" => ?msg_type,
            );

            let merge_target = if let Some(peer) = util::find_peer(region, from_store_id) {
                // Maybe the target is promoted from learner to voter, but the follower
                // doesn't know it. So we only compare peer id.
                assert_eq!(peer.get_id(), msg.get_from_peer().get_id());
                // Let stale peer decides whether it should wait for merging or just remove
                // itself.
                Some(local_state.get_merge_state().get_target().to_owned())
            } else {
                // If a peer is isolated before prepare_merge and conf remove, it should just
                // remove itself.
                None
            };
            self.ctx
                .handle_stale_msg(msg, region_epoch.clone(), true, merge_target);
            return Ok(true);
        }
        // The region in this peer is already destroyed
        if util::is_epoch_stale(from_epoch, region_epoch) {
            info!(
                "tombstone peer receives a stale message";
                "region_id" => region_id,
                "from_region_epoch" => ?from_epoch,
                "current_region_epoch" => ?region_epoch,
                "msg_type" => ?msg_type,
            );

            let not_exist = util::find_peer(region, from_store_id).is_none();
            self.ctx
                .handle_stale_msg(msg, region_epoch.clone(), is_vote_msg && not_exist, None);

            return Ok(true);
        }

        if from_epoch.get_conf_ver() == region_epoch.get_conf_ver() {
            self.ctx.raft_metrics.message_dropped.region_tombstone_peer += 1;
            return Err(box_err!(
                "tombstone peer [epoch: {:?}] receive an invalid \
                 message {:?}, ignore it",
                region_epoch,
                msg_type
            ));
        }

        Ok(false)
    }

    fn on_raft_message(&mut self, mut msg: RaftMessage) -> Result<()> {
        let region_id = msg.get_region_id();
        match self.ctx.router.send(region_id, PeerMsg::RaftMessage(msg)) {
            Ok(()) | Err(TrySendError::Full(_)) => return Ok(()),
            Err(TrySendError::Disconnected(PeerMsg::RaftMessage(m))) => msg = m,
            e => panic!(
                "[store {}] [region {}] unexpected redirect error: {:?}",
                self.fsm.store.id, region_id, e
            ),
        }

        debug!(
            "handle raft message";
            "from_peer_id" => msg.get_from_peer().get_id(),
            "to_peer_id" => msg.get_to_peer().get_id(),
            "store_id" => self.fsm.store.id,
            "region_id" => region_id,
            "msg_type" => ?msg.get_message().get_msg_type(),
        );

        if msg.get_to_peer().get_store_id() != self.ctx.store_id() {
            warn!(
                "store not match, ignore it";
                "store_id" => self.ctx.store_id(),
                "to_store_id" => msg.get_to_peer().get_store_id(),
                "region_id" => region_id,
            );
            self.ctx.raft_metrics.message_dropped.mismatch_store_id += 1;
            return Ok(());
        }

        if !msg.has_region_epoch() {
            error!(
                "missing epoch in raft message, ignore it";
                "region_id" => region_id,
            );
            self.ctx.raft_metrics.message_dropped.mismatch_region_epoch += 1;
            return Ok(());
        }
        if msg.get_is_tombstone() || msg.has_merge_target() {
            // Target tombstone peer doesn't exist, so ignore it.
            return Ok(());
        }
        if self.check_msg(&msg)? {
            return Ok(());
        }
        if !self.maybe_create_peer(region_id, &msg)? {
            return Ok(());
        }
        let _ = self.ctx.router.send(region_id, PeerMsg::RaftMessage(msg));
        Ok(())
    }

    /// If target peer doesn't exist, create it.
    ///
    /// return false to indicate that target peer is in invalid state or
    /// doesn't exist and can't be created.
    fn maybe_create_peer(&mut self, region_id: u64, msg: &RaftMessage) -> Result<bool> {
        let target = msg.get_to_peer();
        // we may encounter a message with larger peer id, which means
        // current peer is stale, then we should remove current peer
        let mut guard = self.ctx.store_meta.lock().unwrap();
        let meta: &mut StoreMeta = &mut *guard;
        if meta.regions.contains_key(&region_id) {
            return Ok(true);
        }

        if !is_initial_msg(msg.get_message()) {
            let msg_type = msg.get_message().get_msg_type();
            debug!(
                "target peer doesn't exist, stale message";
                "target_peer" => ?target,
                "region_id" => region_id,
                "msg_type" => ?msg_type,
            );
            self.ctx.raft_metrics.message_dropped.stale_msg += 1;
            return Ok(false);
        }

        let mut regions_to_destroy = vec![];
        for (_, id) in meta.region_ranges.range((
            Excluded(data_key(msg.get_start_key())),
            Unbounded::<Vec<u8>>,
        )) {
            let exist_region = &meta.regions[&id];
            if enc_start_key(exist_region) >= data_end_key(msg.get_end_key()) {
                break;
            }

            debug!(
                "msg is overlapped with exist region";
                "region_id" => region_id,
                "msg" => ?msg,
                "exist_region" => ?exist_region,
            );
            if util::is_first_vote_msg(msg.get_message()) {
                meta.pending_votes.push(msg.to_owned());
            }

            // Make sure the range of region from msg is covered by existing regions.
            // If so, means that the region may be generated by some kinds of split
            // and merge by catching logs. So there is no need to accept a snapshot.
            if !is_range_covered(
                &meta.region_ranges,
                |id: u64| &meta.regions[&id],
                data_key(msg.get_start_key()),
                data_end_key(msg.get_end_key()),
            ) {
                if maybe_destroy_source(
                    meta,
                    region_id,
                    exist_region.get_id(),
                    msg.get_region_epoch().to_owned(),
                ) {
                    regions_to_destroy.push(exist_region.get_id());
                    continue;
                }
            }
            self.ctx.raft_metrics.message_dropped.region_overlap += 1;
            return Ok(false);
        }

        for id in regions_to_destroy {
            self.ctx
                .router
                .force_send(
                    id,
                    PeerMsg::CasualMessage(CasualMessage::MergeResult {
                        target: target.clone(),
                        stale: true,
                    }),
                )
                .unwrap();
        }

        // New created peers should know it's learner or not.
        let (tx, peer) = PeerFsm::replicate(
            self.ctx.store_id(),
            &self.ctx.cfg,
            self.ctx.region_scheduler.clone(),
            self.ctx.engines.clone(),
            region_id,
            target.clone(),
        )?;
        // following snapshot may overlap, should insert into region_ranges after
        // snapshot is applied.
        meta.regions
            .insert(region_id, peer.get_peer().region().to_owned());
        let mailbox = BasicMailbox::new(tx, peer);
        self.ctx.router.register(region_id, mailbox);
        self.ctx
            .router
            .force_send(region_id, PeerMsg::Start)
            .unwrap();
        Ok(true)
    }

    fn on_compaction_finished(&mut self, event: CompactedEvent) {
        // If size declining is trivial, skip.
        let total_bytes_declined = if event.total_input_bytes > event.total_output_bytes {
            event.total_input_bytes - event.total_output_bytes
        } else {
            0
        };
        if total_bytes_declined < self.ctx.cfg.region_split_check_diff.0
            || total_bytes_declined * 10 < event.total_input_bytes
        {
            return;
        }

        let output_level_str = event.output_level.to_string();
        COMPACTION_DECLINED_BYTES
            .with_label_values(&[&output_level_str])
            .observe(total_bytes_declined as f64);

        // self.cfg.region_split_check_diff.0 / 16 is an experienced value.
        let mut region_declined_bytes = {
            let meta = self.ctx.store_meta.lock().unwrap();
            calc_region_declined_bytes(
                event,
                &meta.region_ranges,
                self.ctx.cfg.region_split_check_diff.0 / 16,
            )
        };

        COMPACTION_RELATED_REGION_COUNT
            .with_label_values(&[&output_level_str])
            .observe(region_declined_bytes.len() as f64);

        for (region_id, declined_bytes) in region_declined_bytes.drain(..) {
            let _ = self.ctx.router.send(
                region_id,
                PeerMsg::CasualMessage(CasualMessage::CompactionDeclinedBytes {
                    bytes: declined_bytes,
                }),
            );
        }
    }

    fn register_compact_check_tick(&self) {
        self.ctx.schedule_store_tick(
            StoreTick::CompactCheck,
            self.ctx.cfg.region_compact_check_interval.0,
        )
    }

    fn on_compact_check_tick(&mut self) {
        self.register_compact_check_tick();
        if self.ctx.compact_scheduler.is_busy() {
            debug!(
                "compact worker is busy, check space redundancy next time";
                "store_id" => self.fsm.store.id,
            );
            return;
        }

        if rocksdb_util::auto_compactions_is_disabled(&self.ctx.engines.kv) {
            debug!(
                "skip compact check when disabled auto compactions";
                "store_id" => self.fsm.store.id,
            );
            return;
        }

        // Start from last checked key.
        let mut ranges_need_check =
            Vec::with_capacity(self.ctx.cfg.region_compact_check_step as usize + 1);
        ranges_need_check.push(self.fsm.store.last_compact_checked_key.clone());

        let largest_key = {
            let meta = self.ctx.store_meta.lock().unwrap();
            if meta.region_ranges.is_empty() {
                debug!(
                    "there is no range need to check";
                    "store_id" => self.fsm.store.id
                );
                return;
            }

            // Collect continuous ranges.
            let left_ranges = meta.region_ranges.range((
                Excluded(self.fsm.store.last_compact_checked_key.clone()),
                Unbounded::<Key>,
            ));
            ranges_need_check.extend(
                left_ranges
                    .take(self.ctx.cfg.region_compact_check_step as usize)
                    .map(|(k, _)| k.to_owned()),
            );

            // Update last_compact_checked_key.
            meta.region_ranges.keys().last().unwrap().to_vec()
        };

        let last_key = ranges_need_check.last().unwrap().clone();
        if last_key == largest_key {
            // Range [largest key, DATA_MAX_KEY) also need to check.
            if last_key != keys::DATA_MAX_KEY.to_vec() {
                ranges_need_check.push(keys::DATA_MAX_KEY.to_vec());
            }
            // Next task will start from the very beginning.
            self.fsm.store.last_compact_checked_key = keys::DATA_MIN_KEY.to_vec();
        } else {
            self.fsm.store.last_compact_checked_key = last_key;
        }

        // Schedule the task.
        let cf_names = vec![CF_DEFAULT.to_owned(), CF_WRITE.to_owned()];
        if let Err(e) = self
            .ctx
            .compact_scheduler
            .schedule(CompactTask::CheckAndCompact {
                cf_names,
                ranges: ranges_need_check,
                tombstones_num_threshold: self.ctx.cfg.region_compact_min_tombstones,
                tombstones_percent_threshold: self.ctx.cfg.region_compact_tombstones_percent,
            })
        {
            error!(
                "schedule space check task failed";
                "store_id" => self.fsm.store.id,
                "err" => ?e,
            );
        }
    }

    fn store_heartbeat_pd(&mut self) {
        let mut stats = StoreStats::new();

        let used_size = self.ctx.snap_mgr.get_total_snap_size();
        stats.set_used_size(used_size);
        stats.set_store_id(self.ctx.store_id());
        {
            let meta = self.ctx.store_meta.lock().unwrap();
            stats.set_region_count(meta.regions.len() as u32);
        }

        let snap_stats = self.ctx.snap_mgr.stats();
        stats.set_sending_snap_count(snap_stats.sending_count as u32);
        stats.set_receiving_snap_count(snap_stats.receiving_count as u32);
        STORE_SNAPSHOT_TRAFFIC_GAUGE_VEC
            .with_label_values(&["sending"])
            .set(snap_stats.sending_count as i64);
        STORE_SNAPSHOT_TRAFFIC_GAUGE_VEC
            .with_label_values(&["receiving"])
            .set(snap_stats.receiving_count as i64);

        let apply_snapshot_count = self.ctx.applying_snap_count.load(Ordering::SeqCst);
        stats.set_applying_snap_count(apply_snapshot_count as u32);
        STORE_SNAPSHOT_TRAFFIC_GAUGE_VEC
            .with_label_values(&["applying"])
            .set(apply_snapshot_count as i64);

        stats.set_start_time(self.fsm.store.start_time.unwrap().sec as u32);

        // report store write flow to pd
        stats.set_bytes_written(
            self.ctx
                .global_stat
                .stat
                .engine_total_bytes_written
                .swap(0, Ordering::SeqCst),
        );
        stats.set_keys_written(
            self.ctx
                .global_stat
                .stat
                .engine_total_keys_written
                .swap(0, Ordering::SeqCst),
        );

        stats.set_is_busy(
            self.ctx
                .global_stat
                .stat
                .is_busy
                .swap(false, Ordering::SeqCst),
        );

        let store_info = StoreInfo {
            engine: Arc::clone(&self.ctx.engines.kv),
            capacity: self.ctx.cfg.capacity.0,
        };

        let task = PdTask::StoreHeartbeat { stats, store_info };
        if let Err(e) = self.ctx.pd_scheduler.schedule(task) {
            error!("notify pd failed";
                "store_id" => self.fsm.store.id,
                "err" => ?e
            );
        }
    }

    fn on_pd_store_heartbeat_tick(&mut self) {
        self.store_heartbeat_pd();
        self.register_pd_store_heartbeat_tick();
    }

    fn handle_snap_mgr_gc(&mut self) -> Result<()> {
        let snap_keys = self.ctx.snap_mgr.list_idle_snap()?;
        if snap_keys.is_empty() {
            return Ok(());
        }
        let (mut last_region_id, mut keys) = (0, vec![]);
        let schedule_gc_snap = |region_id: u64, snaps| -> Result<()> {
            debug!(
                "schedule snap gc";
                "store_id" => self.fsm.store.id,
                "region_id" => region_id,
            );
            let gc_snap = PeerMsg::CasualMessage(CasualMessage::GcSnap { snaps });
            match self.ctx.router.send(region_id, gc_snap) {
                Ok(()) => Ok(()),
                Err(TrySendError::Disconnected(PeerMsg::CasualMessage(
                    CasualMessage::GcSnap { snaps },
                ))) => {
                    // The snapshot exists because MsgAppend has been rejected. So the
                    // peer must have been exist. But now it's disconnected, so the peer
                    // has to be destroyed instead of being created.
                    info!(
                        "region is disconnected, remove snaps";
                        "region_id" => region_id,
                        "snaps" => ?snaps,
                    );
                    for (key, is_sending) in snaps {
                        let snap = if is_sending {
                            self.ctx.snap_mgr.get_snapshot_for_sending(&key)?
                        } else {
                            self.ctx.snap_mgr.get_snapshot_for_applying(&key)?
                        };
                        self.ctx
                            .snap_mgr
                            .delete_snapshot(&key, snap.as_ref(), false);
                    }
                    Ok(())
                }
                Err(TrySendError::Full(_)) => Ok(()),
                Err(TrySendError::Disconnected(_)) => unreachable!(),
            }
        };
        for (key, is_sending) in snap_keys {
            if last_region_id == key.region_id {
                keys.push((key, is_sending));
                continue;
            }

            if !keys.is_empty() {
                schedule_gc_snap(last_region_id, keys)?;
                keys = vec![];
            }

            last_region_id = key.region_id;
            keys.push((key, is_sending));
        }
        if !keys.is_empty() {
            schedule_gc_snap(last_region_id, keys)?;
        }
        Ok(())
    }

    fn on_snap_mgr_gc(&mut self) {
        if let Err(e) = self.handle_snap_mgr_gc() {
            error!(
                "handle gc snap failed";
                "store_id" => self.fsm.store.id,
                "err" => ?e
            );
        }
        self.register_snap_mgr_gc_tick();
    }

    fn on_compact_lock_cf(&mut self) {
        // Create a compact lock cf task(compact whole range) and schedule directly.
        let lock_cf_bytes_written = self
            .ctx
            .global_stat
            .stat
            .lock_cf_bytes_written
            .load(Ordering::SeqCst);
        if lock_cf_bytes_written > self.ctx.cfg.lock_cf_compact_bytes_threshold.0 {
            self.ctx
                .global_stat
                .stat
                .lock_cf_bytes_written
                .fetch_sub(lock_cf_bytes_written, Ordering::SeqCst);

            let task = CompactTask::Compact {
                cf_name: String::from(CF_LOCK),
                start_key: None,
                end_key: None,
            };
            if let Err(e) = self.ctx.compact_scheduler.schedule(task) {
                error!(
                    "schedule compact lock cf task failed";
                    "store_id" => self.fsm.store.id,
                    "err" => ?e,
                );
            }
        }

        self.register_compact_lock_cf_tick();
    }

    fn register_pd_store_heartbeat_tick(&self) {
        self.ctx.schedule_store_tick(
            StoreTick::PdStoreHeartbeat,
            self.ctx.cfg.pd_store_heartbeat_tick_interval.0,
        );
    }

    fn register_snap_mgr_gc_tick(&self) {
        self.ctx
            .schedule_store_tick(StoreTick::SnapGc, self.ctx.cfg.snap_mgr_gc_tick_interval.0)
    }

    fn register_compact_lock_cf_tick(&self) {
        self.ctx.schedule_store_tick(
            StoreTick::CompactLockCf,
            self.ctx.cfg.lock_cf_compact_interval.0,
        )
    }
}

impl<'a, T: Transport, C: PdClient> StoreFsmDelegate<'a, T, C> {
    fn on_validate_sst_result(&mut self, ssts: Vec<SSTMeta>) {
        if ssts.is_empty() {
            return;
        }
        // A stale peer can still ingest a stale SST before it is
        // destroyed. We need to make sure that no stale peer exists.
        let mut delete_ssts = Vec::new();
        {
            let meta = self.ctx.store_meta.lock().unwrap();
            for sst in ssts {
                if !meta.regions.contains_key(&sst.get_region_id()) {
                    delete_ssts.push(sst);
                }
            }
        }
        if delete_ssts.is_empty() {
            return;
        }

        let task = CleanupSSTTask::DeleteSST { ssts: delete_ssts };
        if let Err(e) = self.ctx.cleanup_sst_scheduler.schedule(task) {
            error!(
                "schedule to delete ssts failed";
                "store_id" => self.fsm.store.id,
                "err" => ?e,
            );
        }
    }

    fn on_cleanup_import_sst(&mut self) -> Result<()> {
        let mut delete_ssts = Vec::new();
        let mut validate_ssts = Vec::new();

        let ssts = box_try!(self.ctx.importer.list_ssts());
        if ssts.is_empty() {
            return Ok(());
        }
        {
            let meta = self.ctx.store_meta.lock().unwrap();
            for sst in ssts {
                if let Some(r) = meta.regions.get(&sst.get_region_id()) {
                    let region_epoch = r.get_region_epoch();
                    if util::is_epoch_stale(sst.get_region_epoch(), region_epoch) {
                        // If the SST epoch is stale, it will not be ingested anymore.
                        delete_ssts.push(sst);
                    }
                } else {
                    // If the peer doesn't exist, we need to validate the SST through PD.
                    validate_ssts.push(sst);
                }
            }
        }

        if !delete_ssts.is_empty() {
            let task = CleanupSSTTask::DeleteSST { ssts: delete_ssts };
            if let Err(e) = self.ctx.cleanup_sst_scheduler.schedule(task) {
                error!(
                    "schedule to delete ssts failed";
                    "store_id" => self.fsm.store.id,
                    "err" => ?e
                );
            }
        }

        if !validate_ssts.is_empty() {
            let task = CleanupSSTTask::ValidateSST {
                ssts: validate_ssts,
            };
            if let Err(e) = self.ctx.cleanup_sst_scheduler.schedule(task) {
                error!(
                   "schedule to validate ssts failed";
                   "store_id" => self.fsm.store.id,
                   "err" => ?e,
                );
            }
        }

        Ok(())
    }

    fn register_consistency_check_tick(&mut self) {
        self.ctx.schedule_store_tick(
            StoreTick::ConsistencyCheck,
            self.ctx.cfg.consistency_check_interval.0,
        )
    }

    fn on_consistency_check_tick(&mut self) {
        self.register_consistency_check_tick();
        if self.ctx.consistency_check_scheduler.is_busy() {
            return;
        }
        let (mut target_region_id, mut oldest) = (0, Instant::now());
        let target_peer = {
            let meta = self.ctx.store_meta.lock().unwrap();
            for region_id in meta.regions.keys() {
                match self.fsm.store.consistency_check_time.get(region_id) {
                    Some(time) => {
                        if *time < oldest {
                            oldest = *time;
                            target_region_id = *region_id;
                        }
                    }
                    None => {
                        target_region_id = *region_id;
                        break;
                    }
                }
            }
            if target_region_id == 0 {
                return;
            }
            match util::find_peer(&meta.regions[&target_region_id], self.ctx.store_id()) {
                None => return,
                Some(p) => p.clone(),
            }
        };
        info!(
            "scheduling consistency check for region";
            "store_id" => self.fsm.store.id,
            "region_id" => target_region_id,
        );
        self.fsm
            .store
            .consistency_check_time
            .insert(target_region_id, Instant::now());
        let mut request = new_admin_request(target_region_id, target_peer);
        let mut admin = AdminRequest::new();
        admin.set_cmd_type(AdminCmdType::ComputeHash);
        request.set_admin_request(admin);

        let _ = self.ctx.router.send(
            target_region_id,
            PeerMsg::RaftCommand(RaftCommand::new(request, Callback::None)),
        );
    }

    fn on_cleanup_import_sst_tick(&mut self) {
        if let Err(e) = self.on_cleanup_import_sst() {
            error!(
                "cleanup import sst failed";
                "store_id" => self.fsm.store.id,
                "err" => ?e,
            );
        }
        self.register_cleanup_import_sst_tick();
    }

    fn register_cleanup_import_sst_tick(&self) {
        self.ctx.schedule_store_tick(
            StoreTick::CleanupImportSST,
            self.ctx.cfg.cleanup_import_sst_interval.0,
        )
    }

    fn clear_region_size_in_range(&mut self, start_key: &[u8], end_key: &[u8]) {
        let start_key = data_key(start_key);
        let end_key = data_end_key(end_key);

        let mut regions = vec![];
        {
            let meta = self.ctx.store_meta.lock().unwrap();
            for (_, region_id) in meta
                .region_ranges
                .range((Excluded(start_key), Included(end_key)))
            {
                regions.push(*region_id);
            }
        }
        for region_id in regions {
            let _ = self.ctx.router.send(
                region_id,
                PeerMsg::CasualMessage(CasualMessage::ClearRegionSize),
            );
        }
    }
}

fn size_change_filter(info: &CompactionJobInfo) -> bool {
    // When calculating region size, we only consider write and default
    // column families.
    let cf = info.cf_name();
    if cf != CF_WRITE && cf != CF_DEFAULT {
        return false;
    }
    // Compactions in level 0 and level 1 are very frequently.
    if info.output_level() < 2 {
        return false;
    }

    true
}

pub fn new_compaction_listener(ch: RaftRouter) -> CompactionListener {
    let ch = Mutex::new(ch);
    let compacted_handler = Box::new(move |compacted_event: CompactedEvent| {
        let ch = ch.lock().unwrap();
        if let Err(e) = ch.send_control(StoreMsg::CompactedEvent(compacted_event)) {
            error!(
                "send compaction finished event to raftstore failed"; "err" => ?e,
            );
        }
    });
    CompactionListener::new(compacted_handler, Some(size_change_filter))
}

fn calc_region_declined_bytes(
    event: CompactedEvent,
    region_ranges: &BTreeMap<Key, u64>,
    bytes_threshold: u64,
) -> Vec<(u64, u64)> {
    // Calculate influenced regions.
    let mut influenced_regions = vec![];
    for (end_key, region_id) in
        region_ranges.range((Excluded(event.start_key), Included(event.end_key.clone())))
    {
        influenced_regions.push((region_id, end_key.clone()));
    }
    if let Some((end_key, region_id)) = region_ranges
        .range((Included(event.end_key), Unbounded))
        .next()
    {
        influenced_regions.push((region_id, end_key.clone()));
    }

    // Calculate declined bytes for each region.
    // `end_key` in influenced_regions are in incremental order.
    let mut region_declined_bytes = vec![];
    let mut last_end_key: Vec<u8> = vec![];
    for (region_id, end_key) in influenced_regions {
        let mut old_size = 0;
        for prop in &event.input_props {
            old_size += prop.get_approximate_size_in_range(&last_end_key, &end_key);
        }
        let mut new_size = 0;
        for prop in &event.output_props {
            new_size += prop.get_approximate_size_in_range(&last_end_key, &end_key);
        }
        last_end_key = end_key;

        // Filter some trivial declines for better performance.
        if old_size > new_size && old_size - new_size > bytes_threshold {
            region_declined_bytes.push((*region_id, old_size - new_size));
        }
    }

    region_declined_bytes
}

// check whether the range is covered by existing regions.
fn is_range_covered<'a, F: Fn(u64) -> &'a metapb::Region>(
    region_ranges: &BTreeMap<Key, u64>,
    get_region: F,
    mut start: Vec<u8>,
    end: Vec<u8>,
) -> bool {
    for (end_key, &id) in region_ranges.range((Excluded(start.clone()), Unbounded::<Key>)) {
        let region = get_region(id);
        // find a missing range
        if start < enc_start_key(region) {
            return false;
        }
        if *end_key >= end {
            return true;
        }
        start = end_key.clone();
    }
    false
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::collections::HashMap;

    use crate::util::rocksdb_util::properties::{IndexHandle, IndexHandles, SizeProperties};
    use crate::util::rocksdb_util::CompactedEvent;
    use protobuf::RepeatedField;

    use super::*;

    #[test]
    fn test_calc_region_declined_bytes() {
        let index_handle1 = IndexHandle {
            size: 4 * 1024,
            offset: 4 * 1024,
        };
        let index_handle2 = IndexHandle {
            size: 4 * 1024,
            offset: 8 * 1024,
        };
        let index_handle3 = IndexHandle {
            size: 4 * 1024,
            offset: 12 * 1024,
        };
        let mut index_handles = IndexHandles::new();
        index_handles.add(b"a".to_vec(), index_handle1);
        index_handles.add(b"b".to_vec(), index_handle2);
        index_handles.add(b"c".to_vec(), index_handle3);
        let size_prop = SizeProperties {
            total_size: 12 * 1024,
            index_handles,
        };
        let event = CompactedEvent {
            cf: "default".to_owned(),
            output_level: 3,
            total_input_bytes: 12 * 1024,
            total_output_bytes: 0,
            start_key: size_prop.smallest_key().unwrap(),
            end_key: size_prop.largest_key().unwrap(),
            input_props: vec![size_prop.into()],
            output_props: vec![],
        };

        let mut region_ranges = BTreeMap::new();
        region_ranges.insert(b"a".to_vec(), 1);
        region_ranges.insert(b"b".to_vec(), 2);
        region_ranges.insert(b"c".to_vec(), 3);

        let declined_bytes = calc_region_declined_bytes(event, &region_ranges, 1024);
        let expected_declined_bytes = vec![(2, 8192), (3, 4096)];
        assert_eq!(declined_bytes, expected_declined_bytes);
    }

    #[test]
    fn test_is_range_covered() {
        let meta = vec![(b"b", b"d"), (b"d", b"e"), (b"e", b"f"), (b"f", b"h")];
        let mut region_ranges = BTreeMap::new();
        let mut region_peers = HashMap::new();

        {
            for (i, (start, end)) in meta.into_iter().enumerate() {
                let mut region = metapb::Region::new();
                let peer = metapb::Peer::new();
                region.set_peers(RepeatedField::from_vec(vec![peer]));
                region.set_start_key(start.to_vec());
                region.set_end_key(end.to_vec());

                region_ranges.insert(enc_end_key(&region), i as u64);
                region_peers.insert(i as u64, region);
            }

            let check_range = |start: &[u8], end: &[u8]| {
                is_range_covered(
                    &region_ranges,
                    |id: u64| &region_peers[&id],
                    data_key(start),
                    data_end_key(end),
                )
            };

            assert!(!check_range(b"a", b"c"));
            assert!(check_range(b"b", b"d"));
            assert!(check_range(b"b", b"e"));
            assert!(check_range(b"e", b"f"));
            assert!(check_range(b"b", b"g"));
            assert!(check_range(b"e", b"h"));
            assert!(!check_range(b"e", b"n"));
            assert!(!check_range(b"g", b"n"));
            assert!(!check_range(b"o", b"z"));
            assert!(!check_range(b"", b""));
        }

        let meta = vec![(b"b", b"d"), (b"e", b"f"), (b"f", b"h")];
        region_ranges.clear();
        region_peers.clear();
        {
            for (i, (start, end)) in meta.into_iter().enumerate() {
                let mut region = metapb::Region::new();
                let peer = metapb::Peer::new();
                region.set_peers(RepeatedField::from_vec(vec![peer]));
                region.set_start_key(start.to_vec());
                region.set_end_key(end.to_vec());

                region_ranges.insert(enc_end_key(&region), i as u64);
                region_peers.insert(i as u64, region);
            }

            let check_range = |start: &[u8], end: &[u8]| {
                is_range_covered(
                    &region_ranges,
                    |id: u64| &region_peers[&id],
                    data_key(start),
                    data_end_key(end),
                )
            };

            assert!(!check_range(b"a", b"c"));
            assert!(check_range(b"b", b"d"));
            assert!(!check_range(b"b", b"e"));
            assert!(check_range(b"e", b"f"));
            assert!(!check_range(b"b", b"g"));
            assert!(check_range(b"e", b"g"));
            assert!(check_range(b"e", b"h"));
            assert!(!check_range(b"e", b"n"));
            assert!(!check_range(b"g", b"n"));
            assert!(!check_range(b"o", b"z"));
            assert!(!check_range(b"", b""));
        }
    }
}
