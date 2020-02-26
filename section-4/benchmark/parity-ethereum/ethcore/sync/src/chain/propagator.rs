// Copyright 2015-2019 Parity Technologies (UK) Ltd.
// This file is part of Parity Ethereum.

// Parity Ethereum is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Parity Ethereum is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Parity Ethereum.  If not, see <http://www.gnu.org/licenses/>.

use std::cmp;
use std::collections::HashSet;

use bytes::Bytes;
use ethereum_types::H256;
use fastmap::H256FastSet;
use network::client_version::ClientCapabilities;
use network::PeerId;
use rand::Rng;
use rlp::{Encodable, RlpStream};
use sync_io::SyncIo;
use types::transaction::SignedTransaction;
use types::BlockNumber;
use types::blockchain_info::BlockChainInfo;

use super::sync_packet::SyncPacket;
use super::sync_packet::SyncPacket::{
	NewBlockHashesPacket,
	TransactionsPacket,
	NewBlockPacket,
	ConsensusDataPacket,
};

use super::{
	random,
	ChainSync,
	MAX_TRANSACTION_PACKET_SIZE,
	MAX_PEER_LAG_PROPAGATION,
	MAX_PEERS_PROPAGATION,
	MIN_PEERS_PROPAGATION,
};

/// The Chain Sync Propagator: propagates data to peers
pub struct SyncPropagator;

impl SyncPropagator {
	/// propagates latest block to a set of peers
	pub fn propagate_blocks(sync: &mut ChainSync, chain_info: &BlockChainInfo, io: &mut SyncIo, blocks: &[H256], peers: &[PeerId]) -> usize {
		trace!(target: "sync", "Sending NewBlocks to {:?}", peers);
		let sent = peers.len();
		let mut send_packet = |io: &mut SyncIo, rlp: Bytes| {
			for peer_id in peers {
				SyncPropagator::send_packet(io, *peer_id, NewBlockPacket, rlp.clone());

				if let Some(ref mut peer) = sync.peers.get_mut(peer_id) {
					peer.latest_hash = chain_info.best_block_hash.clone();
				}
			}
		};

		if blocks.is_empty() {
			let rlp =  ChainSync::create_latest_block_rlp(io.chain());
			send_packet(io, rlp);
		} else {
			for h in blocks {
				let rlp =  ChainSync::create_new_block_rlp(io.chain(), h);
				send_packet(io, rlp);
			}
		}

		sent
	}

	/// propagates new known hashes to all peers
	pub fn propagate_new_hashes(sync: &mut ChainSync, chain_info: &BlockChainInfo, io: &mut SyncIo, peers: &[PeerId]) -> usize {
		trace!(target: "sync", "Sending NewHashes to {:?}", peers);
		let last_parent = *io.chain().best_block_header().parent_hash();
		let best_block_hash = chain_info.best_block_hash;
		let rlp = match ChainSync::create_new_hashes_rlp(io.chain(), &last_parent, &best_block_hash) {
			Some(rlp) => rlp,
			None => return 0
		};

		let sent = peers.len();
		for peer_id in peers {
			if let Some(ref mut peer) = sync.peers.get_mut(peer_id) {
				peer.latest_hash = best_block_hash;
			}
			SyncPropagator::send_packet(io, *peer_id, NewBlockHashesPacket, rlp.clone());
		}
		sent
	}

