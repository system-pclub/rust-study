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

use std::str::FromStr;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Instant, Duration, SystemTime, UNIX_EPOCH};

use accounts::AccountProvider;
use ethcore::client::{BlockChainClient, BlockId, EachBlockWith, Executed, TestBlockChainClient, TransactionId};
use ethcore::miner::{self, MinerService};
use ethereum_types::{H160, H256, U256, Address};
use miner::external::ExternalMiner;
use parity_runtime::Runtime;
use parking_lot::Mutex;
use rlp;
use rustc_hex::{FromHex, ToHex};
use sync::SyncState;
use types::transaction::{Transaction, Action};
use types::log_entry::{LocalizedLogEntry, LogEntry};
use types::receipt::{LocalizedReceipt, TransactionOutcome};

use jsonrpc_core::IoHandler;
use v1::{Eth, EthClient, EthClientOptions, EthFilter, EthFilterClient};
use v1::tests::helpers::{TestSyncProvider, Config, TestMinerService, TestSnapshotService};
use v1::metadata::Metadata;

fn blockchain_client() -> Arc<TestBlockChainClient> {
	let client = TestBlockChainClient::new();
	Arc::new(client)
}

fn accounts_provider() -> Arc<AccountProvider> {
	Arc::new(AccountProvider::transient_provider())
}

fn sync_provider() -> Arc<TestSyncProvider> {
	Arc::new(TestSyncProvider::new(Config {
		network_id: 3,
		num_peers: 120,
	}))
}

fn miner_service() -> Arc<TestMinerService> {
	Arc::new(TestMinerService::default())
}

fn snapshot_service() -> Arc<TestSnapshotService> {
	Arc::new(TestSnapshotService::new())
}

struct EthTester {
	pub runtime: Runtime,
	pub client: Arc<TestBlockChainClient>,
	pub sync: Arc<TestSyncProvider>,
	pub accounts_provider: Arc<AccountProvider>,
	pub miner: Arc<TestMinerService>,
	pub snapshot: Arc<TestSnapshotService>,
	hashrates: Arc<Mutex<HashMap<H256, (Instant, U256)>>>,
	pub io: IoHandler<Metadata>,
}

impl Default for EthTester {
	fn default() -> Self {
		Self::new_with_options(Default::default())
	}
}

impl EthTester {
	pub fn new_with_options(options: EthClientOptions) -> Self {
		let runtime = Runtime::with_thread_count(1);
		let client = blockchain_client();
		let sync = sync_provider();
		let ap = accounts_provider();
		let ap2 = ap.clone();
		let opt_ap = Arc::new(move || ap2.accounts().unwrap_or_default()) as _;
		let miner = miner_service();
		let snapshot = snapshot_service();
		let hashrates = Arc::new(Mutex::new(HashMap::new()));
		let external_miner = Arc::new(ExternalMiner::new(hashrates.clone()));
		let eth = EthClient::new(&client, &snapshot, &sync, &opt_ap, &miner, &external_miner, options).to_delegate();
		let filter = EthFilterClient::new(client.clone(), miner.clone(), 60).to_delegate();

		let mut io: IoHandler<Metadata> = IoHandler::default();
		io.extend_with(eth);
		io.extend_with(filter);

		EthTester {
			runtime,
			client: client,
			sync: sync,
			accounts_provider: ap,
			miner: miner,
			snapshot: snapshot,
			io: io,
			hashrates: hashrates,
		}
	}

	pub fn add_blocks(&self, count: usize, with: EachBlockWith) {
		self.client.add_blocks(count, with);
		self.sync.increase_imported_block_number(count as u64);
	}
}

