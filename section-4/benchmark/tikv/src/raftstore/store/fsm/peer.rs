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

use std::borrow::Cow;
use std::collections::Bound::{Excluded, Included, Unbounded};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::{cmp, u64};

use futures::Future;
use kvproto::errorpb;
use kvproto::import_sstpb::SSTMeta;
use kvproto::metapb::{self, Region, RegionEpoch};
use kvproto::pdpb::CheckPolicy;
use kvproto::raft_cmdpb::{
    AdminCmdType, AdminRequest, RaftCmdRequest, RaftCmdResponse, StatusCmdType, StatusResponse,
};
use kvproto::raft_serverpb::{
    MergeState, PeerState, RaftMessage, RaftSnapshotData, RaftTruncatedState, RegionLocalState,
};
use protobuf::{Message, RepeatedField};
use raft::eraftpb::ConfChangeType;
use raft::Ready;
use raft::{self, SnapshotStatus, INVALID_INDEX, NO_LIMIT};

use crate::pd::{PdClient, PdTask};
use crate::raftstore::{Error, Result};
use crate::storage::CF_RAFT;
use crate::util::mpsc::{self, LooseBoundedSender, Receiver};
use crate::util::time::duration_to_sec;
use crate::util::worker::{Scheduler, Stopped};
use crate::util::{escape, is_zero_duration};

use crate::raftstore::coprocessor::RegionChangeEvent;
use crate::raftstore::store::cmd_resp::{bind_term, new_error};
use crate::raftstore::store::engine::{Peekable, Snapshot as EngineSnapshot};
use crate::raftstore::store::fsm::store::{PollContext, StoreMeta};
use crate::raftstore::store::fsm::{
    apply, ApplyMetrics, ApplyTask, ApplyTaskRes, BasicMailbox, ChangePeer, ExecResult, Fsm,
    RegionProposal,
};
use crate::raftstore::store::keys::{self, enc_end_key, enc_start_key};
use crate::raftstore::store::metrics::*;
use crate::raftstore::store::msg::Callback;
use crate::raftstore::store::peer::{ConsistencyState, Peer, StaleState, WaitApplyResultState};
use crate::raftstore::store::peer_storage::{ApplySnapResult, InvokeContext};
use crate::raftstore::store::transport::Transport;
use crate::raftstore::store::util::KeysInfoFormatter;
use crate::raftstore::store::worker::{
    CleanupSSTTask, ConsistencyCheckTask, RaftlogGcTask, ReadTask, RegionTask, SplitCheckTask,
};
use crate::raftstore::store::Engines;
use crate::raftstore::store::{
    util, CasualMessage, Config, PeerMsg, PeerTick, RaftCommand, SignificantMsg, SnapKey,
    SnapshotDeleter, StoreMsg,
};

pub struct DestroyPeerJob {
    pub initialized: bool,
    pub async_remove: bool,
    pub region_id: u64,
    pub peer: metapb::Peer,
}

pub struct PeerFsm {
    peer: Peer,
    stopped: bool,
    has_ready: bool,
    mailbox: Option<BasicMailbox<PeerFsm>>,
    pub receiver: Receiver<PeerMsg>,
}

impl Drop for PeerFsm {
    fn drop(&mut self) {
        self.peer.stop();
        while let Ok(msg) = self.receiver.try_recv() {
            let callback = match msg {
                PeerMsg::RaftCommand(cmd) => cmd.callback,
                PeerMsg::CasualMessage(CasualMessage::SplitRegion { callback, .. }) => callback,
                _ => continue,
            };

            let mut err = errorpb::Error::new();
            err.set_message("region is not found".to_owned());
            err.mut_region_not_found().set_region_id(self.region_id());
            let mut resp = RaftCmdResponse::new();
            resp.mut_header().set_error(err);
            callback.invoke_with_response(resp);
        }
    }
}

impl PeerFsm {
    // If we create the peer actively, like bootstrap/split/merge region, we should
    // use this function to create the peer. The region must contain the peer info
    // for this store.
    pub fn create(
        store_id: u64,
        cfg: &Config,
        sched: Scheduler<RegionTask>,
        engines: Engines,
        region: &metapb::Region,
    ) -> Result<(LooseBoundedSender<PeerMsg>, Box<PeerFsm>)> {
        let meta_peer = match util::find_peer(region, store_id) {
            None => {
                return Err(box_err!(
                    "find no peer for store {} in region {:?}",
                    store_id,
                    region
                ));
            }
            Some(peer) => peer.clone(),
        };

        info!(
            "create peer";
            "region_id" => region.get_id(),
            "peer_id" => meta_peer.get_id(),
        );
        let (tx, rx) = mpsc::loose_bounded(cfg.notify_capacity);
        Ok((
            tx,
            Box::new(PeerFsm {
                peer: Peer::new(store_id, cfg, sched, engines, region, meta_peer)?,
                stopped: false,
                has_ready: false,
                mailbox: None,
                receiver: rx,
            }),
        ))
    }

    // The peer can be created from another node with raft membership changes, and we only
    // know the region_id and peer_id when creating this replicated peer, the region info
    // will be retrieved later after applying snapshot.
    pub fn replicate(
        store_id: u64,
        cfg: &Config,
        sched: Scheduler<RegionTask>,
        engines: Engines,
        region_id: u64,
        peer: metapb::Peer,
    ) -> Result<(LooseBoundedSender<PeerMsg>, Box<PeerFsm>)> {
        // We will remove tombstone key when apply snapshot
        info!(
            "replicate peer";
            "region_id" => region_id,
            "peer_id" => peer.get_id(),
        );

        let mut region = metapb::Region::new();
        region.set_id(region_id);

        let (tx, rx) = mpsc::loose_bounded(cfg.notify_capacity);
        Ok((
            tx,
            Box::new(PeerFsm {
                peer: Peer::new(store_id, cfg, sched, engines, &region, peer)?,
                stopped: false,
                has_ready: false,
                mailbox: None,
                receiver: rx,
            }),
        ))
    }

    #[inline]
    pub fn region_id(&self) -> u64 {
        self.peer.region().get_id()
    }

    #[inline]
    pub fn get_peer(&self) -> &Peer {
        &self.peer
    }

    #[inline]
    pub fn peer_id(&self) -> u64 {
        self.peer.peer_id()
    }

    #[inline]
    pub fn stop(&mut self) {
        self.stopped = true;
    }

    pub fn set_pending_merge_state(&mut self, state: MergeState) {
        self.peer.pending_merge_state = Some(state);
    }

    pub fn schedule_applying_snapshot(&mut self) {
        self.peer.mut_store().schedule_applying_snapshot();
    }

    pub fn have_pending_merge_apply_result(&self) -> bool {
        self.peer.pending_merge_apply_result.is_some()
    }
}

impl Fsm for PeerFsm {
    type Message = PeerMsg;

    #[inline]
    fn is_stopped(&self) -> bool {
        self.stopped
    }

    /// Set a mailbox to Fsm, which should be used to send message to itself.
    #[inline]
    fn set_mailbox(&mut self, mailbox: Cow<'_, BasicMailbox<Self>>)
    where
        Self: Sized,
    {
        self.mailbox = Some(mailbox.into_owned());
    }

    /// Take the mailbox from Fsm. Implementation should ensure there will be
    /// no reference to mailbox after calling this method.
    #[inline]
    fn take_mailbox(&mut self) -> Option<BasicMailbox<Self>>
    where
        Self: Sized,
    {
        self.mailbox.take()
    }
}

pub struct PeerFsmDelegate<'a, T: 'static, C: 'static> {
    fsm: &'a mut PeerFsm,
    ctx: &'a mut PollContext<T, C>,
}