	/// propagates new transactions to all peers
	pub fn propagate_new_transactions<F: FnMut() -> bool>(sync: &mut ChainSync, io: &mut SyncIo, mut should_continue: F) -> usize {
		// Early out if nobody to send to.
		if sync.peers.is_empty() {
			return 0;
		}

		let transactions = io.chain().transactions_to_propagate();
		if transactions.is_empty() {
			return 0;
		}

		if !should_continue() {
			return 0;
		}

		let (transactions, service_transactions): (Vec<_>, Vec<_>) = transactions.iter()
			.map(|tx| tx.signed())
			.partition(|tx| !tx.gas_price.is_zero());

		// usual transactions could be propagated to all peers
		let mut affected_peers = HashSet::new();
		if !transactions.is_empty() {
			let peers = SyncPropagator::select_peers_for_transactions(sync, |_| true);
			affected_peers = SyncPropagator::propagate_transactions_to_peers(
				sync, io, peers, transactions, &mut should_continue,
			);
		}

		// most of times service_transactions will be empty
		// => there's no need to merge packets
		if !service_transactions.is_empty() {
			let service_transactions_peers = SyncPropagator::select_peers_for_transactions(sync, |peer_id| io.peer_version(*peer_id).accepts_service_transaction());
			let service_transactions_affected_peers = SyncPropagator::propagate_transactions_to_peers(
				sync, io, service_transactions_peers, service_transactions, &mut should_continue
			);
			affected_peers.extend(&service_transactions_affected_peers);
		}

		affected_peers.len()
	}

	fn propagate_transactions_to_peers<F: FnMut() -> bool>(
		sync: &mut ChainSync,
		io: &mut SyncIo,
		peers: Vec<PeerId>,
		transactions: Vec<&SignedTransaction>,
		mut should_continue: F,
	) -> HashSet<PeerId> {
		let all_transactions_hashes = transactions.iter()
			.map(|tx| tx.hash())
			.collect::<H256FastSet>();
		let all_transactions_rlp = {
			let mut packet = RlpStream::new_list(transactions.len());
			for tx in &transactions { packet.append(&**tx); }
			packet.out()
		};

		// Clear old transactions from stats
		sync.transactions_stats.retain(&all_transactions_hashes);

		let send_packet = |io: &mut SyncIo, peer_id: PeerId, sent: usize, rlp: Bytes| {
			let size = rlp.len();
			SyncPropagator::send_packet(io, peer_id, TransactionsPacket, rlp);
			trace!(target: "sync", "{:02} <- Transactions ({} entries; {} bytes)", peer_id, sent, size);
		};

		let block_number = io.chain().chain_info().best_block_number;
		let mut sent_to_peers = HashSet::new();
		let mut max_sent = 0;

		// for every peer construct and send transactions packet
		for peer_id in peers {
			if !should_continue() {
				debug!(target: "sync", "Sent up to {} transactions to {} peers.", max_sent, sent_to_peers.len());
				return sent_to_peers;
			}

			let stats = &mut sync.transactions_stats;
			let peer_info = sync.peers.get_mut(&peer_id)
				.expect("peer_id is form peers; peers is result of select_peers_for_transactions; select_peers_for_transactions selects peers from self.peers; qed");

			// Send all transactions, if the peer doesn't know about anything
			if peer_info.last_sent_transactions.is_empty() {
				// update stats
				for hash in &all_transactions_hashes {
					let id = io.peer_session_info(peer_id).and_then(|info| info.id);
					stats.propagated(hash, id, block_number);
				}
				peer_info.last_sent_transactions = all_transactions_hashes.clone();

				send_packet(io, peer_id, all_transactions_hashes.len(), all_transactions_rlp.clone());
				sent_to_peers.insert(peer_id);
				max_sent = cmp::max(max_sent, all_transactions_hashes.len());
				continue;
			}

			// Get hashes of all transactions to send to this peer
			let to_send = all_transactions_hashes.difference(&peer_info.last_sent_transactions)
				.cloned()
				.collect::<HashSet<_>>();
			if to_send.is_empty() {
				continue;
			}

			// Construct RLP
			let (packet, to_send) = {
				let mut to_send = to_send;
				let mut packet = RlpStream::new();
				packet.begin_unbounded_list();
				let mut pushed = 0;
				for tx in &transactions {
					let hash = tx.hash();
					if to_send.contains(&hash) {
						let mut transaction = RlpStream::new();
						tx.rlp_append(&mut transaction);
						let appended = packet.append_raw_checked(&transaction.drain(), 1, MAX_TRANSACTION_PACKET_SIZE);
						if !appended {
							// Maximal packet size reached just proceed with sending
							debug!(target: "sync", "Transaction packet size limit reached. Sending incomplete set of {}/{} transactions.", pushed, to_send.len());
							to_send = to_send.into_iter().take(pushed).collect();
							break;
						}
						pushed += 1;
					}
				}
				packet.complete_unbounded_list();
				(packet, to_send)
			};

			// Update stats
			let id = io.peer_session_info(peer_id).and_then(|info| info.id);
			for hash in &to_send {
				// update stats
				stats.propagated(hash, id, block_number);
			}

			peer_info.last_sent_transactions = all_transactions_hashes
				.intersection(&peer_info.last_sent_transactions)
				.chain(&to_send)
				.cloned()
				.collect();
			send_packet(io, peer_id, to_send.len(), packet.out());
			sent_to_peers.insert(peer_id);
			max_sent = cmp::max(max_sent, to_send.len());

		}

		debug!(target: "sync", "Sent up to {} transactions to {} peers.", max_sent, sent_to_peers.len());
		sent_to_peers
	}

