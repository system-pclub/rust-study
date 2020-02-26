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

use std::sync::Arc;
use ethcore::client::{TestBlockChainClient, Executed, TransactionId};
use ethcore_logger::RotatingLogger;
use ethereum_types::{Address, U256, H256};
use ethstore::ethkey::{Generator, Random};
use miner::pool::local_transactions::Status as LocalTransactionStatus;
use sync::ManageNetwork;
use types::receipt::{LocalizedReceipt, TransactionOutcome};

use jsonrpc_core::IoHandler;
use v1::{Parity, ParityClient};
use v1::metadata::Metadata;
use v1::helpers::NetworkSettings;
use v1::helpers::external_signer::SignerService;
use v1::tests::helpers::{TestSyncProvider, Config, TestMinerService, TestUpdater};
use super::manage_network::TestManageNetwork;
use Host;

pub type TestParityClient = ParityClient<TestBlockChainClient, TestMinerService, TestUpdater>;

pub struct Dependencies {
	pub miner: Arc<TestMinerService>,
	pub client: Arc<TestBlockChainClient>,
	pub sync: Arc<TestSyncProvider>,
	pub updater: Arc<TestUpdater>,
	pub logger: Arc<RotatingLogger>,
	pub settings: Arc<NetworkSettings>,
	pub network: Arc<ManageNetwork>,
	pub ws_address: Option<Host>,
}

impl Dependencies {
	pub fn new() -> Self {
		Dependencies {
			miner: Arc::new(TestMinerService::default()),
			client: Arc::new(TestBlockChainClient::default()),
			sync: Arc::new(TestSyncProvider::new(Config {
				network_id: 3,
				num_peers: 120,
			})),
			updater: Arc::new(TestUpdater::default()),
			logger: Arc::new(RotatingLogger::new("rpc=trace".to_owned())),
			settings: Arc::new(NetworkSettings {
				name: "mynode".to_owned(),
				chain: "testchain".to_owned(),
				is_dev_chain: false,
				network_port: 30303,
				rpc_enabled: true,
				rpc_interface: "all".to_owned(),
				rpc_port: 8545,
			}),
			network: Arc::new(TestManageNetwork),
			ws_address: Some("127.0.0.1:18546".into()),
		}
	}

	pub fn client(&self, signer: Option<Arc<SignerService>>) -> TestParityClient {
		ParityClient::new(
			self.client.clone(),
			self.miner.clone(),
			self.sync.clone(),
			self.updater.clone(),
			self.network.clone(),
			self.logger.clone(),
			self.settings.clone(),
			signer,
			self.ws_address.clone(),
			None,
		)
	}

	fn default_client(&self) -> IoHandler<Metadata> {
		let mut io = IoHandler::default();
		io.extend_with(self.client(None).to_delegate());
		io
	}

	fn with_signer(&self, signer: SignerService) -> IoHandler<Metadata> {
		let mut io = IoHandler::default();
		io.extend_with(self.client(Some(Arc::new(signer))).to_delegate());
		io
	}
}