impl<'a, T: Transport, C: PdClient> PeerFsmDelegate<'a, T, C> {
    pub fn new(fsm: &'a mut PeerFsm, ctx: &'a mut PollContext<T, C>) -> PeerFsmDelegate<'a, T, C> {
        PeerFsmDelegate { fsm, ctx }
    }

    pub fn handle_msgs(&mut self, msgs: &mut Vec<PeerMsg>) {
        for m in msgs.drain(..) {
            match m {
                PeerMsg::RaftMessage(msg) => {
                    if let Err(e) = self.on_raft_message(msg) {
                        error!(
                            "handle raft message err";
                            "region_id" => self.fsm.region_id(),
                            "peer_id" => self.fsm.peer_id(),
                            "err" => %e,
                        );
                    }
                }
                PeerMsg::RaftCommand(cmd) => {
                    self.ctx
                        .raft_metrics
                        .propose
                        .request_wait_time
                        .observe(duration_to_sec(cmd.send_time.elapsed()) as f64);
                    self.propose_raft_command(cmd.request, cmd.callback)
                }
                PeerMsg::Tick(tick) => self.on_tick(tick),
                PeerMsg::ApplyRes { res } => {
                    if let Some(state) = self.fsm.peer.pending_merge_apply_result.as_mut() {
                        state.results.push(res);
                        continue;
                    }
                    self.on_apply_res(res);
                }
                PeerMsg::SignificantMsg(msg) => self.on_significant_msg(msg),
                PeerMsg::CasualMessage(msg) => self.on_casual_msg(msg),
                PeerMsg::Start => self.start(),
                PeerMsg::Noop => {}
            }
        }
    }

    fn on_casual_msg(&mut self, msg: CasualMessage) {
        match msg {
            CasualMessage::SplitRegion {
                region_epoch,
                split_keys,
                callback,
            } => {
                info!(
                    "on split";
                    "region_id" => self.fsm.region_id(),
                    "peer_id" => self.fsm.peer_id(),
                    "split_keys" => %KeysInfoFormatter(&split_keys),
                );
                self.on_prepare_split_region(region_epoch, split_keys, callback);
            }
            CasualMessage::ComputeHashResult { index, hash } => {
                self.on_hash_computed(index, hash);
            }
            CasualMessage::RegionApproximateSize { size } => {
                self.on_approximate_region_size(size);
            }
            CasualMessage::RegionApproximateKeys { keys } => {
                self.on_approximate_region_keys(keys);
            }
            CasualMessage::CompactionDeclinedBytes { bytes } => {
                self.on_compaction_declined_bytes(bytes);
            }
            CasualMessage::HalfSplitRegion {
                region_epoch,
                policy,
            } => {
                self.on_schedule_half_split_region(&region_epoch, policy);
            }
            CasualMessage::MergeResult { target, stale } => {
                self.on_merge_result(target, stale);
            }
            CasualMessage::GcSnap { snaps } => {
                self.on_gc_snap(snaps);
            }
            CasualMessage::ClearRegionSize => {
                self.on_clear_region_size();
            }
        }
    }

    fn on_tick(&mut self, tick: PeerTick) {
        if self.fsm.stopped {
            return;
        }
        match tick {
            PeerTick::Raft => self.on_raft_base_tick(),
            PeerTick::RaftLogGc => self.on_raft_gc_log_tick(),
            PeerTick::PdHeartbeat => self.on_pd_heartbeat_tick(),
            PeerTick::SplitRegionCheck => self.on_split_region_check_tick(),
            PeerTick::CheckMerge => self.on_check_merge(),
            PeerTick::CheckPeerStaleState => self.on_check_peer_stale_state_tick(),
        }
    }

    fn start(&mut self) {
        if self.fsm.peer.pending_merge_state.is_some() {
            self.notify_prepare_merge();
        }
        self.register_raft_base_tick();
        self.register_raft_gc_log_tick();
        self.register_pd_heartbeat_tick();
        self.register_split_region_check_tick();
        self.register_check_peer_stale_state_tick();
        self.on_check_merge();
    }

    fn notify_prepare_merge(&self) {
        let region_id = self.region_id();
        let version = self.region().get_region_epoch().get_version();
        // If there is no merge lock for that key, insert one to let target peer know `PrepareMerge`
        // is already executed.
        let mut meta = self.ctx.store_meta.lock().unwrap();
        let (exist_version, ready_to_merge) =
            match meta.merge_locks.insert(region_id, (version, None)) {
                None => return,
                Some((v, r)) => (v, r),
            };
        if exist_version == version {
            let ready_to_merge = ready_to_merge.unwrap();
            // Set `ready_to_merge` to true to indicate `PrepareMerge` is finished.
            ready_to_merge.store(true, Ordering::SeqCst);
            let state = self.fsm.peer.pending_merge_state.as_ref().unwrap();
            let target_region_id = state.get_target().get_id();
            // Send an empty message to target peer to make sure it will check `ready_to_merge`
            self.ctx
                .router
                .force_send(target_region_id, PeerMsg::Noop)
                .unwrap();
        } else if exist_version > version {
            meta.merge_locks
                .insert(region_id, (exist_version, ready_to_merge));
        } else {
            panic!(
                "{} expects version {} but got {}",
                self.fsm.peer.tag, version, exist_version
            );
        }
    }

    pub fn resume_handling_pending_apply_result(&mut self) -> bool {
        match self.fsm.peer.pending_merge_apply_result {
            Some(ref state) => {
                if !state.ready_to_merge.load(Ordering::SeqCst) {
                    return false;
                }
            }
            None => panic!(
                "{} doesn't have pending apply result, can't be resume.",
                self.fsm.peer.tag
            ),
        }

        let mut pending_apply = self.fsm.peer.pending_merge_apply_result.take().unwrap();
        let mut drainer = pending_apply.results.drain(..);
        while let Some(res) = drainer.next() {
            debug!(
                "resume handling apply result";
                "region_id" => self.region_id(),
                "peer_id" => self.fsm.peer_id(),
                "res" => ?res,
            );
            self.on_apply_res(res);
            // So meet another `CommitMerge` apply result needed to wait.
            if let Some(state) = self.fsm.peer.pending_merge_apply_result.as_mut() {
                state.results.extend(drainer);
                return false;
            }
        }
        true
    }

    fn on_gc_snap(&mut self, snaps: Vec<(SnapKey, bool)>) {
        let s = self.fsm.peer.get_store();
        let compacted_idx = s.truncated_index();
        let compacted_term = s.truncated_term();
        let is_applying_snap = s.is_applying_snapshot();
        for (key, is_sending) in snaps {
            if is_sending {
                let s = match self.ctx.snap_mgr.get_snapshot_for_sending(&key) {
                    Ok(s) => s,
                    Err(e) => {
                        error!(
                            "failed to load snapshot";
                            "region_id" => self.fsm.region_id(),
                            "peer_id" => self.fsm.peer_id(),
                            "snapshot" => ?key,
                            "err" => %e,
                        );
                        continue;
                    }
                };
                if key.term < compacted_term || key.idx < compacted_idx {
                    info!(
                        "deleting compacted snap file";
                        "region_id" => self.fsm.region_id(),
                        "peer_id" => self.fsm.peer_id(),
                        "snap_file" => %key,
                    );
                    self.ctx.snap_mgr.delete_snapshot(&key, s.as_ref(), false);
                } else if let Ok(meta) = s.meta() {
                    let modified = match meta.modified() {
                        Ok(m) => m,
                        Err(e) => {
                            error!(
                                "failed to load snapshot";
                                "region_id" => self.fsm.region_id(),
                                "peer_id" => self.fsm.peer_id(),
                                "snapshot" => ?key,
                                "err" => %e,
                            );
                            continue;
                        }
                    };
                    if let Ok(elapsed) = modified.elapsed() {
                        if elapsed > self.ctx.cfg.snap_gc_timeout.0 {
                            info!(
                                "deleting expired snap file";
                                "region_id" => self.fsm.region_id(),
                                "peer_id" => self.fsm.peer_id(),
                                "snap_file" => %key,
                            );
                            self.ctx.snap_mgr.delete_snapshot(&key, s.as_ref(), false);
                        }
                    }
                }
            } else if key.term <= compacted_term
                && (key.idx < compacted_idx || key.idx == compacted_idx && !is_applying_snap)
            {
                info!(
                    "deleting applied snap file";
                    "region_id" => self.fsm.region_id(),
                    "peer_id" => self.fsm.peer_id(),
                    "snap_file" => %key,
                );
                let a = match self.ctx.snap_mgr.get_snapshot_for_applying(&key) {
                    Ok(a) => a,
                    Err(e) => {
                        error!(
                            "failed to load snapshot";
                            "region_id" => self.fsm.region_id(),
                            "peer_id" => self.fsm.peer_id(),
                            "snap_file" => %key,
                            "err" => %e,
                        );
                        continue;
                    }
                };
                self.ctx.snap_mgr.delete_snapshot(&key, a.as_ref(), false);
            }
        }
    }

    fn on_clear_region_size(&mut self) {
        self.fsm.peer.approximate_size = None;
        self.fsm.peer.approximate_keys = None;
    }

    fn on_significant_msg(&mut self, msg: SignificantMsg) {
        match msg {
            SignificantMsg::SnapshotStatus {
                to_peer_id, status, ..
            } => {
                // Report snapshot status to the corresponding peer.
                self.report_snapshot_status(to_peer_id, status);
            }
            SignificantMsg::Unreachable { to_peer_id, .. } => {
                self.fsm.peer.raft_group.report_unreachable(to_peer_id);
            }
        }
    }

    fn report_snapshot_status(&mut self, to_peer_id: u64, status: SnapshotStatus) {
        let to_peer = match self.fsm.peer.get_peer_from_cache(to_peer_id) {
            Some(peer) => peer,
            None => {
                // If to_peer is gone, ignore this snapshot status
                warn!(
                    "peer not found, ignore snapshot status";
                    "region_id" => self.region_id(),
                    "peer_id" => self.fsm.peer_id(),
                    "to_peer_id" => to_peer_id,
                    "status" => ?status,
                );
                return;
            }
        };
        info!(
            "report snapshot status";
            "region_id" => self.fsm.region_id(),
            "peer_id" => self.fsm.peer_id(),
            "to" => ?to_peer,
            "status" => ?status,
        );
        self.fsm.peer.raft_group.report_snapshot(to_peer_id, status)
    }

    pub fn collect_ready(&mut self, proposals: &mut Vec<RegionProposal>) {
        let has_ready = self.fsm.has_ready;
        self.fsm.has_ready = false;
        if !has_ready || self.fsm.stopped {
            return;
        }
        self.ctx.pending_count += 1;
        self.ctx.has_ready = true;
        if let Some(p) = self.fsm.peer.take_apply_proposals() {
            proposals.push(p);
        }
        self.fsm.peer.handle_raft_ready_append(self.ctx);
    }

    pub fn post_raft_ready_append(&mut self, mut ready: Ready, invoke_ctx: InvokeContext) {
        let is_merging = self.fsm.peer.pending_merge_state.is_some();
        let res = self
            .fsm
            .peer
            .post_raft_ready_append(self.ctx, &mut ready, invoke_ctx);
        self.fsm.peer.handle_raft_ready_apply(self.ctx, ready);
        let mut has_snapshot = false;
        if let Some(apply_res) = res {
            self.on_ready_apply_snapshot(apply_res);
            has_snapshot = true;
        }
        if is_merging && has_snapshot {
            // After applying a snapshot, merge is rollbacked implicitly.
            self.on_ready_rollback_merge(0, None);
        }
    }

    #[inline]
    fn region_id(&self) -> u64 {
        self.fsm.peer.region().get_id()
    }

    #[inline]
    fn region(&self) -> &Region {
        self.fsm.peer.region()
    }

    #[inline]
    fn store_id(&self) -> u64 {
        self.fsm.peer.peer.get_store_id()
    }

    #[inline]
    fn schedule_tick(&self, tick: PeerTick, timeout: Duration) {
        if is_zero_duration(&timeout) {
            return;
        }

        let region_id = self.region_id();
        let mb = match self.ctx.router.mailbox(region_id) {
            Some(mb) => mb,
            None => {
                error!(
                    "failed to get mailbox";
                    "region_id" => self.fsm.region_id(),
                    "peer_id" => self.fsm.peer_id(),
                    "tick" => ?tick,
                );
                return;
            }
        };
        let peer_id = self.fsm.peer.peer_id();
        let f = self
            .ctx
            .timer
            .delay(timeout)
            .map(move |_| {
                fail_point!(
                    "on_raft_log_gc_tick_1",
                    peer_id == 1 && tick == PeerTick::RaftLogGc,
                    |_| unreachable!()
                );
                if let Err(e) = mb.force_send(PeerMsg::Tick(tick)) {
                    info!(
                        "failed to schedule peer tick";
                        "region_id" => region_id,
                        "peer_id" => peer_id,
                        "tick" => ?tick,
                        "err" => %e,
                    );
                }
            })
            .map_err(move |e| {
                panic!(
                    "[region {}] {} tick {:?} is lost due to timeout error: {:?}",
                    region_id, peer_id, tick, e
                );
            });
        self.ctx.future_poller.spawn(f).unwrap();
    }

    fn register_raft_base_tick(&self) {
        // If we register raft base tick failed, the whole raft can't run correctly,
        // TODO: shutdown the store?
        self.schedule_tick(PeerTick::Raft, self.ctx.cfg.raft_base_tick_interval.0)
    }

    fn on_raft_base_tick(&mut self) {
        if self.fsm.peer.pending_remove {
            self.fsm.peer.mut_store().flush_cache_metrics();
            return;
        }
        // When having pending snapshot, if election timeout is met, it can't pass
        // the pending conf change check because first index has been updated to
        // a value that is larger than last index.
        if self.fsm.peer.is_applying_snapshot() || self.fsm.peer.has_pending_snapshot() {
            // need to check if snapshot is applied.
            self.fsm.has_ready = true;
            self.register_raft_base_tick();
            return;
        }
        if self.fsm.peer.raft_group.tick() {
            self.fsm.has_ready = true;
        }

        self.fsm.peer.mut_store().flush_cache_metrics();
        self.register_raft_base_tick();
    }

    fn on_apply_res(&mut self, res: ApplyTaskRes) {
        match res {
            ApplyTaskRes::Apply(mut res) => {
                debug!(
                    "async apply finish";
                    "region_id" => self.region_id(),
                    "peer_id" => self.fsm.peer_id(),
                    "res" => ?res,
                );
                if let Some(ready_to_merge) =
                    self.on_ready_result(res.merged, &mut res.exec_res, &res.metrics)
                {
                    // There is a `CommitMerge` needed to wait
                    self.fsm.peer.pending_merge_apply_result = Some(WaitApplyResultState {
                        results: vec![ApplyTaskRes::Apply(res)],
                        ready_to_merge,
                    });
                    return;
                }
                if self.fsm.stopped {
                    return;
                }
                self.fsm.has_ready |= self.fsm.peer.post_apply(
                    self.ctx,
                    res.apply_state,
                    res.applied_index_term,
                    res.merged,
                    &res.metrics,
                );
            }
            ApplyTaskRes::Destroy { peer_id, .. } => {
                assert_eq!(peer_id, self.fsm.peer.peer_id());
                self.destroy_peer(false);
            }
        }
    }

    fn on_raft_message(&mut self, mut msg: RaftMessage) -> Result<()> {
        debug!(
            "handle raft message";
            "region_id" => self.region_id(),
            "peer_id" => self.fsm.peer_id(),
            "message_type" => ?msg.get_message().get_msg_type(),
            "from_peer_id" => msg.get_from_peer().get_id(),
            "to_peer_id" => msg.get_to_peer().get_id(),
        );

        if !self.validate_raft_msg(&msg) {
            return Ok(());
        }
        if self.fsm.peer.pending_remove || self.fsm.stopped {
            return Ok(());
        }

        if msg.get_is_tombstone() {
            // we receive a message tells us to remove ourself.
            self.handle_gc_peer_msg(&msg);
            return Ok(());
        }

        if msg.has_merge_target() {
            if self.need_gc_merge(&msg)? {
                self.on_stale_merge();
            }
            return Ok(());
        }

        if self.check_msg(&msg) {
            return Ok(());
        }

        if let Some(key) = self.check_snapshot(&msg)? {
            // If the snapshot file is not used again, then it's OK to
            // delete them here. If the snapshot file will be reused when
            // receiving, then it will fail to pass the check again, so
            // missing snapshot files should not be noticed.
            let s = self.ctx.snap_mgr.get_snapshot_for_applying(&key)?;
            self.ctx.snap_mgr.delete_snapshot(&key, s.as_ref(), false);
            return Ok(());
        }

        let from_peer_id = msg.get_from_peer().get_id();
        self.fsm.peer.insert_peer_cache(msg.take_from_peer());
        self.fsm.peer.step(msg.take_message())?;

        if self.fsm.peer.any_new_peer_catch_up(from_peer_id) {
            self.fsm.peer.heartbeat_pd(self.ctx);
        }

        self.fsm.has_ready = true;
        Ok(())
    }

    // return false means the message is invalid, and can be ignored.
    fn validate_raft_msg(&mut self, msg: &RaftMessage) -> bool {
        let region_id = msg.get_region_id();
        let to = msg.get_to_peer();

        if to.get_store_id() != self.store_id() {
            warn!(
                "store not match, ignore it";
                "region_id" => region_id,
                "to_store_id" => to.get_store_id(),
                "my_store_id" => self.store_id(),
            );
            self.ctx.raft_metrics.message_dropped.mismatch_store_id += 1;
            return false;
        }

        if !msg.has_region_epoch() {
            error!(
                "missing epoch in raft message, ignore it";
                "region_id" => region_id,
            );
            self.ctx.raft_metrics.message_dropped.mismatch_region_epoch += 1;
            return false;
        }

        true
    }

    /// Checks if the message is sent to the correct peer.
    ///
    /// Returns true means that the message can be dropped silently.
    fn check_msg(&mut self, msg: &RaftMessage) -> bool {
        let from_epoch = msg.get_region_epoch();
        let is_vote_msg = util::is_vote_msg(msg.get_message());
        let from_store_id = msg.get_from_peer().get_store_id();

        // Let's consider following cases with three nodes [1, 2, 3] and 1 is leader:
        // a. 1 removes 2, 2 may still send MsgAppendResponse to 1.
        //  We should ignore this stale message and let 2 remove itself after
        //  applying the ConfChange log.
        // b. 2 is isolated, 1 removes 2. When 2 rejoins the cluster, 2 will
        //  send stale MsgRequestVote to 1 and 3, at this time, we should tell 2 to gc itself.
        // c. 2 is isolated but can communicate with 3. 1 removes 3.
        //  2 will send stale MsgRequestVote to 3, 3 should ignore this message.
        // d. 2 is isolated but can communicate with 3. 1 removes 2, then adds 4, remove 3.
        //  2 will send stale MsgRequestVote to 3, 3 should tell 2 to gc itself.
        // e. 2 is isolated. 1 adds 4, 5, 6, removes 3, 1. Now assume 4 is leader.
        //  After 2 rejoins the cluster, 2 may send stale MsgRequestVote to 1 and 3,
        //  1 and 3 will ignore this message. Later 4 will send messages to 2 and 2 will
        //  rejoin the raft group again.
        // f. 2 is isolated. 1 adds 4, 5, 6, removes 3, 1. Now assume 4 is leader, and 4 removes 2.
        //  unlike case e, 2 will be stale forever.
        // TODO: for case f, if 2 is stale for a long time, 2 will communicate with pd and pd will
        // tell 2 is stale, so 2 can remove itself.
        if util::is_epoch_stale(from_epoch, self.fsm.peer.region().get_region_epoch())
            && util::find_peer(self.fsm.peer.region(), from_store_id).is_none()
        {
            // The message is stale and not in current region.
            self.ctx.handle_stale_msg(
                msg,
                self.fsm.peer.region().get_region_epoch().clone(),
                is_vote_msg,
                None,
            );
            return true;
        }

        let target = msg.get_to_peer();
        if target.get_id() < self.fsm.peer.peer_id() {
            info!(
                "target peer id is smaller, msg maybe stale";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
                "target_peer" => ?target,
            );
            self.ctx.raft_metrics.message_dropped.stale_msg += 1;
            true
        } else if target.get_id() > self.fsm.peer.peer_id() {
            match self.fsm.peer.maybe_destroy() {
                Some(job) => {
                    info!(
                        "target peer id is larger, destroying self";
                        "region_id" => self.fsm.region_id(),
                        "peer_id" => self.fsm.peer_id(),
                        "target_peer" => ?target,
                    );
                    if self.handle_destroy_peer(job) {
                        if let Err(e) = self
                            .ctx
                            .router
                            .send_control(StoreMsg::RaftMessage(msg.clone()))
                        {
                            info!(
                                "failed to send back store message, are we shutting down?";
                                "region_id" => self.fsm.region_id(),
                                "peer_id" => self.fsm.peer_id(),
                                "err" => %e,
                            );
                        }
                    }
                }
                None => self.ctx.raft_metrics.message_dropped.applying_snap += 1,
            }
            true
        } else {
            false
        }
    }

    /// Check if it's necessary to gc the source merge peer.
    ///
    /// If the target merge peer won't be created on this store,
    /// then it's appropriate to destroy it immediately.
    fn need_gc_merge(&mut self, msg: &RaftMessage) -> Result<bool> {
        let merge_target = msg.get_merge_target();
        let target_region_id = merge_target.get_id();
        debug!(
            "receive merge target";
            "region_id" => self.fsm.region_id(),
            "peer_id" => self.fsm.peer_id(),
            "merge_target" => ?merge_target,
        );

        // When receiving message that has a merge target, it indicates that the source peer on this
        // store is stale, the peers on other stores are already merged. The epoch in merge target
        // is the state of target peer at the time when source peer is merged. So here we record the
        // merge target epoch version to let the target peer on this store to decide whether to
        // destroy the source peer.
        let mut meta = self.ctx.store_meta.lock().unwrap();
        meta.targets_map.insert(self.region_id(), target_region_id);
        let v = meta
            .pending_merge_targets
            .entry(target_region_id)
            .or_default();
        if let Some(epoch) = (*v).insert(self.region_id(), merge_target.get_region_epoch().clone())
        {
            // Merge target epoch records the version of target region when source region is merged.
            // So it must be same no matter when receiving merge target.
            if epoch.get_version() != merge_target.get_region_epoch().get_version() {
                panic!(
                    "conflict merge target epoch version {:?} {:?}",
                    epoch,
                    merge_target.get_region_epoch()
                );
            }
        }
        if let Some(epoch) = meta
            .regions
            .get(&target_region_id)
            .map(|r| r.get_region_epoch())
        {
            // In the case that the source peer's range isn't overlapped with target's anymore:
            //     | region 2 | region 3 | region 1 |
            //                   || merge 3 into 2
            //                   \/
            //     |       region 2      | region 1 |
            //                   || merge 1 into 2
            //                   \/
            //     |            region 2            |
            //                   || split 2 into 4
            //                   \/
            //     |        region 4       |region 2|
            // so the new target peer can't find the source peer.
            // e.g. new region 2 is overlapped with region 1
            //
            // If that, source peer still need to decide whether to destroy itself. When the target
            // peer has already moved on, source peer can destroy itself.
            if epoch.get_version() > merge_target.get_region_epoch().get_version() {
                return Ok(true);
            }
            return Ok(false);
        }

        // Check whether target peer is set to tombstone already.
        let state_key = keys::region_state_key(target_region_id);
        if let Some(state) = self
            .ctx
            .engines
            .kv
            .get_msg_cf::<RegionLocalState>(CF_RAFT, &state_key)?
        {
            debug!(
                "check target region local state";
                "region_id" => self.region_id(),
                "peer_id" => self.fsm.peer_id(),
                "target_region_id" => target_region_id,
                "state" => ?state,
            );
            if state.get_state() == PeerState::Tombstone
                && state.get_region().get_region_epoch().get_conf_ver()
                    >= merge_target.get_region_epoch().get_conf_ver()
            {
                // Replica was destroyed.
                return Ok(true);
            }
        }

        info!(
            "no replica of target region exist, check pd.";
            "region_id" => self.fsm.region_id(),
            "peer_id" => self.fsm.peer_id(),
            "target_region_id" => target_region_id,
        );
        // We can't know whether the peer is destroyed or not for sure locally, ask
        // pd for help.
        let target_peer = merge_target
            .get_peers()
            .iter()
            .find(|p| p.get_store_id() == self.store_id())
            .unwrap();
        let task = PdTask::ValidatePeer {
            peer: target_peer.to_owned(),
            region: merge_target.to_owned(),
            merge_source: Some(self.region_id()),
        };
        if let Err(e) = self.ctx.pd_scheduler.schedule(task) {
            error!(
                "failed to validate target peer";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
                "target_peer" => ?target_peer,
                "err" => %e,
            );
        }
        Ok(false)
    }

    fn handle_gc_peer_msg(&mut self, msg: &RaftMessage) {
        let from_epoch = msg.get_region_epoch();
        if !util::is_epoch_stale(self.fsm.peer.region().get_region_epoch(), from_epoch) {
            return;
        }

        if self.fsm.peer.peer != *msg.get_to_peer() {
            info!(
                "receive stale gc message, ignore.";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
            );
            self.ctx.raft_metrics.message_dropped.stale_msg += 1;
            return;
        }
        // TODO: ask pd to guarantee we are stale now.
        info!(
            "receives gc message, trying to remove";
            "region_id" => self.fsm.region_id(),
            "peer_id" => self.fsm.peer_id(),
            "to_peer" => ?msg.get_to_peer(),
        );
        match self.fsm.peer.maybe_destroy() {
            None => self.ctx.raft_metrics.message_dropped.applying_snap += 1,
            Some(job) => {
                self.handle_destroy_peer(job);
            }
        }
    }

    // Returns `None` if the `msg` doesn't contain a snapshot or it contains a snapshot which
    // doesn't conflict with any other snapshots or regions. Otherwise a `SnapKey` is returned.
    fn check_snapshot(&mut self, msg: &RaftMessage) -> Result<Option<SnapKey>> {
        if !msg.get_message().has_snapshot() {
            return Ok(None);
        }

        let region_id = msg.get_region_id();
        let snap = msg.get_message().get_snapshot();
        let key = SnapKey::from_region_snap(region_id, snap);
        let mut snap_data = RaftSnapshotData::new();
        snap_data.merge_from_bytes(snap.get_data())?;
        let snap_region = snap_data.take_region();
        let peer_id = msg.get_to_peer().get_id();
        let snap_enc_start_key = enc_start_key(&snap_region);
        let snap_enc_end_key = enc_end_key(&snap_region);

        if snap_region
            .get_peers()
            .iter()
            .all(|p| p.get_id() != peer_id)
        {
            info!(
                "snapshot doesn't contain to peer, skip";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
                "snap" => ?snap_region,
                "to_peer" => ?msg.get_to_peer(),
            );
            self.ctx.raft_metrics.message_dropped.region_no_peer += 1;
            return Ok(Some(key));
        }

        let mut meta = self.ctx.store_meta.lock().unwrap();
        if meta.regions[&self.region_id()] != *self.region() {
            if !self.fsm.peer.is_initialized() {
                info!(
                    "stale delegate detected, skip";
                    "region_id" => self.fsm.region_id(),
                    "peer_id" => self.fsm.peer_id(),
                );
                self.ctx.raft_metrics.message_dropped.stale_msg += 1;
                return Ok(Some(key));
            } else {
                panic!(
                    "{} meta corrupted: {:?} != {:?}",
                    self.fsm.peer.tag,
                    meta.regions[&self.region_id()],
                    self.region()
                );
            }
        }
        for region in &meta.pending_snapshot_regions {
            if enc_start_key(region) < snap_enc_end_key &&
               enc_end_key(region) > snap_enc_start_key &&
               // Same region can overlap, we will apply the latest version of snapshot.
               region.get_id() != snap_region.get_id()
            {
                info!(
                    "pending region overlapped";
                    "region_id" => self.fsm.region_id(),
                    "peer_id" => self.fsm.peer_id(),
                    "region" => ?region,
                    "snap" => ?snap_region,
                );
                self.ctx.raft_metrics.message_dropped.region_overlap += 1;
                return Ok(Some(key));
            }
        }

        let mut regions_to_destroy = vec![];
        // In some extreme cases, it may cause source peer destroyed improperly so that a later
        // CommitMerge may panic because source is already destroyed, so just drop the message:
        // 1. A new snapshot is received whereas a snapshot is still in applying, and the snapshot
        // under applying is generated before merge and the new snapshot is generated after merge.
        // After the applying snapshot is finished, the log may able to catch up and so a
        // CommitMerge will be applied.
        // 2. There is a CommitMerge pending in apply thread.
        let ready = !self.fsm.peer.is_applying_snapshot()
            && !self.fsm.peer.has_pending_snapshot()
            && self.fsm.peer.ready_to_handle_pending_snap();
        for exist_region in meta
            .region_ranges
            .range((Excluded(snap_enc_start_key), Unbounded::<Vec<u8>>))
            .map(|(_, &region_id)| &meta.regions[&region_id])
            .take_while(|r| enc_start_key(r) < snap_enc_end_key)
            .filter(|r| r.get_id() != region_id)
        {
            info!(
                "region overlapped";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
                "exist" => ?exist_region,
                "snap" => ?snap_region,
            );
            if ready
                && maybe_destroy_source(
                    &meta,
                    self.region_id(),
                    exist_region.get_id(),
                    snap_region.get_region_epoch().to_owned(),
                )
            {
                // The snapshot that we decide to whether destroy peer based on must can be applied.
                // So here not to destroy peer immediately, or the snapshot maybe dropped in later
                // check but the peer is already destroyed.
                regions_to_destroy.push(exist_region.get_id());
                continue;
            }
            self.ctx.raft_metrics.message_dropped.region_overlap += 1;
            return Ok(Some(key));
        }

        // Check if snapshot file exists.
        self.ctx.snap_mgr.get_snapshot_for_applying(&key)?;

        meta.pending_snapshot_regions.push(snap_region);
        self.ctx.queued_snapshot.insert(region_id);
        for region_id in regions_to_destroy {
            self.ctx
                .router
                .force_send(
                    region_id,
                    PeerMsg::CasualMessage(CasualMessage::MergeResult {
                        target: self.fsm.peer.peer.clone(),
                        stale: true,
                    }),
                )
                .unwrap();
        }

        Ok(None)
    }

    fn handle_destroy_peer(&mut self, job: DestroyPeerJob) -> bool {
        if job.initialized {
            self.ctx
                .apply_router
                .schedule_task(job.region_id, ApplyTask::destroy(job.region_id));
        }
        if job.async_remove {
            info!(
                "peer is destroyed asynchronously";
                "region_id" => job.region_id,
                "peer_id" => job.peer.get_id(),
            );
            false
        } else {
            self.destroy_peer(false);
            true
        }
    }

    fn destroy_peer(&mut self, merged_by_target: bool) {
        info!(
            "starts destroy";
            "region_id" => self.fsm.region_id(),
            "peer_id" => self.fsm.peer_id(),
            "merged_by_target" => merged_by_target,
        );
        let region_id = self.region_id();
        // We can't destroy a peer which is applying snapshot.
        assert!(!self.fsm.peer.is_applying_snapshot());

        // Clear merge related structures.
        let mut meta = self.ctx.store_meta.lock().unwrap();
        meta.pending_merge_targets.remove(&region_id);
        if let Some(target) = meta.targets_map.remove(&region_id) {
            if meta.pending_merge_targets.contains_key(&target) {
                meta.pending_merge_targets
                    .get_mut(&target)
                    .unwrap()
                    .remove(&region_id);
                // When the target doesn't exist(add peer but the store is isolated), source peer decide to destroy by itself.
                // Without target, the `pending_merge_targets` for target won't be removed, so here source peer help target to clear.
                if meta.regions.get(&target).is_none()
                    && meta.pending_merge_targets.get(&target).unwrap().is_empty()
                {
                    meta.pending_merge_targets.remove(&target);
                }
            }
        }
        meta.merge_locks.remove(&region_id);

        // Destroy read delegates.
        if self
            .ctx
            .local_reader
            .schedule(ReadTask::destroy(region_id))
            .is_err()
        {
            info!(
                "unable to destroy read delegate, are we shutting down?";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
            );
        }
        self.ctx
            .apply_router
            .schedule_task(region_id, ApplyTask::destroy(region_id));

        // Trigger region change observer
        self.ctx.coprocessor_host.on_region_changed(
            self.fsm.peer.region(),
            RegionChangeEvent::Destroy,
            self.fsm.peer.get_role(),
        );
        let task = PdTask::DestroyPeer { region_id };
        if let Err(e) = self.ctx.pd_scheduler.schedule(task) {
            error!(
                "failed to notify pd";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
                "err" => %e,
            );
        }
        let is_initialized = self.fsm.peer.is_initialized();
        if let Err(e) = self.fsm.peer.destroy(self.ctx, merged_by_target) {
            // If not panic here, the peer will be recreated in the next restart,
            // then it will be gc again. But if some overlap region is created
            // before restarting, the gc action will delete the overlap region's
            // data too.
            panic!("{} destroy err {:?}", self.fsm.peer.tag, e);
        }
        self.ctx.router.close(region_id);
        self.fsm.stop();

        if is_initialized
            && !merged_by_target
            && meta
                .region_ranges
                .remove(&enc_end_key(self.fsm.peer.region()))
                .is_none()
        {
            panic!("{} meta corruption detected", self.fsm.peer.tag,);
        }
        if meta.regions.remove(&region_id).is_none() && !merged_by_target {
            panic!("{} meta corruption detected", self.fsm.peer.tag,)
        }
    }

    fn on_ready_change_peer(&mut self, cp: ChangePeer) {
        let change_type = cp.conf_change.get_change_type();
        self.fsm.peer.raft_group.apply_conf_change(&cp.conf_change);
        if cp.conf_change.get_node_id() == raft::INVALID_ID {
            // Apply failed, skip.
            return;
        }
        {
            let mut meta = self.ctx.store_meta.lock().unwrap();
            meta.set_region(
                &self.ctx.coprocessor_host,
                &self.ctx.local_reader,
                cp.region,
                &mut self.fsm.peer,
            );
        }

        let peer_id = cp.peer.get_id();
        match change_type {
            ConfChangeType::AddNode | ConfChangeType::AddLearnerNode => {
                let peer = cp.peer.clone();
                if self.fsm.peer.peer_id() == peer_id && self.fsm.peer.peer.get_is_learner() {
                    self.fsm.peer.peer = peer.clone();
                }

                // Add this peer to cache and heartbeats.
                let now = Instant::now();
                let id = peer.get_id();
                self.fsm.peer.peer_heartbeats.insert(id, now);
                if self.fsm.peer.is_leader() {
                    self.fsm.peer.peers_start_pending_time.push((id, now));
                }
                self.fsm.peer.recent_added_peer.update(id, now);
                self.fsm.peer.insert_peer_cache(peer);
            }
            ConfChangeType::RemoveNode => {
                // Remove this peer from cache.
                self.fsm.peer.peer_heartbeats.remove(&peer_id);
                if self.fsm.peer.is_leader() {
                    self.fsm
                        .peer
                        .peers_start_pending_time
                        .retain(|&(p, _)| p != peer_id);
                }
                self.fsm.peer.remove_peer_from_cache(peer_id);
            }
        }

        // In pattern matching above, if the peer is the leader,
        // it will push the change peer into `peers_start_pending_time`
        // without checking if it is duplicated. We move `heartbeat_pd` here
        // to utilize `collect_pending_peers` in `heartbeat_pd` to avoid
        // adding the redundant peer.
        if self.fsm.peer.is_leader() {
            // Notify pd immediately.
            info!(
                "notify pd with change peer region";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
                "region" => ?self.fsm.peer.region(),
            );
            self.fsm.peer.heartbeat_pd(self.ctx);
        }
        let my_peer_id = self.fsm.peer.peer_id();

        let peer = cp.peer;

        // We only care remove itself now.
        if change_type == ConfChangeType::RemoveNode && peer.get_store_id() == self.store_id() {
            if my_peer_id == peer.get_id() {
                self.destroy_peer(false)
            } else {
                panic!(
                    "{} trying to remove unknown peer {:?}",
                    self.fsm.peer.tag, peer
                );
            }
        }
    }

    fn on_ready_compact_log(&mut self, first_index: u64, state: RaftTruncatedState) {
        let total_cnt = self.fsm.peer.last_applying_idx - first_index;
        // the size of current CompactLog command can be ignored.
        let remain_cnt = self.fsm.peer.last_applying_idx - state.get_index() - 1;
        self.fsm.peer.raft_log_size_hint =
            self.fsm.peer.raft_log_size_hint * remain_cnt / total_cnt;
        let task = RaftlogGcTask {
            raft_engine: Arc::clone(&self.fsm.peer.get_store().get_raft_engine()),
            region_id: self.fsm.peer.get_store().get_region_id(),
            start_idx: self.fsm.peer.last_compacted_idx,
            end_idx: state.get_index() + 1,
        };
        self.fsm.peer.last_compacted_idx = task.end_idx;
        self.fsm.peer.mut_store().compact_to(task.end_idx);
        if let Err(e) = self.ctx.raftlog_gc_scheduler.schedule(task) {
            error!(
                "failed to schedule compact task";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
                "err" => %e,
            );
        }
    }

    fn on_ready_split_region(&mut self, derived: metapb::Region, regions: Vec<metapb::Region>) {
        let mut guard = self.ctx.store_meta.lock().unwrap();
        let meta: &mut StoreMeta = &mut *guard;
        let region_id = derived.get_id();
        meta.set_region(
            &self.ctx.coprocessor_host,
            &self.ctx.local_reader,
            derived,
            &mut self.fsm.peer,
        );
        self.fsm.peer.post_split();
        let is_leader = self.fsm.peer.is_leader();
        if is_leader {
            self.fsm.peer.heartbeat_pd(self.ctx);
            // Notify pd immediately to let it update the region meta.
            info!(
                "notify pd with split";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
                "split_count" => regions.len(),
            );
            // Now pd only uses ReportBatchSplit for history operation show,
            // so we send it independently here.
            let task = PdTask::ReportBatchSplit {
                regions: regions.to_vec(),
            };
            if let Err(e) = self.ctx.pd_scheduler.schedule(task) {
                error!(
                    "failed to notify pd";
                    "region_id" => self.fsm.region_id(),
                    "peer_id" => self.fsm.peer_id(),
                    "err" => %e,
                );
            }
        }

        let last_key = enc_end_key(regions.last().unwrap());
        if meta.region_ranges.remove(&last_key).is_none() {
            panic!("{} original region should exists", self.fsm.peer.tag);
        }
        // It's not correct anymore, so set it to None to let split checker update it.
        self.fsm.peer.approximate_size.take();
        let last_region_id = regions.last().unwrap().get_id();
        for new_region in regions {
            let new_region_id = new_region.get_id();

            let not_exist = meta
                .region_ranges
                .insert(enc_end_key(&new_region), new_region_id)
                .is_none();
            assert!(not_exist, "[region {}] should not exists", new_region_id);

            if new_region_id == region_id {
                continue;
            }

            // Insert new regions and validation
            info!(
                "insert new region";
                "region_id" => new_region_id,
                "region" => ?new_region,
            );
            if let Some(r) = meta.regions.get(&new_region_id) {
                // Suppose a new node is added by conf change and the snapshot comes slowly.
                // Then, the region splits and the first vote message comes to the new node
                // before the old snapshot, which will create an uninitialized peer on the
                // store. After that, the old snapshot comes, followed with the last split
                // proposal. After it's applied, the uninitialized peer will be met.
                // We can remove this uninitialized peer directly.
                if !r.get_peers().is_empty() {
                    panic!(
                        "[region {}] duplicated region {:?} for split region {:?}",
                        new_region_id, r, new_region
                    );
                }
                self.ctx.router.close(new_region_id);
            }

            let (sender, mut new_peer) = match PeerFsm::create(
                self.ctx.store_id(),
                &self.ctx.cfg,
                self.ctx.region_scheduler.clone(),
                self.ctx.engines.clone(),
                &new_region,
            ) {
                Ok((sender, new_peer)) => (sender, new_peer),
                Err(e) => {
                    // peer information is already written into db, can't recover.
                    // there is probably a bug.
                    panic!("create new split region {:?} err {:?}", new_region, e);
                }
            };
            let meta_peer = new_peer.peer.peer.clone();

            for p in new_region.get_peers() {
                // Add this peer to cache.
                new_peer.peer.insert_peer_cache(p.clone());
            }

            // New peer derive write flow from parent region,
            // this will be used by balance write flow.
            new_peer.peer.peer_stat = self.fsm.peer.peer_stat.clone();
            let campaigned = new_peer.peer.maybe_campaign(is_leader);
            new_peer.has_ready |= campaigned;

            if is_leader {
                // The new peer is likely to become leader, send a heartbeat immediately to reduce
                // client query miss.
                new_peer.peer.heartbeat_pd(self.ctx);
            }

            new_peer.peer.activate(self.ctx);
            meta.regions.insert(new_region_id, new_region);
            if last_region_id == new_region_id {
                // To prevent from big region, the right region needs run split
                // check again after split.
                new_peer.peer.size_diff_hint = self.ctx.cfg.region_split_check_diff.0;
            }
            let mailbox = BasicMailbox::new(sender, new_peer);
            self.ctx.router.register(new_region_id, mailbox);
            self.ctx
                .router
                .force_send(new_region_id, PeerMsg::Start)
                .unwrap();

            if !campaigned {
                if let Some(msg) = meta
                    .pending_votes
                    .swap_remove_front(|m| m.get_to_peer() == &meta_peer)
                {
                    let _ = self
                        .ctx
                        .router
                        .send(new_region_id, PeerMsg::RaftMessage(msg));
                }
            }
        }
    }

    fn register_merge_check_tick(&self) {
        self.schedule_tick(
            PeerTick::CheckMerge,
            self.ctx.cfg.merge_check_tick_interval.0,
        )
    }

    fn validate_merge_peer(&self, target_region: &metapb::Region) -> Result<bool> {
        let region_id = target_region.get_id();
        let exist_region = {
            let meta = self.ctx.store_meta.lock().unwrap();
            meta.regions.get(&region_id).cloned()
        };
        if let Some(r) = exist_region {
            let exist_epoch = r.get_region_epoch();
            let expect_epoch = target_region.get_region_epoch();
            // exist_epoch > expect_epoch
            if util::is_epoch_stale(expect_epoch, exist_epoch) {
                return Err(box_err!(
                    "target region changed {:?} -> {:?}",
                    target_region,
                    r
                ));
            }
            // exist_epoch < expect_epoch
            if util::is_epoch_stale(exist_epoch, expect_epoch) {
                info!(
                    "target region still not catch up, skip.";
                    "region_id" => self.fsm.region_id(),
                    "peer_id" => self.fsm.peer_id(),
                    "target_region" => ?target_region,
                    "exist_region" => ?r,
                );
                return Ok(false);
            }
            return Ok(true);
        }

        let state_key = keys::region_state_key(region_id);
        let state: RegionLocalState = match self.ctx.engines.kv.get_msg_cf(CF_RAFT, &state_key) {
            Err(e) => {
                error!(
                    "failed to load region state, ignore";
                    "region_id" => self.fsm.region_id(),
                    "peer_id" => self.fsm.peer_id(),
                    "err" => %e,
                    "target_region_id" => region_id,
                );
                return Ok(false);
            }
            Ok(None) => {
                info!(
                    "seems to merge into a new replica of region, let's wait.";
                    "region_id" => self.fsm.region_id(),
                    "peer_id" => self.fsm.peer_id(),
                    "target_region_id" => region_id,
                );
                return Ok(false);
            }
            Ok(Some(state)) => state,
        };
        if state.get_state() != PeerState::Tombstone {
            info!(
                "wait for region split";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
                "target_region_id" => region_id,
            );
            return Ok(false);
        }

        let tombstone_region = state.get_region();
        if tombstone_region.get_region_epoch().get_conf_ver()
            < target_region.get_region_epoch().get_conf_ver()
        {
            info!(
                "seems to merge into a new replica of region, let's wait.";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
                "target_region_id" => region_id,
            );
            return Ok(false);
        }

        Err(box_err!("region {} is destroyed", region_id))
    }

    fn schedule_merge(&mut self) -> Result<()> {
        fail_point!("on_schedule_merge", |_| Ok(()));
        let (request, target_id) = {
            let state = self.fsm.peer.pending_merge_state.as_ref().unwrap();
            let expect_region = state.get_target();
            if !self.validate_merge_peer(expect_region)? {
                // Wait till next round.
                return Ok(());
            }
            let target_id = expect_region.get_id();
            let sibling_region = expect_region;

            let min_index = self.fsm.peer.get_min_progress() + 1;
            let low = cmp::max(min_index, state.get_min_index());
            // TODO: move this into raft module.
            // > over >= to include the PrepareMerge proposal.
            let entries = if low > state.get_commit() {
                vec![]
            } else {
                self.fsm
                    .peer
                    .get_store()
                    .entries(low, state.get_commit() + 1, NO_LIMIT)
                    .unwrap()
            };

            let sibling_peer = util::find_peer(&sibling_region, self.store_id()).unwrap();
            let mut request = new_admin_request(sibling_region.get_id(), sibling_peer.clone());
            request
                .mut_header()
                .set_region_epoch(sibling_region.get_region_epoch().clone());
            let mut admin = AdminRequest::new();
            admin.set_cmd_type(AdminCmdType::CommitMerge);
            admin
                .mut_commit_merge()
                .set_source(self.fsm.peer.region().clone());
            admin.mut_commit_merge().set_commit(state.get_commit());
            admin
                .mut_commit_merge()
                .set_entries(RepeatedField::from_vec(entries));
            request.set_admin_request(admin);
            (request, target_id)
        };
        // Please note that, here assumes that the unit of network isolation is store rather than
        // peer. So a quorum stores of source region should also be the quorum stores of target
        // region. Otherwise we need to enable proposal forwarding.
        self.ctx
            .router
            .force_send(
                target_id,
                PeerMsg::RaftCommand(RaftCommand::new(request, Callback::None)),
            )
            .map_err(|_| Error::RegionNotFound(target_id))
    }

    fn rollback_merge(&mut self) {
        let req = {
            let state = self.fsm.peer.pending_merge_state.as_ref().unwrap();
            let mut request =
                new_admin_request(self.fsm.peer.region().get_id(), self.fsm.peer.peer.clone());
            request
                .mut_header()
                .set_region_epoch(self.fsm.peer.region().get_region_epoch().clone());
            let mut admin = AdminRequest::new();
            admin.set_cmd_type(AdminCmdType::RollbackMerge);
            admin.mut_rollback_merge().set_commit(state.get_commit());
            request.set_admin_request(admin);
            request
        };
        self.propose_raft_command(req, Callback::None);
    }

    fn on_check_merge(&mut self) {
        if self.fsm.stopped || self.fsm.peer.pending_merge_state.is_none() {
            return;
        }
        self.register_merge_check_tick();
        if let Err(e) = self.schedule_merge() {
            info!(
                "failed to schedule merge, rollback";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
                "err" => %e,
            );
            self.rollback_merge();
        }
    }

    fn on_ready_prepare_merge(&mut self, region: metapb::Region, state: MergeState, merged: bool) {
        {
            let mut meta = self.ctx.store_meta.lock().unwrap();
            meta.set_region(
                &self.ctx.coprocessor_host,
                &self.ctx.local_reader,
                region.clone(),
                &mut self.fsm.peer,
            );
        }
        self.fsm.peer.pending_merge_state = Some(state);
        self.notify_prepare_merge();

        if merged {
            // CommitMerge will try to catch up log for source region. If PrepareMerge is executed
            // in the progress of catching up, there is no need to schedule merge again.
            return;
        }

        self.on_check_merge();
    }

    // The `PrepareMerge` and `CommitMerge` is executed sequentially, but we cannot
    // ensure the order to handle the apply results between different peers. So check
    // the merge locks to ensure `on_ready_prepare_merge` is called.
    fn check_locks(
        &self,
        source: &metapb::Region,
        meta: &mut StoreMeta,
    ) -> Option<Arc<AtomicBool>> {
        let source_region_id = source.get_id();
        let source_version = source.get_region_epoch().get_version();

        if let Some((exist_version, ready_to_merge)) = meta.merge_locks.remove(&source_region_id) {
            if exist_version == source_version {
                assert!(ready_to_merge.is_none());
                // So `on_ready_prepare_merge` is executed.
                return None;
            } else if exist_version < source_version {
                assert!(
                    ready_to_merge.is_none(),
                    "{} source region {} meets a commit merge before {} < {}",
                    self.fsm.peer.tag,
                    source_region_id,
                    exist_version,
                    source_version
                );
            } else {
                panic!(
                    "{} source region {} can't finished current merge: {} > {}",
                    self.fsm.peer.tag, source_region_id, exist_version, source_region_id
                );
            }
        }

        // The corresponding `on_ready_prepare_merge` is not executed yet.
        // Insert the lock, and `on_ready_prepare_merge` will check and use `ready_to_merge`
        // to notify.
        let ready_to_merge = Arc::new(AtomicBool::new(false));
        meta.merge_locks.insert(
            source_region_id,
            (source_version, Some(ready_to_merge.clone())),
        );
        Some(ready_to_merge)
    }

    fn on_ready_commit_merge(
        &mut self,
        region: metapb::Region,
        source: metapb::Region,
    ) -> Option<Arc<AtomicBool>> {
        let mut meta = self.ctx.store_meta.lock().unwrap();

        let ready_to_merge = self.check_locks(&source, &mut meta);
        if ready_to_merge.is_some() {
            return ready_to_merge;
        }

        let prev = meta.region_ranges.remove(&enc_end_key(&source));
        assert_eq!(prev, Some(source.get_id()));
        let prev = if region.get_end_key() == source.get_end_key() {
            meta.region_ranges.remove(&enc_start_key(&source))
        } else {
            meta.region_ranges.remove(&enc_end_key(&region))
        };
        if prev != Some(region.get_id()) {
            panic!(
                "{} meta corrupted: prev: {:?}, ranges: {:?}",
                self.fsm.peer.tag, prev, meta.region_ranges
            );
        }
        meta.region_ranges
            .insert(enc_end_key(&region), region.get_id());
        assert!(meta.regions.remove(&source.get_id()).is_some());
        meta.set_region(
            &self.ctx.coprocessor_host,
            &self.ctx.local_reader,
            region,
            &mut self.fsm.peer,
        );
        // make approximate size and keys updated in time.
        // the reason why follower need to update is that there is a issue that after merge
        // and then transfer leader, the new leader may have stale size and keys.
        self.fsm.peer.size_diff_hint = self.ctx.cfg.region_split_check_diff.0;
        if self.fsm.peer.is_leader() {
            info!(
                "notify pd with merge";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
                "source_region" => ?source,
                "target_region" => ?self.fsm.peer.region(),
            );
            self.fsm.peer.heartbeat_pd(self.ctx);
        }
        self.ctx
            .router
            .send(
                source.get_id(),
                PeerMsg::CasualMessage(CasualMessage::MergeResult {
                    target: self.fsm.peer.peer.clone(),
                    stale: false,
                }),
            )
            .unwrap();
        None
    }

    /// Handle rollbacking Merge result.
    ///
    /// If commit is 0, it means that Merge is rollbacked by a snapshot; otherwise
    /// it's rollbacked by a proposal, and its value should be equal to the commit
    /// index of previous PrepareMerge.
    fn on_ready_rollback_merge(&mut self, commit: u64, region: Option<metapb::Region>) {
        let pending_commit = self
            .fsm
            .peer
            .pending_merge_state
            .as_ref()
            .unwrap()
            .get_commit();
        if commit != 0 && pending_commit != commit {
            panic!(
                "{} rollbacks a wrong merge: {} != {}",
                self.fsm.peer.tag, pending_commit, commit
            );
        }
        self.fsm.peer.pending_merge_state = None;
        {
            let mut meta = self.ctx.store_meta.lock().unwrap();
            if let Some(r) = region {
                meta.set_region(
                    &self.ctx.coprocessor_host,
                    &self.ctx.local_reader,
                    r,
                    &mut self.fsm.peer,
                );
            }
            let region = self.fsm.peer.region();
            let region_id = region.get_id();
            let source_version = region.get_region_epoch().get_version();
            if let Some((exist_version, ready_to_merge)) = meta.merge_locks.remove(&region_id) {
                if exist_version > source_version {
                    assert!(
                        ready_to_merge.is_some(),
                        "{} unexpected empty merge state at {}",
                        self.fsm.peer.tag,
                        exist_version
                    );
                    meta.merge_locks
                        .insert(region_id, (exist_version, ready_to_merge));
                } else {
                    assert!(
                        ready_to_merge.is_none(),
                        "{} rollback a commit merge state at {}",
                        self.fsm.peer.tag,
                        exist_version
                    );
                }
            }
        }
        if self.fsm.peer.is_leader() {
            info!(
                "notify pd with rollback merge";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
                "commit_index" => commit,
            );
            self.fsm.peer.heartbeat_pd(self.ctx);
        }
    }

    fn on_merge_result(&mut self, target: metapb::Peer, stale: bool) {
        let exists = self
            .fsm
            .peer
            .pending_merge_state
            .as_ref()
            .map_or(true, |s| s.get_target().get_peers().contains(&target));
        if !exists {
            panic!(
                "{} unexpected merge result: {:?} {:?} {}",
                self.fsm.peer.tag, self.fsm.peer.pending_merge_state, target, stale
            );
        }
        if !stale {
            info!(
                "merge finished.";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
                "target_region" => ?self.fsm.peer.pending_merge_state.as_ref().unwrap().target,
            );
            self.destroy_peer(true);
        } else {
            self.on_stale_merge();
        }
    }

    fn on_stale_merge(&mut self) {
        info!(
            "successful merge can't be continued, try to gc stale peer.";
            "region_id" => self.fsm.region_id(),
            "peer_id" => self.fsm.peer_id(),
            "merge_state" => ?self.fsm.peer.pending_merge_state,
        );
        if let Some(job) = self.fsm.peer.maybe_destroy() {
            self.handle_destroy_peer(job);
        }
    }

    fn on_ready_apply_snapshot(&mut self, apply_result: ApplySnapResult) {
        let prev_region = apply_result.prev_region;
        let region = apply_result.region;

        info!(
            "snapshot is applied";
            "region_id" => self.fsm.region_id(),
            "peer_id" => self.fsm.peer_id(),
            "region" => ?region,
        );

        let mut meta = self.ctx.store_meta.lock().unwrap();
        debug!(
            "check snapshot range";
            "region_id" => self.region_id(),
            "peer_id" => self.fsm.peer_id(),
            "ranges" => ?meta.region_ranges,
            "prev_region" => ?prev_region,
        );
        let initialized = !prev_region.get_peers().is_empty();
        if initialized {
            info!(
                "region changed after applying snapshot";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
                "prev_region" => ?prev_region,
                "region" => ?region,
            );
            let prev = meta.region_ranges.remove(&enc_end_key(&prev_region));
            if prev != Some(region.get_id()) {
                panic!(
                    "{} meta corrupted, expect {:?} got {:?}",
                    self.fsm.peer.tag, prev_region, prev
                );
            }
        }
        if let Some(r) = meta
            .region_ranges
            .insert(enc_end_key(&region), region.get_id())
        {
            panic!("{} unexpected region {:?}", self.fsm.peer.tag, r);
        }
        let prev = meta.regions.insert(region.get_id(), region);
        assert_eq!(prev, Some(prev_region));
    }

    fn on_ready_result(
        &mut self,
        merged: bool,
        exec_results: &mut VecDeque<ExecResult>,
        metrics: &ApplyMetrics,
    ) -> Option<Arc<AtomicBool>> {
        if exec_results.is_empty() {
            return None;
        }

        self.ctx.store_stat.lock_cf_bytes_written += metrics.lock_cf_written_bytes;
        self.ctx.store_stat.engine_total_bytes_written += metrics.written_bytes;
        self.ctx.store_stat.engine_total_keys_written += metrics.written_keys;

        // handle executing committed log results
        while let Some(result) = exec_results.pop_front() {
            match result {
                ExecResult::ChangePeer(cp) => self.on_ready_change_peer(cp),
                ExecResult::CompactLog { first_index, state } => {
                    if !merged {
                        self.on_ready_compact_log(first_index, state)
                    }
                }
                ExecResult::SplitRegion { derived, regions } => {
                    self.on_ready_split_region(derived, regions)
                }
                ExecResult::PrepareMerge { region, state } => {
                    self.on_ready_prepare_merge(region, state, merged);
                }
                ExecResult::CommitMerge { region, source } => {
                    if let Some(ready_to_merge) =
                        self.on_ready_commit_merge(region.clone(), source.clone())
                    {
                        exec_results.push_front(ExecResult::CommitMerge { region, source });
                        return Some(ready_to_merge);
                    }
                }
                ExecResult::RollbackMerge { region, commit } => {
                    self.on_ready_rollback_merge(commit, Some(region))
                }
                ExecResult::ComputeHash {
                    region,
                    index,
                    snap,
                } => self.on_ready_compute_hash(region, index, snap),
                ExecResult::VerifyHash { index, hash } => self.on_ready_verify_hash(index, hash),
                ExecResult::DeleteRange { .. } => {
                    // TODO: clean user properties?
                }
                ExecResult::IngestSST { ssts } => self.on_ingest_sst_result(ssts),
            }
        }
        None
    }

    /// Check if a request is valid if it has valid prepare_merge/commit_merge proposal.
    fn check_merge_proposal(&self, msg: &mut RaftCmdRequest) -> Result<()> {
        if !msg.get_admin_request().has_prepare_merge()
            && !msg.get_admin_request().has_commit_merge()
        {
            return Ok(());
        }

        let region = self.fsm.peer.region();
        if msg.get_admin_request().has_prepare_merge() {
            let target_region = msg.get_admin_request().get_prepare_merge().get_target();
            {
                let meta = self.ctx.store_meta.lock().unwrap();
                match meta.regions.get(&target_region.get_id()) {
                    Some(r) => {
                        if r != target_region {
                            return Err(box_err!(
                                "target region not matched, skip proposing: {:?} != {:?}",
                                r,
                                target_region
                            ));
                        }
                    }
                    None => {
                        return Err(box_err!(
                            "target region {} doesn't exist.",
                            target_region.get_id()
                        ));
                    }
                }
            }
            if !util::is_sibling_regions(target_region, region) {
                return Err(box_err!(
                    "{:?} and {:?} are not sibling, skip proposing.",
                    target_region,
                    region
                ));
            }
            if !util::region_on_same_stores(target_region, region) {
                return Err(box_err!(
                    "peers doesn't match {:?} != {:?}, reject merge",
                    region.get_peers(),
                    target_region.get_peers()
                ));
            }
        } else {
            let source_region = msg.get_admin_request().get_commit_merge().get_source();
            if !util::is_sibling_regions(source_region, region) {
                return Err(box_err!(
                    "{:?} and {:?} should be sibling",
                    source_region,
                    region
                ));
            }
            if !util::region_on_same_stores(source_region, region) {
                return Err(box_err!(
                    "peers not matched: {:?} {:?}",
                    source_region,
                    region
                ));
            }
        }

        Ok(())
    }

    fn pre_propose_raft_command(
        &mut self,
        msg: &RaftCmdRequest,
    ) -> Result<Option<RaftCmdResponse>> {
        // Check store_id, make sure that the msg is dispatched to the right place.
        if let Err(e) = util::check_store_id(msg, self.store_id()) {
            self.ctx.raft_metrics.invalid_proposal.mismatch_store_id += 1;
            return Err(e);
        }
        if msg.has_status_request() {
            // For status commands, we handle it here directly.
            let resp = self.execute_status_command(msg)?;
            return Ok(Some(resp));
        }

        // Check whether the store has the right peer to handle the request.
        let region_id = self.region_id();
        let leader_id = self.fsm.peer.leader_id();
        if !self.fsm.peer.is_leader() {
            self.ctx.raft_metrics.invalid_proposal.not_leader += 1;
            let leader = self.fsm.peer.get_peer_from_cache(leader_id);
            return Err(Error::NotLeader(region_id, leader));
        }
        // peer_id must be the same as peer's.
        if let Err(e) = util::check_peer_id(msg, self.fsm.peer.peer_id()) {
            self.ctx.raft_metrics.invalid_proposal.mismatch_peer_id += 1;
            return Err(e);
        }
        // Check whether the term is stale.
        if let Err(e) = util::check_term(msg, self.fsm.peer.term()) {
            self.ctx.raft_metrics.invalid_proposal.stale_command += 1;
            return Err(e);
        }

        match util::check_region_epoch(msg, self.fsm.peer.region(), true) {
            Err(Error::EpochNotMatch(msg, mut new_regions)) => {
                // Attach the region which might be split from the current region. But it doesn't
                // matter if the region is not split from the current region. If the region meta
                // received by the TiKV driver is newer than the meta cached in the driver, the meta is
                // updated.
                let sibling_region = self.find_sibling_region();
                if let Some(sibling_region) = sibling_region {
                    new_regions.push(sibling_region);
                }
                self.ctx.raft_metrics.invalid_proposal.epoch_not_match += 1;
                Err(Error::EpochNotMatch(msg, new_regions))
            }
            Err(e) => Err(e),
            Ok(()) => Ok(None),
        }
    }

    fn propose_raft_command(&mut self, mut msg: RaftCmdRequest, cb: Callback) {
        match self.pre_propose_raft_command(&msg) {
            Ok(Some(resp)) => {
                cb.invoke_with_response(resp);
                return;
            }
            Err(e) => {
                debug!(
                    "failed to propose";
                    "region_id" => self.region_id(),
                    "peer_id" => self.fsm.peer_id(),
                    "message" => ?msg,
                    "err" => %e,
                );
                cb.invoke_with_response(new_error(e));
                return;
            }
            _ => (),
        }

        if self.fsm.peer.pending_remove {
            apply::notify_req_region_removed(self.region_id(), cb);
            return;
        }

        if let Err(e) = self.check_merge_proposal(&mut msg) {
            warn!(
                "failed to propose merge";
                "region_id" => self.region_id(),
                "peer_id" => self.fsm.peer_id(),
                "message" => ?msg,
                "err" => %e,
            );
            cb.invoke_with_response(new_error(e));
            return;
        }

        // Note:
        // The peer that is being checked is a leader. It might step down to be a follower later. It
        // doesn't matter whether the peer is a leader or not. If it's not a leader, the proposing
        // command log entry can't be committed.

        let mut resp = RaftCmdResponse::new();
        let term = self.fsm.peer.term();
        bind_term(&mut resp, term);
        if self.fsm.peer.propose(self.ctx, cb, msg, resp) {
            self.fsm.has_ready = true;
        }

        // TODO: add timeout, if the command is not applied after timeout,
        // we will call the callback with timeout error.
    }

    fn find_sibling_region(&self) -> Option<Region> {
        let start = if self.ctx.cfg.right_derive_when_split {
            Included(enc_start_key(self.fsm.peer.region()))
        } else {
            Excluded(enc_end_key(self.fsm.peer.region()))
        };
        let meta = self.ctx.store_meta.lock().unwrap();
        meta.region_ranges
            .range((start, Unbounded::<Vec<u8>>))
            .next()
            .map(|(_, region_id)| meta.regions[region_id].to_owned())
    }

    fn register_raft_gc_log_tick(&self) {
        self.schedule_tick(
            PeerTick::RaftLogGc,
            self.ctx.cfg.raft_log_gc_tick_interval.0,
        )
    }

    #[allow(clippy::if_same_then_else)]
    fn on_raft_gc_log_tick(&mut self) {
        self.register_raft_gc_log_tick();

        // As leader, we would not keep caches for the peers that didn't response heartbeat in the
        // last few seconds. That happens probably because another TiKV is down. In this case if we
        // do not clean up the cache, it may keep growing.
        let drop_cache_duration =
            self.ctx.cfg.raft_heartbeat_interval() + self.ctx.cfg.raft_entry_cache_life_time.0;
        let cache_alive_limit = Instant::now() - drop_cache_duration;

        let mut total_gc_logs = 0;

        let applied_idx = self.fsm.peer.get_store().applied_index();
        if !self.fsm.peer.is_leader() {
            self.fsm.peer.mut_store().compact_to(applied_idx + 1);
            return;
        }

        // Leader will replicate the compact log command to followers,
        // If we use current replicated_index (like 10) as the compact index,
        // when we replicate this log, the newest replicated_index will be 11,
        // but we only compact the log to 10, not 11, at that time,
        // the first index is 10, and replicated_index is 11, with an extra log,
        // and we will do compact again with compact index 11, in cycles...
        // So we introduce a threshold, if replicated index - first index > threshold,
        // we will try to compact log.
        // raft log entries[..............................................]
        //                  ^                                       ^
        //                  |-----------------threshold------------ |
        //              first_index                         replicated_index
        // `alive_cache_idx` is the smallest `replicated_index` of healthy up nodes.
        // `alive_cache_idx` is only used to gc cache.
        let truncated_idx = self.fsm.peer.get_store().truncated_index();
        let last_idx = self.fsm.peer.get_store().last_index();
        let (mut replicated_idx, mut alive_cache_idx) = (last_idx, last_idx);
        for (peer_id, p) in self.fsm.peer.raft_group.raft.prs().iter() {
            if replicated_idx > p.matched {
                replicated_idx = p.matched;
            }
            if let Some(last_heartbeat) = self.fsm.peer.peer_heartbeats.get(peer_id) {
                if alive_cache_idx > p.matched
                    && p.matched >= truncated_idx
                    && *last_heartbeat > cache_alive_limit
                {
                    alive_cache_idx = p.matched;
                }
            }
        }
        // When an election happened or a new peer is added, replicated_idx can be 0.
        if replicated_idx > 0 {
            assert!(
                last_idx >= replicated_idx,
                "expect last index {} >= replicated index {}",
                last_idx,
                replicated_idx
            );
            REGION_MAX_LOG_LAG.observe((last_idx - replicated_idx) as f64);
        }
        self.fsm
            .peer
            .mut_store()
            .maybe_gc_cache(alive_cache_idx, applied_idx);
        let first_idx = self.fsm.peer.get_store().first_index();
        let mut compact_idx;
        if applied_idx > first_idx
            && applied_idx - first_idx >= self.ctx.cfg.raft_log_gc_count_limit
        {
            compact_idx = applied_idx;
        } else if self.fsm.peer.raft_log_size_hint >= self.ctx.cfg.raft_log_gc_size_limit.0 {
            compact_idx = applied_idx;
        } else if replicated_idx < first_idx
            || replicated_idx - first_idx <= self.ctx.cfg.raft_log_gc_threshold
        {
            return;
        } else {
            compact_idx = replicated_idx;
        }

        // Have no idea why subtract 1 here, but original code did this by magic.
        assert!(compact_idx > 0);
        compact_idx -= 1;
        if compact_idx < first_idx {
            // In case compact_idx == first_idx before subtraction.
            return;
        }

        total_gc_logs += compact_idx - first_idx;

        let res = self.fsm.peer.raft_group.raft.raft_log.term(compact_idx);
        let term = match res {
            Ok(t) => t,
            Err(e) => panic!(
                "{} fail to load term for {}: {:?}",
                self.fsm.peer.tag, compact_idx, e
            ),
        };

        // Create a compact log request and notify directly.
        let region_id = self.fsm.peer.region().get_id();
        let request =
            new_compact_log_request(region_id, self.fsm.peer.peer.clone(), compact_idx, term);
        self.propose_raft_command(request, Callback::None);

        PEER_GC_RAFT_LOG_COUNTER.inc_by(total_gc_logs as i64);
    }

    fn register_split_region_check_tick(&self) {
        self.schedule_tick(
            PeerTick::SplitRegionCheck,
            self.ctx.cfg.split_region_check_tick_interval.0,
        )
    }

    fn on_split_region_check_tick(&mut self) {
        self.register_split_region_check_tick();
        // To avoid frequent scan, we only add new scan tasks if all previous tasks
        // have finished.
        // TODO: check whether a gc progress has been started.
        if self.ctx.split_check_scheduler.is_busy() {
            return;
        }

        if !self.fsm.peer.is_leader() {
            return;
        }

        // When restart, the approximate size will be None. The
        // split check will first check the region size, and then
        // check whether the region should split.  This should
        // work even if we change the region max size.
        // If peer says should update approximate size, update region
        // size and check whether the region should split.
        if self.fsm.peer.approximate_size.is_some()
            && self.fsm.peer.compaction_declined_bytes < self.ctx.cfg.region_split_check_diff.0
            && self.fsm.peer.size_diff_hint < self.ctx.cfg.region_split_check_diff.0
        {
            return;
        }
        let task = SplitCheckTask::new(self.fsm.peer.region().clone(), true, CheckPolicy::SCAN);
        if let Err(e) = self.ctx.split_check_scheduler.schedule(task) {
            error!(
                "failed to schedule split check";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
                "err" => %e,
            );
        }
        self.fsm.peer.size_diff_hint = 0;
        self.fsm.peer.compaction_declined_bytes = 0;
    }

    fn on_prepare_split_region(
        &mut self,
        region_epoch: metapb::RegionEpoch,
        split_keys: Vec<Vec<u8>>,
        cb: Callback,
    ) {
        if let Err(e) = self.validate_split_region(&region_epoch, &split_keys) {
            cb.invoke_with_response(new_error(e));
            return;
        }
        let region = self.fsm.peer.region();
        let task = PdTask::AskBatchSplit {
            region: region.clone(),
            split_keys,
            peer: self.fsm.peer.peer.clone(),
            right_derive: self.ctx.cfg.right_derive_when_split,
            callback: cb,
        };
        if let Err(Stopped(t)) = self.ctx.pd_scheduler.schedule(task) {
            error!(
                "failed to notify pd to split: Stopped";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
            );
            match t {
                PdTask::AskBatchSplit { callback, .. } => {
                    callback.invoke_with_response(new_error(box_err!("failed to split: Stopped")));
                }
                _ => unreachable!(),
            }
        }
    }

    fn validate_split_region(
        &mut self,
        epoch: &metapb::RegionEpoch,
        split_keys: &[Vec<u8>],
    ) -> Result<()> {
        if split_keys.is_empty() {
            error!(
                "no split key is specified.";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
            );
            return Err(box_err!("{} no split key is specified.", self.fsm.peer.tag));
        }
        for key in split_keys {
            if key.is_empty() {
                error!(
                    "split key should not be empty!!!";
                    "region_id" => self.fsm.region_id(),
                    "peer_id" => self.fsm.peer_id(),
                );
                return Err(box_err!(
                    "{} split key should not be empty",
                    self.fsm.peer.tag
                ));
            }
        }
        if !self.fsm.peer.is_leader() {
            // region on this store is no longer leader, skipped.
            info!(
                "not leader, skip.";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
            );
            return Err(Error::NotLeader(
                self.region_id(),
                self.fsm.peer.get_peer_from_cache(self.fsm.peer.leader_id()),
            ));
        }

        let region = self.fsm.peer.region();
        let latest_epoch = region.get_region_epoch();

        // This is a little difference for `check_region_epoch` in region split case.
        // Here we just need to check `version` because `conf_ver` will be update
        // to the latest value of the peer, and then send to PD.
        if latest_epoch.get_version() != epoch.get_version() {
            info!(
                "epoch changed, retry later";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
                "prev_epoch" => ?region.get_region_epoch(),
                "epoch" => ?epoch,
            );
            return Err(Error::EpochNotMatch(
                format!(
                    "{} epoch changed {:?} != {:?}, retry later",
                    self.fsm.peer.tag, latest_epoch, epoch
                ),
                vec![region.to_owned()],
            ));
        }
        Ok(())
    }

    fn on_approximate_region_size(&mut self, size: u64) {
        self.fsm.peer.approximate_size = Some(size);
    }

    fn on_approximate_region_keys(&mut self, keys: u64) {
        self.fsm.peer.approximate_keys = Some(keys);
    }

    fn on_compaction_declined_bytes(&mut self, declined_bytes: u64) {
        self.fsm.peer.compaction_declined_bytes += declined_bytes;
        if self.fsm.peer.compaction_declined_bytes >= self.ctx.cfg.region_split_check_diff.0 {
            UPDATE_REGION_SIZE_BY_COMPACTION_COUNTER.inc();
        }
    }

    fn on_schedule_half_split_region(
        &mut self,
        region_epoch: &metapb::RegionEpoch,
        policy: CheckPolicy,
    ) {
        if !self.fsm.peer.is_leader() {
            // region on this store is no longer leader, skipped.
            warn!(
                "not leader, skip";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
            );
            return;
        }

        let region = self.fsm.peer.region();
        if util::is_epoch_stale(region_epoch, region.get_region_epoch()) {
            warn!(
                "receive a stale halfsplit message";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
            );
            return;
        }

        let task = SplitCheckTask::new(region.clone(), false, policy);
        if let Err(e) = self.ctx.split_check_scheduler.schedule(task) {
            error!(
                "failed to schedule split check";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
                "err" => %e,
            );
        }
    }

    fn on_pd_heartbeat_tick(&mut self) {
        self.register_pd_heartbeat_tick();
        self.fsm.peer.check_peers();

        if !self.fsm.peer.is_leader() {
            return;
        }
        self.fsm.peer.heartbeat_pd(self.ctx);
    }

    fn register_pd_heartbeat_tick(&self) {
        self.schedule_tick(
            PeerTick::PdHeartbeat,
            self.ctx.cfg.pd_heartbeat_tick_interval.0,
        )
    }

    fn on_check_peer_stale_state_tick(&mut self) {
        if self.fsm.peer.pending_remove {
            return;
        }

        self.register_check_peer_stale_state_tick();

        if self.fsm.peer.is_applying_snapshot() || self.fsm.peer.has_pending_snapshot() {
            return;
        }

        // If this peer detects the leader is missing for a long long time,
        // it should consider itself as a stale peer which is removed from
        // the original cluster.
        // This most likely happens in the following scenario:
        // At first, there are three peer A, B, C in the cluster, and A is leader.
        // Peer B gets down. And then A adds D, E, F into the cluster.
        // Peer D becomes leader of the new cluster, and then removes peer A, B, C.
        // After all these peer in and out, now the cluster has peer D, E, F.
        // If peer B goes up at this moment, it still thinks it is one of the cluster
        // and has peers A, C. However, it could not reach A, C since they are removed
        // from the cluster or probably destroyed.
        // Meantime, D, E, F would not reach B, since it's not in the cluster anymore.
        // In this case, peer B would notice that the leader is missing for a long time,
        // and it would check with pd to confirm whether it's still a member of the cluster.
        // If not, it destroys itself as a stale peer which is removed out already.
        let state = self.fsm.peer.check_stale_state(self.ctx);
        fail_point!("peer_check_stale_state", state != StaleState::Valid, |_| {});
        match state {
            StaleState::Valid => (),
            StaleState::LeaderMissing => {
                warn!(
                    "leader missing longer than abnormal_leader_missing_duration";
                    "region_id" => self.fsm.region_id(),
                    "peer_id" => self.fsm.peer_id(),
                    "expect" => %self.ctx.cfg.abnormal_leader_missing_duration,
                );
                self.ctx
                    .raft_metrics
                    .leader_missing
                    .lock()
                    .unwrap()
                    .insert(self.region_id());
            }
            StaleState::ToValidate => {
                // for peer B in case 1 above
                warn!(
                    "leader missing longer than max_leader_missing_duration. \
                     To check with pd whether it's still valid";
                    "region_id" => self.fsm.region_id(),
                    "peer_id" => self.fsm.peer_id(),
                    "expect" => %self.ctx.cfg.max_leader_missing_duration,
                );
                let task = PdTask::ValidatePeer {
                    peer: self.fsm.peer.peer.clone(),
                    region: self.fsm.peer.region().clone(),
                    merge_source: None,
                };
                if let Err(e) = self.ctx.pd_scheduler.schedule(task) {
                    error!(
                        "failed to notify pd";
                        "region_id" => self.fsm.region_id(),
                        "peer_id" => self.fsm.peer_id(),
                        "err" => %e,
                    )
                }
            }
        }
    }

    fn register_check_peer_stale_state_tick(&self) {
        self.schedule_tick(
            PeerTick::CheckPeerStaleState,
            self.ctx.cfg.peer_stale_state_check_interval.0,
        )
    }
}