	pub fn propagate_latest_blocks(sync: &mut ChainSync, io: &mut SyncIo, sealed: &[H256]) {
		let chain_info = io.chain().chain_info();
		if (((chain_info.best_block_number as i64) - (sync.last_sent_block_number as i64)).abs() as BlockNumber) < MAX_PEER_LAG_PROPAGATION {
			let peers = sync.get_lagging_peers(&chain_info);
			if sealed.is_empty() {
				let hashes = SyncPropagator::propagate_new_hashes(sync, &chain_info, io, &peers);
				let peers = ChainSync::select_random_peers(&peers);
				let blocks = SyncPropagator::propagate_blocks(sync, &chain_info, io, sealed, &peers);
				if blocks != 0 || hashes != 0 {
					trace!(target: "sync", "Sent latest {} blocks and {} hashes to peers.", blocks, hashes);
				}
			} else {
				SyncPropagator::propagate_blocks(sync, &chain_info, io, sealed, &peers);
				SyncPropagator::propagate_new_hashes(sync, &chain_info, io, &peers);
				trace!(target: "sync", "Sent sealed block to all peers");
			};
		}
		sync.last_sent_block_number = chain_info.best_block_number;
	}

	/// Distribute valid proposed blocks to subset of current peers.
	pub fn propagate_proposed_blocks(sync: &mut ChainSync, io: &mut SyncIo, proposed: &[Bytes]) {
		let peers = sync.get_consensus_peers();
		trace!(target: "sync", "Sending proposed blocks to {:?}", peers);
		for block in proposed {
			let rlp = ChainSync::create_block_rlp(
				block,
				io.chain().chain_info().total_difficulty
			);
			for peer_id in &peers {
				SyncPropagator::send_packet(io, *peer_id, NewBlockPacket, rlp.clone());
			}
		}
	}

	/// Broadcast consensus message to peers.
	pub fn propagate_consensus_packet(sync: &mut ChainSync, io: &mut SyncIo, packet: Bytes) {
		let lucky_peers = ChainSync::select_random_peers(&sync.get_consensus_peers());
		trace!(target: "sync", "Sending consensus packet to {:?}", lucky_peers);
		for peer_id in lucky_peers {
			SyncPropagator::send_packet(io, peer_id, ConsensusDataPacket, packet.clone());
		}
	}