#[test]
fn rpc_eth_protocol_version() {
	let request = r#"{"jsonrpc": "2.0", "method": "eth_protocolVersion", "params": [], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"63","id":1}"#;

	assert_eq!(EthTester::default().io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_eth_syncing() {
	use ethcore::snapshot::RestorationStatus;

	let request = r#"{"jsonrpc": "2.0", "method": "eth_syncing", "params": [], "id": 1}"#;

	let tester = EthTester::default();

	let false_res = r#"{"jsonrpc":"2.0","result":false,"id":1}"#;
	assert_eq!(tester.io.handle_request_sync(request), Some(false_res.to_owned()));

	{
		let mut status = tester.sync.status.write();
		status.state = SyncState::Blocks;
		status.highest_block_number = Some(2500);
	}

	// "sync" to 1000 blocks.
	// causes TestBlockChainClient to return 1000 for its best block number.
	tester.add_blocks(1000, EachBlockWith::Nothing);

	let true_res = r#"{"jsonrpc":"2.0","result":{"currentBlock":"0x3e8","highestBlock":"0x9c4","startingBlock":"0x0","warpChunksAmount":null,"warpChunksProcessed":null},"id":1}"#;
	assert_eq!(tester.io.handle_request_sync(request), Some(true_res.to_owned()));

	*tester.client.ancient_block.write() = None;
	*tester.client.first_block.write() = None;

	let snap_res = r#"{"jsonrpc":"2.0","result":{"currentBlock":"0x3e8","highestBlock":"0x9c4","startingBlock":"0x0","warpChunksAmount":"0x32","warpChunksProcessed":"0x18"},"id":1}"#;
	tester.snapshot.set_status(RestorationStatus::Ongoing {
		state_chunks: 40,
		block_chunks: 10,
		state_chunks_done: 18,
		block_chunks_done: 6,
	});

	assert_eq!(tester.io.handle_request_sync(request), Some(snap_res.to_owned()));

	tester.snapshot.set_status(RestorationStatus::Inactive);

	// finish "syncing"
	tester.add_blocks(1500, EachBlockWith::Nothing);

	{
		let mut status = tester.sync.status.write();
		status.state = SyncState::Idle;
	}

	assert_eq!(tester.io.handle_request_sync(request), Some(false_res.to_owned()));
}

#[test]
fn rpc_eth_chain_id() {
	let tester = EthTester::default();
	let request = r#"{"jsonrpc": "2.0", "method": "eth_chainId", "params": [], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":null,"id":1}"#;

	assert_eq!(tester.io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_eth_hashrate() {
	let tester = EthTester::default();
	tester.hashrates.lock().insert(H256::from(0), (Instant::now() + Duration::from_secs(2), U256::from(0xfffa)));
	tester.hashrates.lock().insert(H256::from(0), (Instant::now() + Duration::from_secs(2), U256::from(0xfffb)));
	tester.hashrates.lock().insert(H256::from(1), (Instant::now() + Duration::from_secs(2), U256::from(0x1)));

	let request = r#"{"jsonrpc": "2.0", "method": "eth_hashrate", "params": [], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"0xfffc","id":1}"#;

	assert_eq!(tester.io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_eth_logs() {
	let tester = EthTester::default();
	tester.client.set_logs(vec![LocalizedLogEntry {
		block_number: 1,
		block_hash: H256::default(),
		entry: LogEntry {
			address: Address::default(),
			topics: vec![],
			data: vec![1,2,3],
		},
		transaction_index: 0,
		transaction_log_index: 0,
		transaction_hash: H256::default(),
		log_index: 0,
	}, LocalizedLogEntry {
		block_number: 1,
		block_hash: H256::default(),
		entry: LogEntry {
			address: Address::default(),
			topics: vec![],
			data: vec![1,2,3],
		},
		transaction_index: 0,
		transaction_log_index: 1,
		transaction_hash: H256::default(),
		log_index: 1,
	}]);

	let request1 = r#"{"jsonrpc": "2.0", "method": "eth_getLogs", "params": [{}], "id": 1}"#;
	let request2 = r#"{"jsonrpc": "2.0", "method": "eth_getLogs", "params": [{"limit":1}], "id": 1}"#;
	let request3 = r#"{"jsonrpc": "2.0", "method": "eth_getLogs", "params": [{"limit":0}], "id": 1}"#;

	let response1 = r#"{"jsonrpc":"2.0","result":[{"address":"0x0000000000000000000000000000000000000000","blockHash":"0x0000000000000000000000000000000000000000000000000000000000000000","blockNumber":"0x1","data":"0x010203","logIndex":"0x0","removed":false,"topics":[],"transactionHash":"0x0000000000000000000000000000000000000000000000000000000000000000","transactionIndex":"0x0","transactionLogIndex":"0x0","type":"mined"},{"address":"0x0000000000000000000000000000000000000000","blockHash":"0x0000000000000000000000000000000000000000000000000000000000000000","blockNumber":"0x1","data":"0x010203","logIndex":"0x1","removed":false,"topics":[],"transactionHash":"0x0000000000000000000000000000000000000000000000000000000000000000","transactionIndex":"0x0","transactionLogIndex":"0x1","type":"mined"}],"id":1}"#;
	let response2 = r#"{"jsonrpc":"2.0","result":[{"address":"0x0000000000000000000000000000000000000000","blockHash":"0x0000000000000000000000000000000000000000000000000000000000000000","blockNumber":"0x1","data":"0x010203","logIndex":"0x1","removed":false,"topics":[],"transactionHash":"0x0000000000000000000000000000000000000000000000000000000000000000","transactionIndex":"0x0","transactionLogIndex":"0x1","type":"mined"}],"id":1}"#;
	let response3 = r#"{"jsonrpc":"2.0","result":[],"id":1}"#;

	assert_eq!(tester.io.handle_request_sync(request1), Some(response1.to_owned()));
	assert_eq!(tester.io.handle_request_sync(request2), Some(response2.to_owned()));
	assert_eq!(tester.io.handle_request_sync(request3), Some(response3.to_owned()));
}

#[test]
fn rpc_eth_logs_error() {
	let tester = EthTester::default();
	tester.client.set_error_on_logs(Some(BlockId::Hash(H256::from([5u8].as_ref()))));
	let request = r#"{"jsonrpc": "2.0", "method": "eth_getLogs", "params": [{"limit":1,"blockHash":"0x0000000000000000000000000000000000000000000000000000000000000000"}], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","error":{"code":-32000,"message":"One of the blocks specified in filter (fromBlock, toBlock or blockHash) cannot be found","data":"0x0500000000000000000000000000000000000000000000000000000000000000"},"id":1}"#;
	assert_eq!(tester.io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_logs_filter() {
	let tester = EthTester::default();
	// Set some logs
	tester.client.set_logs(vec![LocalizedLogEntry {
		block_number: 1,
		block_hash: H256::default(),
		entry: LogEntry {
			address: Address::default(),
			topics: vec![],
			data: vec![1,2,3],
		},
		transaction_index: 0,
		transaction_log_index: 0,
		transaction_hash: H256::default(),
		log_index: 0,
	}, LocalizedLogEntry {
		block_number: 1,
		block_hash: H256::default(),
		entry: LogEntry {
			address: Address::default(),
			topics: vec![],
			data: vec![1,2,3],
		},
		transaction_index: 0,
		transaction_log_index: 1,
		transaction_hash: H256::default(),
		log_index: 1,
	}]);

	// Register filters first
	let request_default = r#"{"jsonrpc": "2.0", "method": "eth_newFilter", "params": [{}], "id": 1}"#;
	let request_limit = r#"{"jsonrpc": "2.0", "method": "eth_newFilter", "params": [{"limit":1}], "id": 1}"#;
	let response1 = r#"{"jsonrpc":"2.0","result":"0x0","id":1}"#;
	let response2 = r#"{"jsonrpc":"2.0","result":"0x1","id":1}"#;

	assert_eq!(tester.io.handle_request_sync(request_default), Some(response1.to_owned()));
	assert_eq!(tester.io.handle_request_sync(request_limit), Some(response2.to_owned()));

	let request_changes1 = r#"{"jsonrpc": "2.0", "method": "eth_getFilterChanges", "params": ["0x0"], "id": 1}"#;
	let request_changes2 = r#"{"jsonrpc": "2.0", "method": "eth_getFilterChanges", "params": ["0x1"], "id": 1}"#;
	let response1 = r#"{"jsonrpc":"2.0","result":[{"address":"0x0000000000000000000000000000000000000000","blockHash":"0x0000000000000000000000000000000000000000000000000000000000000000","blockNumber":"0x1","data":"0x010203","logIndex":"0x0","removed":false,"topics":[],"transactionHash":"0x0000000000000000000000000000000000000000000000000000000000000000","transactionIndex":"0x0","transactionLogIndex":"0x0","type":"mined"},{"address":"0x0000000000000000000000000000000000000000","blockHash":"0x0000000000000000000000000000000000000000000000000000000000000000","blockNumber":"0x1","data":"0x010203","logIndex":"0x1","removed":false,"topics":[],"transactionHash":"0x0000000000000000000000000000000000000000000000000000000000000000","transactionIndex":"0x0","transactionLogIndex":"0x1","type":"mined"}],"id":1}"#;
	let response2 = r#"{"jsonrpc":"2.0","result":[{"address":"0x0000000000000000000000000000000000000000","blockHash":"0x0000000000000000000000000000000000000000000000000000000000000000","blockNumber":"0x1","data":"0x010203","logIndex":"0x1","removed":false,"topics":[],"transactionHash":"0x0000000000000000000000000000000000000000000000000000000000000000","transactionIndex":"0x0","transactionLogIndex":"0x1","type":"mined"}],"id":1}"#;

	assert_eq!(tester.io.handle_request_sync(request_changes1), Some(response1.to_owned()));
	assert_eq!(tester.io.handle_request_sync(request_changes2), Some(response2.to_owned()));
}

#[test]
fn rpc_blocks_filter() {
	let tester = EthTester::default();
	let request_filter = r#"{"jsonrpc": "2.0", "method": "eth_newBlockFilter", "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"0x0","id":1}"#;

	assert_eq!(tester.io.handle_request_sync(request_filter), Some(response.to_owned()));

	let request_changes = r#"{"jsonrpc": "2.0", "method": "eth_getFilterChanges", "params": ["0x0"], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":[],"id":1}"#;

	assert_eq!(tester.io.handle_request_sync(request_changes), Some(response.to_owned()));

	tester.client.add_blocks(2, EachBlockWith::Nothing);

	let hash1 = tester.client.block_hash(BlockId::Number(1)).unwrap();
	let hash2 = tester.client.block_hash(BlockId::Number(2)).unwrap();
	let response = format!(
		r#"{{"jsonrpc":"2.0","result":["0x{:x}","0x{:x}"],"id":1}}"#,
		hash1,
		hash2);

	assert_eq!(tester.io.handle_request_sync(request_changes), Some(response.to_owned()));

	// in the case of a re-org we get same block number if hash is different - BlockId::Number(2)
	tester.client.blocks.write().remove(&hash2).unwrap();
	tester.client.numbers.write().remove(&2).unwrap();
	*tester.client.last_hash.write() = hash1;
	tester.client.add_blocks(2, EachBlockWith::Uncle);

	let request_changes = r#"{"jsonrpc": "2.0", "method": "eth_getFilterChanges", "params": ["0x0"], "id": 2}"#;
	let response = format!(
		r#"{{"jsonrpc":"2.0","result":["0x{:x}","0x{:x}"],"id":2}}"#,
		tester.client.block_hash(BlockId::Number(2)).unwrap(),
		tester.client.block_hash(BlockId::Number(3)).unwrap());

	assert_eq!(tester.io.handle_request_sync(request_changes), Some(response.to_owned()));
}

#[test]
fn rpc_eth_submit_hashrate() {
	let tester = EthTester::default();

	let request = r#"{
		"jsonrpc": "2.0",
		"method": "eth_submitHashrate",
		"params": [
			"0x0000000000000000000000000000000000000000000000000000000000500000",
			"0x59daa26581d0acd1fce254fb7e85952f4c09d0915afd33d3886cd914bc7d283c"],
		"id": 1
	}"#;
	let response = r#"{"jsonrpc":"2.0","result":true,"id":1}"#;

	assert_eq!(tester.io.handle_request_sync(request), Some(response.to_owned()));
	assert_eq!(tester.hashrates.lock().get(&H256::from("0x59daa26581d0acd1fce254fb7e85952f4c09d0915afd33d3886cd914bc7d283c")).cloned().unwrap().1,
		U256::from(0x500_000));
}

#[test]
fn rpc_eth_author() {
	let make_res = |addr| r#"{"jsonrpc":"2.0","result":""#.to_owned() + &format!("0x{:x}", addr) + r#"","id":1}"#;
	let tester = EthTester::default();

	let request = r#"{
		"jsonrpc": "2.0",
		"method": "eth_coinbase",
		"params": [],
		"id": 1
	}"#;

	let response = r#"{"jsonrpc":"2.0","error":{"code":-32023,"message":"No accounts were found","data":"\"\""},"id":1}"#;

	// No accounts - returns an error indicating that no accounts were found
	assert_eq!(tester.io.handle_request_sync(request), Some(response.to_string()));

	// Account set - return first account
	let addr = tester.accounts_provider.new_account(&"123".into()).unwrap();
	assert_eq!(tester.io.handle_request_sync(request), Some(make_res(addr)));

	for i in 0..20 {
		let addr = tester.accounts_provider.new_account(&format!("{}", i).into()).unwrap();
		tester.miner.set_author(miner::Author::External(addr));

		assert_eq!(tester.io.handle_request_sync(request), Some(make_res(addr)));
	}
}

#[test]
fn rpc_eth_mining() {
	let tester = EthTester::default();
	tester.miner.set_author(miner::Author::External(Address::from_str("d46e8dd67c5d32be8058bb8eb970870f07244567").unwrap()));

	let request = r#"{"jsonrpc": "2.0", "method": "eth_mining", "params": [], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":false,"id":1}"#;
	assert_eq!(tester.io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_eth_gas_price() {
	let request = r#"{"jsonrpc": "2.0", "method": "eth_gasPrice", "params": [], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"0x4a817c800","id":1}"#;

	assert_eq!(EthTester::default().io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_eth_accounts() {
	let tester = EthTester::default();
	let address = tester.accounts_provider.new_account(&"".into()).unwrap();
	tester.accounts_provider.set_address_name(1.into(), "1".into());
	tester.accounts_provider.set_address_name(10.into(), "10".into());

	// with current policy it should return the account
	let request = r#"{"jsonrpc": "2.0", "method": "eth_accounts", "params": [], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":[""#.to_owned() + &format!("0x{:x}", address) + r#""],"id":1}"#;
	assert_eq!(tester.io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_eth_block_number() {
	let tester = EthTester::default();
	tester.client.add_blocks(10, EachBlockWith::Nothing);

	let request = r#"{"jsonrpc": "2.0", "method": "eth_blockNumber", "params": [], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"0xa","id":1}"#;

	assert_eq!(tester.io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_eth_balance() {
	let tester = EthTester::default();
	tester.client.set_balance(Address::from(1), U256::from(5));

	let request = r#"{
		"jsonrpc": "2.0",
		"method": "eth_getBalance",
		"params": ["0x0000000000000000000000000000000000000001", "latest"],
		"id": 1
	}"#;
	let response = r#"{"jsonrpc":"2.0","result":"0x5","id":1}"#;

	assert_eq!(tester.io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_eth_balance_pending() {
	let tester = EthTester::default();
	tester.client.set_balance(Address::from(1), U256::from(5));

	let request = r#"{
		"jsonrpc": "2.0",
		"method": "eth_getBalance",
		"params": ["0x0000000000000000000000000000000000000001", "pending"],
		"id": 1
	}"#;

	let response = r#"{"jsonrpc":"2.0","result":"0x5","id":1}"#;

	assert_eq!(tester.io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_eth_storage_at() {
	let tester = EthTester::default();
	tester.client.set_storage(Address::from(1), H256::from(4), H256::from(7));

	let request = r#"{
		"jsonrpc": "2.0",
		"method": "eth_getStorageAt",
		"params": ["0x0000000000000000000000000000000000000001", "0x4", "latest"],
		"id": 1
	}"#;
	let response = r#"{"jsonrpc":"2.0","result":"0x0000000000000000000000000000000000000000000000000000000000000007","id":1}"#;

	assert_eq!(tester.io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_eth_transaction_count() {
	let request = r#"{
		"jsonrpc": "2.0",
		"method": "eth_getTransactionCount",
		"params": ["0x0000000000000000000000000000000000000001", "latest"],
		"id": 1
	}"#;
	let response = r#"{"jsonrpc":"2.0","result":"0x0","id":1}"#;

	assert_eq!(EthTester::default().io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_eth_transaction_count_next_nonce() {
	let tester = EthTester::new_with_options(EthClientOptions::with(|options| {
		options.pending_nonce_from_queue = true;
	}));
	tester.miner.increment_nonce(&1.into());

	let request1 = r#"{
		"jsonrpc": "2.0",
		"method": "eth_getTransactionCount",
		"params": ["0x0000000000000000000000000000000000000001", "pending"],
		"id": 1
	}"#;
	let response1 = r#"{"jsonrpc":"2.0","result":"0x1","id":1}"#;
	assert_eq!(tester.io.handle_request_sync(request1), Some(response1.to_owned()));

	let request2 = r#"{
		"jsonrpc": "2.0",
		"method": "eth_getTransactionCount",
		"params": ["0x0000000000000000000000000000000000000002", "pending"],
		"id": 1
	}"#;
	let response2 = r#"{"jsonrpc":"2.0","result":"0x0","id":1}"#;
	assert_eq!(tester.io.handle_request_sync(request2), Some(response2.to_owned()));
}

#[test]
fn rpc_eth_block_transaction_count_by_hash() {
	let request = r#"{
		"jsonrpc": "2.0",
		"method": "eth_getBlockTransactionCountByHash",
		"params": ["0xb903239f8543d04b5dc1ba6579132b143087c68db1b2168786408fcbce568238"],
		"id": 1
	}"#;
	let response = r#"{"jsonrpc":"2.0","result":null,"id":1}"#;

	assert_eq!(EthTester::default().io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_eth_transaction_count_by_number() {
	let request = r#"{
		"jsonrpc": "2.0",
		"method": "eth_getBlockTransactionCountByNumber",
		"params": ["latest"],
		"id": 1
	}"#;
	let response = r#"{"jsonrpc":"2.0","result":"0x0","id":1}"#;

	assert_eq!(EthTester::default().io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_eth_transaction_count_by_number_pending() {
	let request = r#"{
		"jsonrpc": "2.0",
		"method": "eth_getBlockTransactionCountByNumber",
		"params": ["pending"],
		"id": 1
	}"#;
	let response = r#"{"jsonrpc":"2.0","result":"0x0","id":1}"#;

	assert_eq!(EthTester::default().io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_eth_pending_transaction_by_hash() {
	use ethereum_types::H256;
	use rlp;
	use types::transaction::SignedTransaction;

	let tester = EthTester::default();
	{
		let bytes = FromHex::from_hex("f85f800182520894095e7baea6a6c7c4c2dfeb977efac326af552d870a801ba048b55bfa915ac795c431978d8a6a992b628d557da5ff759b307d495a36649353a0efffd310ac743f371de3b9f7f9cb56c0b28ad43601b4ab949f53faa07bd2c804").unwrap();
		let tx = rlp::decode(&bytes).expect("decoding failure");
		let tx = SignedTransaction::new(tx).unwrap();
		tester.miner.pending_transactions.lock().insert(H256::zero(), tx);
	}

	let response = r#"{"jsonrpc":"2.0","result":{"blockHash":null,"blockNumber":null,"chainId":null,"condition":null,"creates":null,"from":"0x0f65fe9276bc9a24ae7083ae28e2660ef72df99e","gas":"0x5208","gasPrice":"0x1","hash":"0x41df922fd0d4766fcc02e161f8295ec28522f329ae487f14d811e4b64c8d6e31","input":"0x","nonce":"0x0","publicKey":"0x7ae46da747962c2ee46825839c1ef9298e3bd2e70ca2938495c3693a485ec3eaa8f196327881090ff64cf4fbb0a48485d4f83098e189ed3b7a87d5941b59f789","r":"0x48b55bfa915ac795c431978d8a6a992b628d557da5ff759b307d495a36649353","raw":"0xf85f800182520894095e7baea6a6c7c4c2dfeb977efac326af552d870a801ba048b55bfa915ac795c431978d8a6a992b628d557da5ff759b307d495a36649353a0efffd310ac743f371de3b9f7f9cb56c0b28ad43601b4ab949f53faa07bd2c804","s":"0xefffd310ac743f371de3b9f7f9cb56c0b28ad43601b4ab949f53faa07bd2c804","standardV":"0x0","to":"0x095e7baea6a6c7c4c2dfeb977efac326af552d87","transactionIndex":null,"v":"0x1b","value":"0xa"},"id":1}"#;
	let request = r#"{
		"jsonrpc": "2.0",
		"method": "eth_getTransactionByHash",
		"params": ["0x0000000000000000000000000000000000000000000000000000000000000000"],
		"id": 1
	}"#;
	assert_eq!(tester.io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_eth_uncle_count_by_block_hash() {
	let request = r#"{
		"jsonrpc": "2.0",
		"method": "eth_getUncleCountByBlockHash",
		"params": ["0xb903239f8543d04b5dc1ba6579132b143087c68db1b2168786408fcbce568238"],
		"id": 1
	}"#;
	let response = r#"{"jsonrpc":"2.0","result":null,"id":1}"#;

	assert_eq!(EthTester::default().io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_eth_uncle_count_by_block_number() {
	let request = r#"{
		"jsonrpc": "2.0",
		"method": "eth_getUncleCountByBlockNumber",
		"params": ["latest"],
		"id": 1
	}"#;
	let response = r#"{"jsonrpc":"2.0","result":"0x0","id":1}"#;

	assert_eq!(EthTester::default().io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_eth_code() {
	let tester = EthTester::default();
	tester.client.set_code(Address::from(1), vec![0xff, 0x21]);

	let request = r#"{
		"jsonrpc": "2.0",
		"method": "eth_getCode",
		"params": ["0x0000000000000000000000000000000000000001", "latest"],
		"id": 1
	}"#;
	let response = r#"{"jsonrpc":"2.0","result":"0xff21","id":1}"#;

	assert_eq!(tester.io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_eth_call_latest() {
	let tester = EthTester::default();
	tester.client.set_execution_result(Ok(Executed {
		exception: None,
		gas: U256::zero(),
		gas_used: U256::from(0xff30),
		refunded: U256::from(0x5),
		cumulative_gas_used: U256::zero(),
		logs: vec![],
		contracts_created: vec![],
		output: vec![0x12, 0x34, 0xff],
		trace: vec![],
		vm_trace: None,
		state_diff: None,
	}));

	let request = r#"{
		"jsonrpc": "2.0",
		"method": "eth_call",
		"params": [{
			"from": "0xb60e8dd61c5d32be8058bb8eb970870f07233155",
			"to": "0xd46e8dd67c5d32be8058bb8eb970870f07244567",
			"gas": "0x76c0",
			"gasPrice": "0x9184e72a000",
			"value": "0x9184e72a",
			"data": "0xd46e8dd67c5d32be8d46e8dd67c5d32be8058bb8eb970870f072445675058bb8eb970870f072445675"
		},
		"latest"],
		"id": 1
	}"#;
	let response = r#"{"jsonrpc":"2.0","result":"0x1234ff","id":1}"#;

	assert_eq!(tester.io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_eth_call() {
	let tester = EthTester::default();
	tester.client.set_execution_result(Ok(Executed {
		exception: None,
		gas: U256::zero(),
		gas_used: U256::from(0xff30),
		refunded: U256::from(0x5),
		cumulative_gas_used: U256::zero(),
		logs: vec![],
		contracts_created: vec![],
		output: vec![0x12, 0x34, 0xff],
		trace: vec![],
		vm_trace: None,
		state_diff: None,
	}));

	let request = r#"{
		"jsonrpc": "2.0",
		"method": "eth_call",
		"params": [{
			"from": "0xb60e8dd61c5d32be8058bb8eb970870f07233155",
			"to": "0xd46e8dd67c5d32be8058bb8eb970870f07244567",
			"gas": "0x76c0",
			"gasPrice": "0x9184e72a000",
			"value": "0x9184e72a",
			"data": "0xd46e8dd67c5d32be8d46e8dd67c5d32be8058bb8eb970870f072445675058bb8eb970870f072445675"
		},
		"0x0"],
		"id": 1
	}"#;
	let response = r#"{"jsonrpc":"2.0","result":"0x1234ff","id":1}"#;

	assert_eq!(tester.io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_eth_call_default_block() {
	let tester = EthTester::default();
	tester.client.set_execution_result(Ok(Executed {
		exception: None,
		gas: U256::zero(),
		gas_used: U256::from(0xff30),
		refunded: U256::from(0x5),
		cumulative_gas_used: U256::zero(),
		logs: vec![],
		contracts_created: vec![],
		output: vec![0x12, 0x34, 0xff],
		trace: vec![],
		vm_trace: None,
		state_diff: None,
	}));

	let request = r#"{
		"jsonrpc": "2.0",
		"method": "eth_call",
		"params": [{
			"from": "0xb60e8dd61c5d32be8058bb8eb970870f07233155",
			"to": "0xd46e8dd67c5d32be8058bb8eb970870f07244567",
			"gas": "0x76c0",
			"gasPrice": "0x9184e72a000",
			"value": "0x9184e72a",
			"data": "0xd46e8dd67c5d32be8d46e8dd67c5d32be8058bb8eb970870f072445675058bb8eb970870f072445675"
		}],
		"id": 1
	}"#;
	let response = r#"{"jsonrpc":"2.0","result":"0x1234ff","id":1}"#;

	assert_eq!(tester.io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_eth_estimate_gas() {
	let tester = EthTester::default();
	tester.client.set_execution_result(Ok(Executed {
		exception: None,
		gas: U256::zero(),
		gas_used: U256::from(0xff30),
		refunded: U256::from(0x5),
		cumulative_gas_used: U256::zero(),
		logs: vec![],
		contracts_created: vec![],
		output: vec![0x12, 0x34, 0xff],
		trace: vec![],
		vm_trace: None,
		state_diff: None,
	}));

	let request = r#"{
		"jsonrpc": "2.0",
		"method": "eth_estimateGas",
		"params": [{
			"from": "0xb60e8dd61c5d32be8058bb8eb970870f07233155",
			"to": "0xd46e8dd67c5d32be8058bb8eb970870f07244567",
			"gas": "0x76c0",
			"gasPrice": "0x9184e72a000",
			"value": "0x9184e72a",
			"data": "0xd46e8dd67c5d32be8d46e8dd67c5d32be8058bb8eb970870f072445675058bb8eb970870f072445675"
		},
		"latest"],
		"id": 1
	}"#;
	let response = r#"{"jsonrpc":"2.0","result":"0x5208","id":1}"#;

	assert_eq!(tester.io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_eth_estimate_gas_default_block() {
	let tester = EthTester::default();
	tester.client.set_execution_result(Ok(Executed {
		exception: None,
		gas: U256::zero(),
		gas_used: U256::from(0xff30),
		refunded: U256::from(0x5),
		cumulative_gas_used: U256::zero(),
		logs: vec![],
		contracts_created: vec![],
		output: vec![0x12, 0x34, 0xff],
		trace: vec![],
		vm_trace: None,
		state_diff: None,
	}));

	let request = r#"{
		"jsonrpc": "2.0",
		"method": "eth_estimateGas",
		"params": [{
			"from": "0xb60e8dd61c5d32be8058bb8eb970870f07233155",
			"to": "0xd46e8dd67c5d32be8058bb8eb970870f07244567",
			"gas": "0x76c0",
			"gasPrice": "0x9184e72a000",
			"value": "0x9184e72a",
			"data": "0xd46e8dd67c5d32be8d46e8dd67c5d32be8058bb8eb970870f072445675058bb8eb970870f072445675"
		}],
		"id": 1
	}"#;
	let response = r#"{"jsonrpc":"2.0","result":"0x5208","id":1}"#;

	assert_eq!(tester.io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_eth_send_raw_transaction_error() {
	let tester = EthTester::default();

	let req = r#"{
		"jsonrpc": "2.0",
		"method": "eth_sendRawTransaction",
		"params": [
			"0x0123"
		],
		"id": 1
	}"#;
	let res = r#"{"jsonrpc":"2.0","error":{"code":-32602,"message":"Invalid RLP.","data":"RlpExpectedToBeList"},"id":1}"#.into();

	assert_eq!(tester.io.handle_request_sync(&req), Some(res));
}

#[test]
fn rpc_eth_send_raw_transaction() {
	let tester = EthTester::default();
	let address = tester.accounts_provider.new_account(&"abcd".into()).unwrap();
	tester.accounts_provider.unlock_account_permanently(address, "abcd".into()).unwrap();

	let t = Transaction {
		nonce: U256::zero(),
		gas_price: U256::from(0x9184e72a000u64),
		gas: U256::from(0x76c0),
		action: Action::Call(Address::from_str("d46e8dd67c5d32be8058bb8eb970870f07244567").unwrap()),
		value: U256::from(0x9184e72au64),
		data: vec![]
	};
	let signature = tester.accounts_provider.sign(address, None, t.hash(None)).unwrap();
	let t = t.with_signature(signature, None);

	let rlp = rlp::encode(&t).to_hex();

	let req = r#"{
		"jsonrpc": "2.0",
		"method": "eth_sendRawTransaction",
		"params": [
			"0x"#.to_owned() + &rlp + r#""
		],
		"id": 1
	}"#;

	let res = r#"{"jsonrpc":"2.0","result":""#.to_owned() + &format!("0x{:x}", t.hash()) + r#"","id":1}"#;

	assert_eq!(tester.io.handle_request_sync(&req), Some(res));
}

#[test]
fn rpc_eth_transaction_receipt() {
	let receipt = LocalizedReceipt {
		from: H160::from_str("b60e8dd61c5d32be8058bb8eb970870f07233155").unwrap(),
		to: Some(H160::from_str("d46e8dd67c5d32be8058bb8eb970870f07244567").unwrap()),
		transaction_hash: H256::zero(),
		transaction_index: 0,
		block_hash: H256::from_str("ed76641c68a1c641aee09a94b3b471f4dc0316efe5ac19cf488e2674cf8d05b5").unwrap(),
		block_number: 0x4510c,
		cumulative_gas_used: U256::from(0x20),
		gas_used: U256::from(0x10),
		contract_address: None,
		logs: vec![LocalizedLogEntry {
			entry: LogEntry {
				address: Address::from_str("33990122638b9132ca29c723bdf037f1a891a70c").unwrap(),
				topics: vec![
					H256::from_str("a6697e974e6a320f454390be03f74955e8978f1a6971ea6730542e37b66179bc").unwrap(),
					H256::from_str("4861736852656700000000000000000000000000000000000000000000000000").unwrap()
				],
				data: vec![],
			},
			block_hash: H256::from_str("ed76641c68a1c641aee09a94b3b471f4dc0316efe5ac19cf488e2674cf8d05b5").unwrap(),
			block_number: 0x4510c,
			transaction_hash: H256::new(),
			transaction_index: 0,
			transaction_log_index: 0,
			log_index: 1,
		}],
		log_bloom: 0.into(),
		outcome: TransactionOutcome::StateRoot(0.into()),
	};

	let hash = H256::from_str("b903239f8543d04b5dc1ba6579132b143087c68db1b2168786408fcbce568238").unwrap();
	let tester = EthTester::default();
	tester.client.set_transaction_receipt(TransactionId::Hash(hash), receipt);

	let request = r#"{
		"jsonrpc": "2.0",
		"method": "eth_getTransactionReceipt",
		"params": ["0xb903239f8543d04b5dc1ba6579132b143087c68db1b2168786408fcbce568238"],
		"id": 1
	}"#;
	let response = r#"{"jsonrpc":"2.0","result":{"blockHash":"0xed76641c68a1c641aee09a94b3b471f4dc0316efe5ac19cf488e2674cf8d05b5","blockNumber":"0x4510c","contractAddress":null,"cumulativeGasUsed":"0x20","from":"0xb60e8dd61c5d32be8058bb8eb970870f07233155","gasUsed":"0x10","logs":[{"address":"0x33990122638b9132ca29c723bdf037f1a891a70c","blockHash":"0xed76641c68a1c641aee09a94b3b471f4dc0316efe5ac19cf488e2674cf8d05b5","blockNumber":"0x4510c","data":"0x","logIndex":"0x1","removed":false,"topics":["0xa6697e974e6a320f454390be03f74955e8978f1a6971ea6730542e37b66179bc","0x4861736852656700000000000000000000000000000000000000000000000000"],"transactionHash":"0x0000000000000000000000000000000000000000000000000000000000000000","transactionIndex":"0x0","transactionLogIndex":"0x0","type":"mined"}],"logsBloom":"0x00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000","root":"0x0000000000000000000000000000000000000000000000000000000000000000","status":null,"to":"0xd46e8dd67c5d32be8058bb8eb970870f07244567","transactionHash":"0x0000000000000000000000000000000000000000000000000000000000000000","transactionIndex":"0x0"},"id":1}"#;

	assert_eq!(tester.io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_eth_transaction_receipt_null() {
	let tester = EthTester::default();

	let request = r#"{
		"jsonrpc": "2.0",
		"method": "eth_getTransactionReceipt",
		"params": ["0xb903239f8543d04b5dc1ba6579132b143087c68db1b2168786408fcbce568238"],
		"id": 1
	}"#;
	let response = r#"{"jsonrpc":"2.0","result":null,"id":1}"#;

	assert_eq!(tester.io.handle_request_sync(request), Some(response.to_owned()));
}

// These tests are incorrect: their output is undefined as long as eth_getCompilers is [].
// Will ignore for now, but should probably be replaced by more substantial tests which check
// the output of eth_getCompilers to determine whether to test. CI systems can then be preinstalled
// with solc/serpent/lllc and they'll be proper again.
#[ignore]
#[test]
fn rpc_eth_compilers() {
	let request = r#"{"jsonrpc": "2.0", "method": "eth_getCompilers", "params": [], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","error":{"code":-32070,"message":"Method deprecated","data":"Compilation functionality is deprecated."},"id":1}"#;

	assert_eq!(EthTester::default().io.handle_request_sync(request), Some(response.to_owned()));
}

#[ignore]
#[test]
fn rpc_eth_compile_lll() {
	let request = r#"{"jsonrpc": "2.0", "method": "eth_compileLLL", "params": [], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","error":{"code":-32070,"message":"Method deprecated","data":"Compilation of LLL via RPC is deprecated"},"id":1}"#;

	assert_eq!(EthTester::default().io.handle_request_sync(request), Some(response.to_owned()));
}

#[ignore]
#[test]
fn rpc_eth_compile_solidity() {
	let request = r#"{"jsonrpc": "2.0", "method": "eth_compileSolidity", "params": [], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","error":{"code":-32070,"message":"Method deprecated","data":"Compilation of Solidity via RPC is deprecated"},"id":1}"#;

	assert_eq!(EthTester::default().io.handle_request_sync(request), Some(response.to_owned()));
}

#[ignore]
#[test]
fn rpc_eth_compile_serpent() {
	let request = r#"{"jsonrpc": "2.0", "method": "eth_compileSerpent", "params": [], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","error":{"code":-32070,"message":"Method deprecated","data":"Compilation of Serpent via RPC is deprecated"},"id":1}"#;

	assert_eq!(EthTester::default().io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_get_work_returns_no_work_if_cant_mine() {
	let eth_tester = EthTester::default();
	eth_tester.client.set_queue_size(10);

	let request = r#"{"jsonrpc": "2.0", "method": "eth_getWork", "params": [], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","error":{"code":-32001,"message":"Still syncing."},"id":1}"#;

	assert_eq!(eth_tester.io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_get_work_returns_correct_work_package() {
	let eth_tester = EthTester::default();
	eth_tester.miner.set_author(miner::Author::External(Address::from_str("d46e8dd67c5d32be8058bb8eb970870f07244567").unwrap()));

	let request = r#"{"jsonrpc": "2.0", "method": "eth_getWork", "params": [], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":["0x76c7bd86693aee93d1a80a408a09a0585b1a1292afcb56192f171d925ea18e2d","0x0000000000000000000000000000000000000000000000000000000000000000","0x0000800000000000000000000000000000000000000000000000000000000000","0x1"],"id":1}"#;

	assert_eq!(eth_tester.io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_get_work_should_not_return_block_number() {
	let eth_tester = EthTester::new_with_options(EthClientOptions::with(|options| {
		options.send_block_number_in_get_work = false;
	}));
	eth_tester.miner.set_author(miner::Author::External(Address::from_str("d46e8dd67c5d32be8058bb8eb970870f07244567").unwrap()));

	let request = r#"{"jsonrpc": "2.0", "method": "eth_getWork", "params": [], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":["0x76c7bd86693aee93d1a80a408a09a0585b1a1292afcb56192f171d925ea18e2d","0x0000000000000000000000000000000000000000000000000000000000000000","0x0000800000000000000000000000000000000000000000000000000000000000"],"id":1}"#;

	assert_eq!(eth_tester.io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_get_work_should_timeout() {
	let eth_tester = EthTester::default();
	eth_tester.miner.set_author(miner::Author::External(Address::from_str("d46e8dd67c5d32be8058bb8eb970870f07244567").unwrap()));
	let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() - 1000;  // Set latest block to 1000 seconds ago
	eth_tester.client.set_latest_block_timestamp(timestamp);
	let hash = eth_tester.miner.work_package(&*eth_tester.client).unwrap().0;

	// Request without providing timeout. This should work since we're disabling timeout.
	let request = r#"{"jsonrpc": "2.0", "method": "eth_getWork", "params": [], "id": 1}"#;
	let work_response = format!(
		r#"{{"jsonrpc":"2.0","result":["0x{:x}","0x0000000000000000000000000000000000000000000000000000000000000000","0x0000800000000000000000000000000000000000000000000000000000000000","0x1"],"id":1}}"#,
		hash,
	);
	assert_eq!(eth_tester.io.handle_request_sync(request), Some(work_response.to_owned()));

	// Request with timeout of 0 seconds. This should work since we're disabling timeout.
	let request = r#"{"jsonrpc": "2.0", "method": "eth_getWork", "params": [0], "id": 1}"#;
	let work_response = format!(
		r#"{{"jsonrpc":"2.0","result":["0x{:x}","0x0000000000000000000000000000000000000000000000000000000000000000","0x0000800000000000000000000000000000000000000000000000000000000000","0x1"],"id":1}}"#,
		hash,
	);
	assert_eq!(eth_tester.io.handle_request_sync(request), Some(work_response.to_owned()));

	// Request with timeout of 10K seconds. This should work.
	let request = r#"{"jsonrpc": "2.0", "method": "eth_getWork", "params": [10000], "id": 1}"#;
	assert_eq!(eth_tester.io.handle_request_sync(request), Some(work_response.to_owned()));

	// Request with timeout of 10 seconds. This should fail.
	let request = r#"{"jsonrpc": "2.0", "method": "eth_getWork", "params": [10], "id": 1}"#;
	let err_response = r#"{"jsonrpc":"2.0","error":{"code":-32003,"message":"Work has not changed."},"id":1}"#;
	assert_eq!(eth_tester.io.handle_request_sync(request), Some(err_response.to_owned()));
}