impl<'a, T: Transport, C: PdClient> PeerFsmDelegate<'a, T, C> {
    fn on_ready_compute_hash(&mut self, region: metapb::Region, index: u64, snap: EngineSnapshot) {
        self.fsm.peer.consistency_state.last_check_time = Instant::now();
        let task = ConsistencyCheckTask::compute_hash(region, index, snap);
        info!(
            "schedule compute hash task";
            "region_id" => self.fsm.region_id(),
            "peer_id" => self.fsm.peer_id(),
            "task" => %task,
        );
        if let Err(e) = self.ctx.consistency_check_scheduler.schedule(task) {
            error!(
                "schedule failed";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
                "err" => %e,
            );
        }
    }

    fn on_ready_verify_hash(&mut self, expected_index: u64, expected_hash: Vec<u8>) {
        self.verify_and_store_hash(expected_index, expected_hash);
    }

    fn on_hash_computed(&mut self, index: u64, hash: Vec<u8>) {
        if !self.verify_and_store_hash(index, hash) {
            return;
        }

        let req = new_verify_hash_request(
            self.region_id(),
            self.fsm.peer.peer.clone(),
            &self.fsm.peer.consistency_state,
        );
        self.propose_raft_command(req, Callback::None);
    }

    fn on_ingest_sst_result(&mut self, ssts: Vec<SSTMeta>) {
        for sst in &ssts {
            self.fsm.peer.size_diff_hint += sst.get_length();
        }

        let task = CleanupSSTTask::DeleteSST { ssts };
        if let Err(e) = self.ctx.cleanup_sst_scheduler.schedule(task) {
            error!(
                "schedule to delete ssts";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
                "err" => %e,
            );
        }
    }