	/// Broadcast private transaction message to peers.
	pub fn propagate_private_transaction(sync: &mut ChainSync, io: &mut SyncIo, transaction_hash: H256, packet_id: SyncPacket, packet: Bytes) {
		let lucky_peers = ChainSync::select_random_peers(&sync.get_private_transaction_peers(&transaction_hash));
		if lucky_peers.is_empty() {
			error!(target: "privatetx", "Cannot propagate the packet, no peers with private tx enabled connected");
		} else {
			trace!(target: "privatetx", "Sending private transaction packet to {:?}", lucky_peers);
			for peer_id in lucky_peers {
				if let Some(ref mut peer) = sync.peers.get_mut(&peer_id) {
					peer.last_sent_private_transactions.insert(transaction_hash);
				}
				SyncPropagator::send_packet(io, peer_id, packet_id, packet.clone());
			}
		}
	}

	fn select_peers_for_transactions<F>(sync: &ChainSync, filter: F) -> Vec<PeerId>
		where F: Fn(&PeerId) -> bool {
		// sqrt(x)/x scaled to max u32
		let fraction = ((sync.peers.len() as f64).powf(-0.5) * (u32::max_value() as f64).round()) as u32;
		let small = sync.peers.len() < MIN_PEERS_PROPAGATION;

		let mut random = random::new();
		sync.peers.keys()
			.cloned()
			.filter(filter)
			.filter(|_| small || random.next_u32() < fraction)
			.take(MAX_PEERS_PROPAGATION)
			.collect()
	}

	/// Generic packet sender
	pub fn send_packet(sync: &mut SyncIo, peer_id: PeerId, packet_id: SyncPacket, packet: Bytes) {
		if let Err(e) = sync.send(peer_id, packet_id, packet) {
			debug!(target:"sync", "Error sending packet: {:?}", e);
			sync.disconnect_peer(peer_id);
		}
	}
}

#[cfg(test)]
mod tests {
	use ethcore::client::{BlockInfo, ChainInfo, EachBlockWith, TestBlockChainClient};
	use parking_lot::RwLock;
	use rlp::{Rlp};
	use std::collections::{VecDeque};
	use tests::helpers::{TestIo};
	use tests::snapshot::TestSnapshotService;

	use super::{*, super::{*, tests::*}};

	#[test]
	fn sends_new_hashes_to_lagging_peer() {
		let mut client = TestBlockChainClient::new();
		client.add_blocks(100, EachBlockWith::Uncle);
		let queue = RwLock::new(VecDeque::new());
		let mut sync = dummy_sync_with_peer(client.block_hash_delta_minus(5), &client);
		let chain_info = client.chain_info();
		let ss = TestSnapshotService::new();
		let mut io = TestIo::new(&mut client, &ss, &queue, None);

		let peers = sync.get_lagging_peers(&chain_info);
		let peer_count = SyncPropagator::propagate_new_hashes(&mut sync, &chain_info, &mut io, &peers);

		// 1 message should be send
		assert_eq!(1, io.packets.len());
		// 1 peer should be updated
		assert_eq!(1, peer_count);
		// NEW_BLOCK_HASHES_PACKET
		assert_eq!(0x01, io.packets[0].packet_id);
	}

	#[test]
	fn sends_latest_block_to_lagging_peer() {
		let mut client = TestBlockChainClient::new();
		client.add_blocks(100, EachBlockWith::Uncle);
		let queue = RwLock::new(VecDeque::new());
		let mut sync = dummy_sync_with_peer(client.block_hash_delta_minus(5), &client);
		let chain_info = client.chain_info();
		let ss = TestSnapshotService::new();
		let mut io = TestIo::new(&mut client, &ss, &queue, None);
		let peers = sync.get_lagging_peers(&chain_info);
		let peer_count = SyncPropagator::propagate_blocks(&mut sync, &chain_info, &mut io, &[], &peers);

		// 1 message should be send
		assert_eq!(1, io.packets.len());
		// 1 peer should be updated
		assert_eq!(1, peer_count);
		// NEW_BLOCK_PACKET
		assert_eq!(0x07, io.packets[0].packet_id);
	}