#[test]
fn rpc_parity_consensus_capability() {
	let deps = Dependencies::new();
	let io = deps.default_client();

	let request = r#"{"jsonrpc": "2.0", "method": "parity_consensusCapability", "params": [], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":{"capableUntil":15100},"id":1}"#;
	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));

	deps.updater.set_current_block(15101);

	let request = r#"{"jsonrpc": "2.0", "method": "parity_consensusCapability", "params": [], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":{"incapableSince":15100},"id":1}"#;
	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));

	deps.updater.set_updated(true);

	let request = r#"{"jsonrpc": "2.0", "method": "parity_consensusCapability", "params": [], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"capable","id":1}"#;
	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_parity_version_info() {
	let deps = Dependencies::new();
	let io = deps.default_client();

	let request = r#"{"jsonrpc": "2.0", "method": "parity_versionInfo", "params": [], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":{"hash":"0x0000000000000000000000000000000000000096","track":"beta","version":{"major":1,"minor":5,"patch":0}},"id":1}"#;
	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_parity_releases_info() {
	let deps = Dependencies::new();
	let io = deps.default_client();

	let request = r#"{"jsonrpc": "2.0", "method": "parity_releasesInfo", "params": [], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":{"fork":15100,"minor":null,"this_fork":15000,"track":{"binary":"0x00000000000000000000000000000000000000000000000000000000000005e6","fork":15100,"is_critical":true,"version":{"hash":"0x0000000000000000000000000000000000000097","track":"beta","version":{"major":1,"minor":5,"patch":1}}}},"id":1}"#;
	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_parity_extra_data() {
	let deps = Dependencies::new();
	let io = deps.default_client();

	let request = r#"{"jsonrpc": "2.0", "method": "parity_extraData", "params": [], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"0x01020304","id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_parity_default_extra_data() {
	use version::version_data;
	use bytes::ToPretty;

	let deps = Dependencies::new();
	let io = deps.default_client();

	let request = r#"{"jsonrpc": "2.0", "method": "parity_defaultExtraData", "params": [], "id": 1}"#;
	let response = format!(r#"{{"jsonrpc":"2.0","result":"0x{}","id":1}}"#, version_data().to_hex());

	assert_eq!(io.handle_request_sync(request), Some(response));
}

#[test]
fn rpc_parity_gas_floor_target() {
	let deps = Dependencies::new();
	let io = deps.default_client();

	let request = r#"{"jsonrpc": "2.0", "method": "parity_gasFloorTarget", "params": [], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"0x3039","id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_parity_min_gas_price() {
	let deps = Dependencies::new();
	let io = deps.default_client();

	let request = r#"{"jsonrpc": "2.0", "method": "parity_minGasPrice", "params": [], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"0x1312d00","id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_parity_dev_logs() {
	let deps = Dependencies::new();
	deps.logger.append("a".to_owned());
	deps.logger.append("b".to_owned());

	let io = deps.default_client();

	let request = r#"{"jsonrpc": "2.0", "method": "parity_devLogs", "params":[], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":["b","a"],"id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_parity_dev_logs_levels() {
	let deps = Dependencies::new();
	let io = deps.default_client();

	let request = r#"{"jsonrpc": "2.0", "method": "parity_devLogsLevels", "params":[], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"rpc=trace","id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_parity_transactions_limit() {
	let deps = Dependencies::new();
	let io = deps.default_client();

	let request = r#"{"jsonrpc": "2.0", "method": "parity_transactionsLimit", "params":[], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":1024,"id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_parity_net_chain() {
	let deps = Dependencies::new();
	let io = deps.default_client();

	let request = r#"{"jsonrpc": "2.0", "method": "parity_netChain", "params":[], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"testchain","id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_parity_chain() {
	let deps = Dependencies::new();
	let io = deps.default_client();

	let request = r#"{"jsonrpc": "2.0", "method": "parity_chain", "params":[], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"foundation","id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_parity_net_peers() {
	let deps = Dependencies::new();
	let io = deps.default_client();

	let request = r#"{"jsonrpc": "2.0", "method": "parity_netPeers", "params":[], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":{"active":0,"connected":120,"max":50,"peers":[{"caps":["eth/62","eth/63"],"id":"node1","name":{"ParityClient":{"can_handle_large_requests":true,"compiler":"rustc","identity":"1","name":"Parity-Ethereum","os":"linux","semver":"2.4.0"}},"network":{"localAddress":"127.0.0.1:8888","remoteAddress":"127.0.0.1:7777"},"protocols":{"eth":{"difficulty":"0x28","head":"0000000000000000000000000000000000000000000000000000000000000032","version":62},"pip":null}},{"caps":["eth/63","eth/64"],"id":null,"name":{"ParityClient":{"can_handle_large_requests":true,"compiler":"rustc","identity":"2","name":"Parity-Ethereum","os":"linux","semver":"2.4.0"}},"network":{"localAddress":"127.0.0.1:3333","remoteAddress":"Handshake"},"protocols":{"eth":{"difficulty":null,"head":"000000000000000000000000000000000000000000000000000000000000003c","version":64},"pip":null}}]},"id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_parity_net_port() {
	let deps = Dependencies::new();
	let io = deps.default_client();

	let request = r#"{"jsonrpc": "2.0", "method": "parity_netPort", "params":[], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":30303,"id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_parity_rpc_settings() {
	let deps = Dependencies::new();
	let io = deps.default_client();

	let request = r#"{"jsonrpc": "2.0", "method": "parity_rpcSettings", "params":[], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":{"enabled":true,"interface":"all","port":8545},"id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_parity_node_name() {
	let deps = Dependencies::new();
	let io = deps.default_client();

	let request = r#"{"jsonrpc": "2.0", "method": "parity_nodeName", "params":[], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"mynode","id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_parity_unsigned_transactions_count() {
	let deps = Dependencies::new();
	let io = deps.with_signer(SignerService::new_test(true));

	let request = r#"{"jsonrpc": "2.0", "method": "parity_unsignedTransactionsCount", "params":[], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":0,"id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_parity_unsigned_transactions_count_when_signer_disabled() {
	let deps = Dependencies::new();
	let io = deps.default_client();

	let request = r#"{"jsonrpc": "2.0", "method": "parity_unsignedTransactionsCount", "params":[], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","error":{"code":-32000,"message":"Trusted Signer is disabled. This API is not available."},"id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_parity_pending_transactions() {
	let deps = Dependencies::new();
	let io = deps.default_client();

	let request = r#"{"jsonrpc": "2.0", "method": "parity_pendingTransactions", "params":[], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":[],"id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_parity_encrypt() {
	let deps = Dependencies::new();
	let io = deps.default_client();
	let key = format!("{:x}", Random.generate().unwrap().public());

	let request = r#"{"jsonrpc": "2.0", "method": "parity_encryptMessage", "params":["0x"#.to_owned() + &key + r#"", "0x01"], "id": 1}"#;
	assert!(io.handle_request_sync(&request).unwrap().contains("result"), "Should return success.");
}

#[test]
fn rpc_parity_ws_address() {
	// given
	let mut deps = Dependencies::new();
	let io1 = deps.default_client();
	deps.ws_address = None;
	let io2 = deps.default_client();

	// when
	let request = r#"{"jsonrpc": "2.0", "method": "parity_wsUrl", "params": [], "id": 1}"#;
	let response1 = r#"{"jsonrpc":"2.0","result":"127.0.0.1:18546","id":1}"#;
	let response2 = r#"{"jsonrpc":"2.0","error":{"code":-32000,"message":"WebSockets Server is disabled. This API is not available."},"id":1}"#;

	// then
	assert_eq!(io1.handle_request_sync(request), Some(response1.to_owned()));
	assert_eq!(io2.handle_request_sync(request), Some(response2.to_owned()));
}

#[test]
fn rpc_parity_next_nonce() {
	let deps = Dependencies::new();
	let address = Address::default();
	let io1 = deps.default_client();
	let deps = Dependencies::new();
	deps.miner.increment_nonce(&address);
	deps.miner.increment_nonce(&address);
	deps.miner.increment_nonce(&address);
	let io2 = deps.default_client();

	let request = r#"{
		"jsonrpc": "2.0",
		"method": "parity_nextNonce",
		"params": [""#.to_owned() + &format!("0x{:x}", address) + r#""],
		"id": 1
	}"#;
	let response1 = r#"{"jsonrpc":"2.0","result":"0x0","id":1}"#;
	let response2 = r#"{"jsonrpc":"2.0","result":"0x3","id":1}"#;

	assert_eq!(io1.handle_request_sync(&request), Some(response1.to_owned()));
	assert_eq!(io2.handle_request_sync(&request), Some(response2.to_owned()));
}

#[test]
fn rpc_parity_transactions_stats() {
	let deps = Dependencies::new();
	let io = deps.default_client();

	let request = r#"{"jsonrpc": "2.0", "method": "parity_pendingTransactionsStats", "params":[], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":{"0x0000000000000000000000000000000000000000000000000000000000000001":{"firstSeen":10,"propagatedTo":{"0x00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000080":16}},"0x0000000000000000000000000000000000000000000000000000000000000005":{"firstSeen":16,"propagatedTo":{"0x00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010":1}}},"id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_parity_local_transactions() {
	let deps = Dependencies::new();
	let io = deps.default_client();
	let tx = ::types::transaction::Transaction {
		value: 5.into(),
		gas: 3.into(),
		gas_price: 2.into(),
		action: ::types::transaction::Action::Create,
		data: vec![1, 2, 3],
		nonce: 0.into(),
	}.fake_sign(3.into());
	let tx = Arc::new(::miner::pool::VerifiedTransaction::from_pending_block_transaction(tx));
	deps.miner.local_transactions.lock().insert(10.into(), LocalTransactionStatus::Pending(tx.clone()));
	deps.miner.local_transactions.lock().insert(15.into(), LocalTransactionStatus::Pending(tx.clone()));

	let request = r#"{"jsonrpc": "2.0", "method": "parity_localTransactions", "params":[], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":{"0x000000000000000000000000000000000000000000000000000000000000000a":{"status":"pending"},"0x000000000000000000000000000000000000000000000000000000000000000f":{"status":"pending"}},"id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_parity_chain_status() {
	let deps = Dependencies::new();
	let io = deps.default_client();

	*deps.client.ancient_block.write() = Some((H256::default(), 5));
	*deps.client.first_block.write() = Some((H256::from(U256::from(1234)), 3333));

	let request = r#"{"jsonrpc": "2.0", "method": "parity_chainStatus", "params":[], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":{"blockGap":["0x6","0xd05"]},"id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_parity_node_kind() {
	let deps = Dependencies::new();
	let io = deps.default_client();

	let request = r#"{"jsonrpc": "2.0", "method": "parity_nodeKind", "params":[], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":{"availability":"personal","capability":"full"},"id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_parity_cid() {
	let deps = Dependencies::new();
	let io = deps.default_client();

	let request = r#"{"jsonrpc": "2.0", "method": "parity_cidV0", "params":["0x414243"], "id": 1}"#;
	let response = r#"{"jsonrpc":"2.0","result":"QmSF59MAENc8ZhM4aM1thuAE8w5gDmyfzkAvNoyPea7aDz","id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_parity_call() {
	let deps = Dependencies::new();
	deps.client.set_execution_result(Ok(Executed {
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
	let io = deps.default_client();

	let request = r#"{
		"jsonrpc": "2.0",
		"method": "parity_call",
		"params": [[{
			"from": "0xb60e8dd61c5d32be8058bb8eb970870f07233155",
			"to": "0xd46e8dd67c5d32be8058bb8eb970870f07244567",
			"gas": "0x76c0",
			"gasPrice": "0x9184e72a000",
			"value": "0x9184e72a",
			"data": "0xd46e8dd67c5d32be8d46e8dd67c5d32be8058bb8eb970870f072445675058bb8eb970870f072445675"
		}],
		"latest"],
		"id": 1
	}"#;
	let response = r#"{"jsonrpc":"2.0","result":["0x1234ff"],"id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_parity_block_receipts() {
	let deps = Dependencies::new();
	deps.client.receipts.write()
		.insert(TransactionId::Hash(1.into()), LocalizedReceipt {
			transaction_hash: 1.into(),
			transaction_index: 0,
			block_hash: 3.into(),
			block_number: 0,
			cumulative_gas_used: 21_000.into(),
			gas_used: 21_000.into(),
			contract_address: None,
			logs: vec![],
			log_bloom: 1.into(),
			outcome: TransactionOutcome::Unknown,
			to: None,
			from: 9.into(),
		});
	let io = deps.default_client();

	let request = r#"{
		"jsonrpc": "2.0",
		"method": "parity_getBlockReceipts",
		"params": [],
		"id": 1
	}"#;
	let response = r#"{"jsonrpc":"2.0","result":[{"blockHash":"0x0000000000000000000000000000000000000000000000000000000000000003","blockNumber":"0x0","contractAddress":null,"cumulativeGasUsed":"0x5208","from":"0x0000000000000000000000000000000000000009","gasUsed":"0x5208","logs":[],"logsBloom":"0x00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001","root":null,"status":null,"to":null,"transactionHash":"0x0000000000000000000000000000000000000000000000000000000000000001","transactionIndex":"0x0"}],"id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_status_ok() {
	let deps = Dependencies::new();
	let io = deps.default_client();

	let request = r#"{
		"jsonrpc": "2.0",
		"method": "parity_nodeStatus",
		"params": [],
		"id": 1
	}"#;
	let response = r#"{"jsonrpc":"2.0","result":null,"id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_status_error_peers() {
	let deps = Dependencies::new();
	deps.sync.status.write().num_peers = 0;
	let io = deps.default_client();

	let request = r#"{
		"jsonrpc": "2.0",
		"method": "parity_nodeStatus",
		"params": [],
		"id": 1
	}"#;
	let response = r#"{"jsonrpc":"2.0","error":{"code":-32066,"message":"Node is not connected to any peers."},"id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_status_error_sync() {
	let deps = Dependencies::new();
	deps.sync.status.write().state = ::sync::SyncState::Blocks;
	let io = deps.default_client();

	let request = r#"{
		"jsonrpc": "2.0",
		"method": "parity_nodeStatus",
		"params": [],
		"id": 1
	}"#;
	let response = r#"{"jsonrpc":"2.0","error":{"code":-32001,"message":"Still syncing."},"id":1}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}

#[test]
fn rpc_parity_verify_signature() {
	let deps = Dependencies::new();
	let io = deps.default_client();

	let request = r#"{
		"jsonrpc": "2.0",
		"method": "parity_verifySignature",
		"params": [
			false,
			"0xe552acf4caabe9661893fd48c7b5e68af20bf007193442f8ca05ce836699d75e",
			"0x2089e84151c3cdc45255c07557b349f5bf2ed3e68f6098723eaa90a0f8b2b3e5",
			"0x5f70e8df7bd0c4417afb5f5a39d82e15d03adeff8796725d8b14889ed1d1aa8a",
			"0x1"
		],
		"id": 0
	}"#;

	let response = r#"{"jsonrpc":"2.0","result":{"address":"0x9a2a08a1170f51208c2f3cede0d29ada94481eed","isValidForCurrentChain":true,"publicKey":"0xbeec94ea24444906fe247c47841a45220f07e5718d06157fe4502fac326dab617e973e221e45746721330c2db3f63202268686378cc28b9800c1daaf0bbafeb1"},"id":0}"#;

	assert_eq!(io.handle_request_sync(request), Some(response.to_owned()));
}