    /// Verify and store the hash to state. return true means the hash has been stored successfully.
    fn verify_and_store_hash(&mut self, expected_index: u64, expected_hash: Vec<u8>) -> bool {
        if expected_index < self.fsm.peer.consistency_state.index {
            REGION_HASH_COUNTER_VEC
                .with_label_values(&["verify", "miss"])
                .inc();
            warn!(
                "has scheduled a new hash, skip.";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
                "index" => self.fsm.peer.consistency_state.index,
                "expected_index" => expected_index,
            );
            return false;
        }
        if self.fsm.peer.consistency_state.index == expected_index {
            if self.fsm.peer.consistency_state.hash.is_empty() {
                warn!(
                    "duplicated consistency check detected, skip.";
                    "region_id" => self.fsm.region_id(),
                    "peer_id" => self.fsm.peer_id(),
                );
                return false;
            }
            if self.fsm.peer.consistency_state.hash != expected_hash {
                panic!(
                    "{} hash at {} not correct, want \"{}\", got \"{}\"!!!",
                    self.fsm.peer.tag,
                    self.fsm.peer.consistency_state.index,
                    escape(&expected_hash),
                    escape(&self.fsm.peer.consistency_state.hash)
                );
            }
            info!(
                "consistency check pass.";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
                "index" => self.fsm.peer.consistency_state.index
            );
            REGION_HASH_COUNTER_VEC
                .with_label_values(&["verify", "matched"])
                .inc();
            self.fsm.peer.consistency_state.hash = vec![];
            return false;
        }
        if self.fsm.peer.consistency_state.index != INVALID_INDEX
            && !self.fsm.peer.consistency_state.hash.is_empty()
        {
            // Maybe computing is too slow or computed result is dropped due to channel full.
            // If computing is too slow, miss count will be increased twice.
            REGION_HASH_COUNTER_VEC
                .with_label_values(&["verify", "miss"])
                .inc();
            warn!(
                "hash belongs to wrong index, skip.";
                "region_id" => self.fsm.region_id(),
                "peer_id" => self.fsm.peer_id(),
                "index" => self.fsm.peer.consistency_state.index,
                "expected_index" => expected_index,
            );
        }

        info!(
            "save hash for consistency check later.";
            "region_id" => self.fsm.region_id(),
            "peer_id" => self.fsm.peer_id(),
            "index" => expected_index,
        );
        self.fsm.peer.consistency_state.index = expected_index;
        self.fsm.peer.consistency_state.hash = expected_hash;
        true
    }
}