	#[test]
	fn sends_sealed_block() {
		let mut client = TestBlockChainClient::new();
		client.add_blocks(100, EachBlockWith::Uncle);
		let queue = RwLock::new(VecDeque::new());
		let hash = client.block_hash(BlockId::Number(99)).unwrap();
		let mut sync = dummy_sync_with_peer(client.block_hash_delta_minus(5), &client);
		let chain_info = client.chain_info();
		let ss = TestSnapshotService::new();
		let mut io = TestIo::new(&mut client, &ss, &queue, None);
		let peers = sync.get_lagging_peers(&chain_info);
		let peer_count = SyncPropagator::propagate_blocks(&mut sync ,&chain_info, &mut io, &[hash.clone()], &peers);

		// 1 message should be send
		assert_eq!(1, io.packets.len());
		// 1 peer should be updated
		assert_eq!(1, peer_count);
		// NEW_BLOCK_PACKET
		assert_eq!(0x07, io.packets[0].packet_id);
	}

	#[test]
	fn sends_proposed_block() {
		let mut client = TestBlockChainClient::new();
		client.add_blocks(2, EachBlockWith::Uncle);
		let queue = RwLock::new(VecDeque::new());
		let block = client.block(BlockId::Latest).unwrap().into_inner();
		let mut sync = ChainSync::new(SyncConfig::default(), &client, None);
		sync.peers.insert(0,
			PeerInfo {
				// Messaging protocol
				protocol_version: 2,
				genesis: H256::zero(),
				network_id: 0,
				latest_hash: client.block_hash_delta_minus(1),
				difficulty: None,
				asking: PeerAsking::Nothing,
				asking_blocks: Vec::new(),
				asking_hash: None,
				ask_time: Instant::now(),
				last_sent_transactions: Default::default(),
				last_sent_private_transactions: Default::default(),
				expired: false,
				private_tx_enabled: false,
				confirmation: ForkConfirmation::Confirmed,
				snapshot_number: None,
				snapshot_hash: None,
				asking_snapshot_data: None,
				block_set: None,
				client_version: ClientVersion::from(""),
			});
		let ss = TestSnapshotService::new();
		let mut io = TestIo::new(&mut client, &ss, &queue, None);
		SyncPropagator::propagate_proposed_blocks(&mut sync, &mut io, &[block]);

		// 1 message should be sent
		assert_eq!(1, io.packets.len());
		// NEW_BLOCK_PACKET
		assert_eq!(0x07, io.packets[0].packet_id);
	}

	#[test]
	fn propagates_transactions() {
		let mut client = TestBlockChainClient::new();
		client.add_blocks(100, EachBlockWith::Uncle);
		client.insert_transaction_to_queue();
		let mut sync = dummy_sync_with_peer(client.block_hash_delta_minus(1), &client);
		let queue = RwLock::new(VecDeque::new());
		let ss = TestSnapshotService::new();
		let mut io = TestIo::new(&mut client, &ss, &queue, None);
		let peer_count = SyncPropagator::propagate_new_transactions(&mut sync, &mut io, || true);
		// Try to propagate same transactions for the second time
		let peer_count2 = SyncPropagator::propagate_new_transactions(&mut sync, &mut io, || true);
		// Even after new block transactions should not be propagated twice
		sync.chain_new_blocks(&mut io, &[], &[], &[], &[], &[], &[]);
		// Try to propagate same transactions for the third time
		let peer_count3 = SyncPropagator::propagate_new_transactions(&mut sync, &mut io, || true);

		// 1 message should be send
		assert_eq!(1, io.packets.len());
		// 1 peer should be updated but only once
		assert_eq!(1, peer_count);
		assert_eq!(0, peer_count2);
		assert_eq!(0, peer_count3);
		// TRANSACTIONS_PACKET
		assert_eq!(0x02, io.packets[0].packet_id);
	}

	#[test]
	fn does_not_propagate_new_transactions_after_new_block() {
		let mut client = TestBlockChainClient::new();
		client.add_blocks(100, EachBlockWith::Uncle);
		client.insert_transaction_to_queue();
		let mut sync = dummy_sync_with_peer(client.block_hash_delta_minus(1), &client);
		let queue = RwLock::new(VecDeque::new());
		let ss = TestSnapshotService::new();
		let mut io = TestIo::new(&mut client, &ss, &queue, None);
		let peer_count = SyncPropagator::propagate_new_transactions(&mut sync, &mut io, || true);
		io.chain.insert_transaction_to_queue();
		// New block import should not trigger propagation.
		// (we only propagate on timeout)
		sync.chain_new_blocks(&mut io, &[], &[], &[], &[], &[], &[]);

		// 2 message should be send
		assert_eq!(1, io.packets.len());
		// 1 peer should receive the message
		assert_eq!(1, peer_count);
		// TRANSACTIONS_PACKET
		assert_eq!(0x02, io.packets[0].packet_id);
	}

	#[test]
	fn does_not_fail_for_no_peers() {
		let mut client = TestBlockChainClient::new();
		client.add_blocks(100, EachBlockWith::Uncle);
		client.insert_transaction_to_queue();
		// Sync with no peers
		let mut sync = ChainSync::new(SyncConfig::default(), &client, None);
		let queue = RwLock::new(VecDeque::new());
		let ss = TestSnapshotService::new();
		let mut io = TestIo::new(&mut client, &ss, &queue, None);
		let peer_count = SyncPropagator::propagate_new_transactions(&mut sync, &mut io, || true);
		sync.chain_new_blocks(&mut io, &[], &[], &[], &[], &[], &[]);
		// Try to propagate same transactions for the second time
		let peer_count2 = SyncPropagator::propagate_new_transactions(&mut sync, &mut io, || true);

		assert_eq!(0, io.packets.len());
		assert_eq!(0, peer_count);
		assert_eq!(0, peer_count2);
	}

	#[test]
	fn propagates_transactions_without_alternating() {
		let mut client = TestBlockChainClient::new();
		client.add_blocks(100, EachBlockWith::Uncle);
		client.insert_transaction_to_queue();
		let mut sync = dummy_sync_with_peer(client.block_hash_delta_minus(1), &client);
		let queue = RwLock::new(VecDeque::new());
		let ss = TestSnapshotService::new();
		// should sent some
		{
			let mut io = TestIo::new(&mut client, &ss, &queue, None);
			let peer_count = SyncPropagator::propagate_new_transactions(&mut sync, &mut io, || true);
			assert_eq!(1, io.packets.len());
			assert_eq!(1, peer_count);
		}
		// Insert some more
		client.insert_transaction_to_queue();
		let (peer_count2, peer_count3) = {
			let mut io = TestIo::new(&mut client, &ss, &queue, None);
			// Propagate new transactions
			let peer_count2 = SyncPropagator::propagate_new_transactions(&mut sync, &mut io, || true);
			// And now the peer should have all transactions
			let peer_count3 = SyncPropagator::propagate_new_transactions(&mut sync, &mut io, || true);
			(peer_count2, peer_count3)
		};

		// 2 message should be send (in total)
		assert_eq!(2, queue.read().len());
		// 1 peer should be updated but only once after inserting new transaction
		assert_eq!(1, peer_count2);
		assert_eq!(0, peer_count3);
		// TRANSACTIONS_PACKET
		assert_eq!(0x02, queue.read()[0].packet_id);
		assert_eq!(0x02, queue.read()[1].packet_id);
	}