/// Checks merge target, returns whether the source peer should be destroyed.
/// It returns true when there is a network isolation which leads to a follower of a merge target
/// Region's log falls behind and then receive a snapshot with epoch version after merge.
pub fn maybe_destroy_source(
    meta: &StoreMeta,
    target_region_id: u64,
    source_region_id: u64,
    region_epoch: RegionEpoch,
) -> bool {
    if let Some(merge_targets) = meta.pending_merge_targets.get(&target_region_id) {
        if let Some(target_epoch) = merge_targets.get(&source_region_id) {
            info!(
                "[region {}] checking source {} epoch: {:?}, merge target epoch: {:?}",
                target_region_id, source_region_id, region_epoch, target_epoch,
            );
            // The target peer will move on, namely, it will apply a snapshot generated after merge,
            // so destroy source peer.
            if region_epoch.get_version() > target_epoch.get_version() {
                return true;
            }
            // Wait till the target peer has caught up logs and source peer will be destroyed at that time.
            return false;
        }
    }
    false
}

pub fn new_admin_request(region_id: u64, peer: metapb::Peer) -> RaftCmdRequest {
    let mut request = RaftCmdRequest::new();
    request.mut_header().set_region_id(region_id);
    request.mut_header().set_peer(peer);
    request
}

fn new_verify_hash_request(
    region_id: u64,
    peer: metapb::Peer,
    state: &ConsistencyState,
) -> RaftCmdRequest {
    let mut request = new_admin_request(region_id, peer);

    let mut admin = AdminRequest::new();
    admin.set_cmd_type(AdminCmdType::VerifyHash);
    admin.mut_verify_hash().set_index(state.index);
    admin.mut_verify_hash().set_hash(state.hash.clone());
    request.set_admin_request(admin);
    request
}