	#[test]
	fn should_maintain_transations_propagation_stats() {
		let mut client = TestBlockChainClient::new();
		client.add_blocks(100, EachBlockWith::Uncle);
		client.insert_transaction_to_queue();
		let mut sync = dummy_sync_with_peer(client.block_hash_delta_minus(1), &client);
		let queue = RwLock::new(VecDeque::new());
		let ss = TestSnapshotService::new();
		let mut io = TestIo::new(&mut client, &ss, &queue, None);
		SyncPropagator::propagate_new_transactions(&mut sync, &mut io, || true);

		let stats = sync.transactions_stats();
		assert_eq!(stats.len(), 1, "Should maintain stats for single transaction.")
	}

	#[test]
	fn should_propagate_service_transaction_to_selected_peers_only() {
		let mut client = TestBlockChainClient::new();
		client.insert_transaction_with_gas_price_to_queue(U256::zero());
		let block_hash = client.block_hash_delta_minus(1);
		let mut sync = ChainSync::new(SyncConfig::default(), &client, None);
		let queue = RwLock::new(VecDeque::new());
		let ss = TestSnapshotService::new();
		let mut io = TestIo::new(&mut client, &ss, &queue, None);

		// when peer#1 is Geth
		insert_dummy_peer(&mut sync, 1, block_hash);
		io.peers_info.insert(1, "Geth".to_owned());
		// and peer#2 is Parity, accepting service transactions
		insert_dummy_peer(&mut sync, 2, block_hash);
		io.peers_info.insert(2, "Parity-Ethereum/v2.6.0/linux/rustc".to_owned());
		// and peer#3 is Parity, accepting service transactions
		insert_dummy_peer(&mut sync, 3, block_hash);
		io.peers_info.insert(3, "Parity-Ethereum/ABCDEFGH/v2.7.3/linux/rustc".to_owned());

		// and new service transaction is propagated to peers
		SyncPropagator::propagate_new_transactions(&mut sync, &mut io, || true);

		// peer#2 && peer#3 are receiving service transaction
		assert!(io.packets.iter().any(|p| p.packet_id == 0x02 && p.recipient == 2)); // TRANSACTIONS_PACKET
		assert!(io.packets.iter().any(|p| p.packet_id == 0x02 && p.recipient == 3)); // TRANSACTIONS_PACKET
		assert_eq!(io.packets.len(), 2);
	}

	#[test]
	fn should_propagate_service_transaction_is_sent_as_separate_message() {
		let mut client = TestBlockChainClient::new();
		let tx1_hash = client.insert_transaction_to_queue();
		let tx2_hash = client.insert_transaction_with_gas_price_to_queue(U256::zero());
		let block_hash = client.block_hash_delta_minus(1);
		let mut sync = ChainSync::new(SyncConfig::default(), &client, None);
		let queue = RwLock::new(VecDeque::new());
		let ss = TestSnapshotService::new();
		let mut io = TestIo::new(&mut client, &ss, &queue, None);

		// when peer#1 is Parity, accepting service transactions
		insert_dummy_peer(&mut sync, 1, block_hash);
		io.peers_info.insert(1, "Parity-Ethereum/v2.6.0/linux/rustc".to_owned());

		// and service + non-service transactions are propagated to peers
		SyncPropagator::propagate_new_transactions(&mut sync, &mut io, || true);

		// two separate packets for peer are queued:
		// 1) with non-service-transaction
		// 2) with service transaction
		let sent_transactions: Vec<UnverifiedTransaction> = io.packets.iter()
			.filter_map(|p| {
				if p.packet_id != 0x02 || p.recipient != 1 { // TRANSACTIONS_PACKET
					return None;
				}

				let rlp = Rlp::new(&*p.data);
				let item_count = rlp.item_count().unwrap_or(0);
				if item_count != 1 {
					return None;
				}

				rlp.at(0).ok().and_then(|r| r.as_val().ok())
			})
			.collect();
		assert_eq!(sent_transactions.len(), 2);
		assert!(sent_transactions.iter().any(|tx| tx.hash() == tx1_hash));
		assert!(sent_transactions.iter().any(|tx| tx.hash() == tx2_hash));
	}
}