fn new_compact_log_request(
    region_id: u64,
    peer: metapb::Peer,
    compact_index: u64,
    compact_term: u64,
) -> RaftCmdRequest {
    let mut request = new_admin_request(region_id, peer);

    let mut admin = AdminRequest::new();
    admin.set_cmd_type(AdminCmdType::CompactLog);
    admin.mut_compact_log().set_compact_index(compact_index);
    admin.mut_compact_log().set_compact_term(compact_term);
    request.set_admin_request(admin);
    request
}

impl<'a, T: Transport, C: PdClient> PeerFsmDelegate<'a, T, C> {
    // Handle status commands here, separate the logic, maybe we can move it
    // to another file later.
    // Unlike other commands (write or admin), status commands only show current
    // store status, so no need to handle it in raft group.
    fn execute_status_command(&mut self, request: &RaftCmdRequest) -> Result<RaftCmdResponse> {
        let cmd_type = request.get_status_request().get_cmd_type();

        let mut response = match cmd_type {
            StatusCmdType::RegionLeader => self.execute_region_leader(),
            StatusCmdType::RegionDetail => self.execute_region_detail(request),
            StatusCmdType::InvalidStatus => Err(box_err!("invalid status command!")),
        }?;
        response.set_cmd_type(cmd_type);

        let mut resp = RaftCmdResponse::new();
        resp.set_status_response(response);
        // Bind peer current term here.
        bind_term(&mut resp, self.fsm.peer.term());
        Ok(resp)
    }

    fn execute_region_leader(&mut self) -> Result<StatusResponse> {
        let mut resp = StatusResponse::new();
        if let Some(leader) = self.fsm.peer.get_peer_from_cache(self.fsm.peer.leader_id()) {
            resp.mut_region_leader().set_leader(leader);
        }

        Ok(resp)
    }

    fn execute_region_detail(&mut self, request: &RaftCmdRequest) -> Result<StatusResponse> {
        if !self.fsm.peer.get_store().is_initialized() {
            let region_id = request.get_header().get_region_id();
            return Err(Error::RegionNotInitialized(region_id));
        }
        let mut resp = StatusResponse::new();
        resp.mut_region_detail()
            .set_region(self.fsm.peer.region().clone());
        if let Some(leader) = self.fsm.peer.get_peer_from_cache(self.fsm.peer.leader_id()) {
            resp.mut_region_detail().set_leader(leader);
        }

        Ok(resp)
    }
}
