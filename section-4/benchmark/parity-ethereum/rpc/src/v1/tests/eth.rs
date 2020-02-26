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

//! rpc integration tests.
use std::env;
use std::sync::Arc;

use accounts::AccountProvider;
use ethcore::client::{BlockChainClient, Client, ClientConfig, ChainInfo, ImportBlock};
use ethcore::ethereum;
use ethcore::miner::Miner;
use ethcore::spec::{Genesis, Spec};
use ethcore::test_helpers;
use ethcore::verification::VerifierType;
use ethcore::verification::queue::kind::blocks::Unverified;
use ethereum_types::{Address, H256, U256};
use ethjson::blockchain::BlockChain;
use ethjson::spec::ForkSpec;
use io::IoChannel;
use miner::external::ExternalMiner;
use parity_runtime::Runtime;
use parking_lot::Mutex;
use types::ids::BlockId;

use jsonrpc_core::IoHandler;
use v1::helpers::dispatch::{self, FullDispatcher};
use v1::helpers::nonce;
use v1::impls::{EthClient, EthClientOptions, SigningUnsafeClient};
use v1::metadata::Metadata;
use v1::tests::helpers::{TestSnapshotService, TestSyncProvider, Config};
use v1::traits::{Eth, EthSigning};

fn account_provider() -> Arc<AccountProvider> {
	Arc::new(AccountProvider::transient_provider())
}

fn sync_provider() -> Arc<TestSyncProvider> {
	Arc::new(TestSyncProvider::new(Config {
		network_id: 3,
		num_peers: 120,
	}))
}

fn miner_service(spec: &Spec) -> Arc<Miner> {
	Arc::new(Miner::new_for_tests(spec, None))
}

fn snapshot_service() -> Arc<TestSnapshotService> {
	Arc::new(TestSnapshotService::new())
}

fn make_spec(chain: &BlockChain) -> Spec {
	let genesis = Genesis::from(chain.genesis());
	let mut spec = ethereum::new_frontier_test();
	let state = chain.pre_state.clone().into();
	spec.set_genesis_state(state).expect("unable to set genesis state");
	spec.overwrite_genesis_params(genesis);
	assert!(spec.is_state_root_valid());
	spec
}

struct EthTester {
	_miner: Arc<Miner>,
	_runtime: Runtime,
	_snapshot: Arc<TestSnapshotService>,
	accounts: Arc<AccountProvider>,
	client: Arc<Client>,
	handler: IoHandler<Metadata>,
}

impl EthTester {
	fn from_chain(chain: &BlockChain) -> Self {

		let tester = if ::ethjson::blockchain::Engine::NoProof == chain.engine {
			let mut config = ClientConfig::default();
			config.verifier_type = VerifierType::CanonNoSeal;
			config.check_seal = false;
			Self::from_spec_conf(make_spec(chain), config)
		} else {
			Self::from_spec(make_spec(chain))
		};

		for b in chain.blocks_rlp() {
			if let Ok(block) = Unverified::from_rlp(b) {
				let _ = tester.client.import_block(block);
				tester.client.flush_queue();
				tester.client.import_verified_blocks();
			}
		}

		tester.client.flush_queue();

		assert!(tester.client.chain_info().best_block_hash == chain.best_block.clone().into());
		tester
	}

	fn from_spec(spec: Spec) -> Self {
		let config = ClientConfig::default();
		Self::from_spec_conf(spec, config)
	}

	fn from_spec_conf(spec: Spec, config: ClientConfig) -> Self {
		let runtime = Runtime::with_thread_count(1);
		let account_provider = account_provider();
		let ap = account_provider.clone();
		let accounts  = Arc::new(move || ap.accounts().unwrap_or_default()) as _;
		let miner_service = miner_service(&spec);
		let snapshot_service = snapshot_service();

		let client = Client::new(
			config,
			&spec,
			test_helpers::new_db(),
			miner_service.clone(),
			IoChannel::disconnected(),
		).unwrap();
		let sync_provider = sync_provider();
		let external_miner = Arc::new(ExternalMiner::default());

		let eth_client = EthClient::new(
			&client,
			&snapshot_service,
			&sync_provider,
			&accounts,
			&miner_service,
			&external_miner,
			EthClientOptions {
				pending_nonce_from_queue: false,
				allow_pending_receipt_query: true,
				send_block_number_in_get_work: true,
				gas_price_percentile: 50,
				allow_experimental_rpcs: true,
				allow_missing_blocks: false
			},
		);

		let reservations = Arc::new(Mutex::new(nonce::Reservations::new(runtime.executor())));

		let dispatcher = FullDispatcher::new(client.clone(), miner_service.clone(), reservations, 50);
		let signer = Arc::new(dispatch::Signer::new(account_provider.clone())) as _;
		let eth_sign = SigningUnsafeClient::new(
			&signer,
			dispatcher,
		);

		let mut handler = IoHandler::default();
		handler.extend_with(eth_client.to_delegate());
		handler.extend_with(eth_sign.to_delegate());

		EthTester {
			_miner: miner_service,
			_runtime: runtime,
			_snapshot: snapshot_service,
			accounts: account_provider,
			client: client,
			handler: handler,
		}
	}
}

#[test]
fn harness_works() {
	let chain: BlockChain = extract_chain!("BlockchainTests/bcWalletTest/wallet2outOf3txs");
	let _ = EthTester::from_chain(&chain);
}

#[test]
fn eth_get_balance() {
	let chain = extract_chain!("BlockchainTests/bcWalletTest/wallet2outOf3txs");
	let tester = EthTester::from_chain(&chain);
	// final account state
	let req_latest = r#"{
		"jsonrpc": "2.0",
		"method": "eth_getBalance",
		"params": ["0xaaaf5374fce5edbc8e2a8697c15331677e6ebaaa", "latest"],
		"id": 1
	}"#;
	let res_latest = r#"{"jsonrpc":"2.0","result":"0x9","id":1}"#.to_owned();
	assert_eq!(tester.handler.handle_request_sync(req_latest).unwrap(), res_latest);

	// non-existant account
	let req_new_acc = r#"{
		"jsonrpc": "2.0",
		"method": "eth_getBalance",
		"params": ["0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"],
		"id": 3
	}"#;

	let res_new_acc = r#"{"jsonrpc":"2.0","result":"0x0","id":3}"#.to_owned();
	assert_eq!(tester.handler.handle_request_sync(req_new_acc).unwrap(), res_new_acc);
}

#[test]
fn eth_get_proof() {
	let chain = extract_chain!("BlockchainTests/bcWalletTest/wallet2outOf3txs");
	let tester = EthTester::from_chain(&chain);
	// final account state
	let req_latest = r#"{
		"jsonrpc": "2.0",
		"method": "eth_getProof",
		"params": ["0xaaaf5374fce5edbc8e2a8697c15331677e6ebaaa", [], "latest"],
		"id": 1
	}"#;

	let res_latest = r#","address":"0xaaaf5374fce5edbc8e2a8697c15331677e6ebaaa","balance":"0x9","codeHash":"0xc5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470","nonce":"0x0","storageHash":"0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421","storageProof":[]},"id":1}"#.to_owned();
    assert!(tester.handler.handle_request_sync(req_latest).unwrap().to_string().ends_with(res_latest.as_str()));

	// non-existant account
	let req_new_acc = r#"{
		"jsonrpc": "2.0",
		"method": "eth_getProof",
		"params": ["0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",[],"latest"],
		"id": 3
	}"#;

	let res_new_acc = r#","address":"0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","balance":"0x0","codeHash":"0xc5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470","nonce":"0x0","storageHash":"0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421","storageProof":[]},"id":3}"#.to_owned();
    assert!(tester.handler.handle_request_sync(req_new_acc).unwrap().to_string().ends_with(res_new_acc.as_str()));
}

#[test]
fn eth_block_number() {
	let chain = extract_chain!("BlockchainTests/bcGasPricerTest/RPC_API_Test");
	let tester = EthTester::from_chain(&chain);
	let req_number = r#"{
		"jsonrpc": "2.0",
		"method": "eth_blockNumber",
		"params": [],
		"id": 1
	}"#;

	let res_number = r#"{"jsonrpc":"2.0","result":"0x20","id":1}"#.to_owned();
	assert_eq!(tester.handler.handle_request_sync(req_number).unwrap(), res_number);
}

#[test]
fn eth_get_block() {
	let chain = extract_chain!("BlockchainTests/bcGasPricerTest/RPC_API_Test");
	let tester = EthTester::from_chain(&chain);
	let req_block = r#"{"method":"eth_getBlockByNumber","params":["0x0",false],"id":1,"jsonrpc":"2.0"}"#;

	let res_block = r#"{"jsonrpc":"2.0","result":{"author":"0x8888f1f195afa192cfee860698584c030f4c9db1","difficulty":"0x20000","extraData":"0x42","gasLimit":"0x1df5d44","gasUsed":"0x0","hash":"0xcded1bc807465a72e2d54697076ab858f28b15d4beaae8faa47339c8eee386a3","logsBloom":"0x00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000","miner":"0x8888f1f195afa192cfee860698584c030f4c9db1","mixHash":"0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421","nonce":"0x0102030405060708","number":"0x0","parentHash":"0x0000000000000000000000000000000000000000000000000000000000000000","receiptsRoot":"0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421","sealFields":["0xa056e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421","0x880102030405060708"],"sha3Uncles":"0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347","size":"0x200","stateRoot":"0x7dba07d6b448a186e9612e5f737d1c909dce473e53199901a302c00646d523c1","timestamp":"0x54c98c81","totalDifficulty":"0x20000","transactions":[],"transactionsRoot":"0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421","uncles":[]},"id":1}"#;
	assert_eq!(tester.handler.handle_request_sync(req_block).unwrap(), res_block);
}

#[test]
fn eth_get_block_by_hash() {
	let chain = extract_chain!("BlockchainTests/bcGasPricerTest/RPC_API_Test");
	let tester = EthTester::from_chain(&chain);

	// We're looking for block number 4 from "RPC_API_Test_Frontier"
	let req_block = r#"{"method":"eth_getBlockByHash","params":["0xaddb9e39795e9e041c936b88a2577802569f34afded0948707b074caa3163a87",false],"id":1,"jsonrpc":"2.0"}"#;

	let res_block = r#"{"jsonrpc":"2.0","result":{"author":"0x8888f1f195afa192cfee860698584c030f4c9db1","difficulty":"0x20080","extraData":"0x","gasLimit":"0x1dd7ea0","gasUsed":"0x5458","hash":"0xaddb9e39795e9e041c936b88a2577802569f34afded0948707b074caa3163a87","logsBloom":"0x00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000","miner":"0x8888f1f195afa192cfee860698584c030f4c9db1","mixHash":"0x713b0b31f6e72d8cb7367eaf59447ea531f209fc80e6379edd9f8d3bb73931c4","nonce":"0x4534b406bc23b86d","number":"0x4","parentHash":"0x17567aa5995b703736e32972289d68af50543acc4d56d37e8ad1fea7252cac4a","receiptsRoot":"0x7ed8026cf72ed0e98e6fd53ab406e51ffd34397d9da0052494ff41376fda7b5f","sealFields":["0xa0713b0b31f6e72d8cb7367eaf59447ea531f209fc80e6379edd9f8d3bb73931c4","0x884534b406bc23b86d"],"sha3Uncles":"0xe588a44b3e320e72e70b32b531f3ac0d432e756120135ae8fe5fa10895196b40","size":"0x661","stateRoot":"0x68805721294e365020aca15ed56c360d9dc2cf03cbeff84c9b84b8aed023bfb5","timestamp":"0x5bbdf772","totalDifficulty":"0xa00c0","transactions":["0xb094b9dc356dbb8b256402c6d5709288066ad6a372c90c9c516f14277545fd58"],"transactionsRoot":"0x97a593d8d7e15b57f5c6bb25bc6c325463ef99f874bc08a78656c3ab5cb23262","uncles":["0x86b48f5186c4b0882d3dca7977aa37840008832ef092f8ef797019dc74bfa8c7","0x2da9d062c11d536f0f1cc2a4e0111597c79926958d0fc26ae1a2d07d1a3bf47d"]},"id":1}"#;
	assert_eq!(tester.handler.handle_request_sync(req_block).unwrap(), res_block);
}

// a frontier-like test with an expanded gas limit and balance on known account.
const TRANSACTION_COUNT_SPEC: &'static [u8] = br#"{
	"name": "Frontier (Test)",
	"engine": {
		"Ethash": {
			"params": {
				"minimumDifficulty": "0x020000",
				"difficultyBoundDivisor": "0x0800",
				"blockReward": "0x4563918244F40000",
				"durationLimit": "0x0d",
				"homesteadTransition": "0xffffffffffffffff",
				"daoHardforkTransition": "0xffffffffffffffff",
				"daoHardforkBeneficiary": "0x0000000000000000000000000000000000000000",
				"daoHardforkAccounts": []
			}
		}
	},
	"params": {
		"gasLimitBoundDivisor": "0x0400",
		"registrar" : "0xc6d9d2cd449a754c494264e1809c50e34d64562b",
		"accountStartNonce": "0x00",
		"maximumExtraDataSize": "0x20",
		"minGasLimit": "0x50000",
		"networkID" : "0x1"
	},
	"genesis": {
		"seal": {
			"ethereum": {
				"nonce": "0x0000000000000042",
				"mixHash": "0x0000000000000000000000000000000000000000000000000000000000000000"
			}
		},
		"difficulty": "0x400000000",
		"author": "0x0000000000000000000000000000000000000000",
		"timestamp": "0x00",
		"parentHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
		"extraData": "0x11bbe8db4e347b4e8c937c1c8370e4b5ed33adb3db69cbdb7a38e1e50b1b82fa",
		"gasLimit": "0x50000"
	},
	"accounts": {
		"0000000000000000000000000000000000000001": { "builtin": { "name": "ecrecover", "pricing": { "linear": { "base": 3000, "word": 0 } } } },
		"0000000000000000000000000000000000000002": { "builtin": { "name": "sha256", "pricing": { "linear": { "base": 60, "word": 12 } } } },
		"0000000000000000000000000000000000000003": { "builtin": { "name": "ripemd160", "pricing": { "linear": { "base": 600, "word": 120 } } } },
		"0000000000000000000000000000000000000004": { "builtin": { "name": "identity", "pricing": { "linear": { "base": 15, "word": 3 } } } },
		"faa34835af5c2ea724333018a515fbb7d5bc0b33": { "balance": "10000000000000", "nonce": "0" }
	}
}
"#;

const POSITIVE_NONCE_SPEC: &'static [u8] = br#"{
	"name": "Frontier (Test)",
	"engine": {
		"Ethash": {
			"params": {
				"minimumDifficulty": "0x020000",
				"difficultyBoundDivisor": "0x0800",
				"blockReward": "0x4563918244F40000",
				"durationLimit": "0x0d",
				"homesteadTransition": "0xffffffffffffffff",
				"daoHardforkTransition": "0xffffffffffffffff",
				"daoHardforkBeneficiary": "0x0000000000000000000000000000000000000000",
				"daoHardforkAccounts": []
			}
		}
	},
	"params": {
		"gasLimitBoundDivisor": "0x0400",
		"registrar" : "0xc6d9d2cd449a754c494264e1809c50e34d64562b",
		"accountStartNonce": "0x0100",
		"maximumExtraDataSize": "0x20",
		"minGasLimit": "0x50000",
		"networkID" : "0x1"
	},
	"genesis": {
		"seal": {
			"ethereum": {
				"nonce": "0x0000000000000042",
				"mixHash": "0x0000000000000000000000000000000000000000000000000000000000000000"
			}
		},
		"difficulty": "0x400000000",
		"author": "0x0000000000000000000000000000000000000000",
		"timestamp": "0x00",
		"parentHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
		"extraData": "0x11bbe8db4e347b4e8c937c1c8370e4b5ed33adb3db69cbdb7a38e1e50b1b82fa",
		"gasLimit": "0x50000"
	},
	"accounts": {
		"0000000000000000000000000000000000000001": { "builtin": { "name": "ecrecover", "pricing": { "linear": { "base": 3000, "word": 0 } } } },
		"0000000000000000000000000000000000000002": { "builtin": { "name": "sha256", "pricing": { "linear": { "base": 60, "word": 12 } } } },
		"0000000000000000000000000000000000000003": { "builtin": { "name": "ripemd160", "pricing": { "linear": { "base": 600, "word": 120 } } } },
		"0000000000000000000000000000000000000004": { "builtin": { "name": "identity", "pricing": { "linear": { "base": 15, "word": 3 } } } },
		"faa34835af5c2ea724333018a515fbb7d5bc0b33": { "balance": "10000000000000", "nonce": "0" }
	}
}
"#;

#[test]
fn eth_transaction_count() {
	let secret = "8a283037bb19c4fed7b1c569e40c7dcff366165eb869110a1b11532963eb9cb2".parse().unwrap();
	let tester = EthTester::from_spec(Spec::load(&env::temp_dir(), TRANSACTION_COUNT_SPEC).expect("invalid chain spec"));
	let address = tester.accounts.insert_account(secret, &"".into()).unwrap();
	tester.accounts.unlock_account_permanently(address, "".into()).unwrap();

	let req_before = r#"{
		"jsonrpc": "2.0",
		"method": "eth_getTransactionCount",
		"params": [""#.to_owned() + format!("0x{:x}", address).as_ref() + r#"", "latest"],
		"id": 15
	}"#;

	let res_before = r#"{"jsonrpc":"2.0","result":"0x0","id":15}"#;

	assert_eq!(tester.handler.handle_request_sync(&req_before).unwrap(), res_before);

	let req_send_trans = r#"{
		"jsonrpc": "2.0",
		"method": "eth_sendTransaction",
		"params": [{
			"from": ""#.to_owned() + format!("0x{:x}", address).as_ref() + r#"",
			"to": "0xd46e8dd67c5d32be8058bb8eb970870f07244567",
			"gas": "0x30000",
			"gasPrice": "0x1",
			"value": "0x9184e72a"
		}],
		"id": 16
	}"#;

	// dispatch the transaction.
	tester.handler.handle_request_sync(&req_send_trans).unwrap();

	// we have submitted the transaction -- but this shouldn't be reflected in a "latest" query.
	let req_after_latest = r#"{
		"jsonrpc": "2.0",
		"method": "eth_getTransactionCount",
		"params": [""#.to_owned() + format!("0x{:x}", address).as_ref() + r#"", "latest"],
		"id": 17
	}"#;

	let res_after_latest = r#"{"jsonrpc":"2.0","result":"0x0","id":17}"#;

	assert_eq!(&tester.handler.handle_request_sync(&req_after_latest).unwrap(), res_after_latest);

	// the pending transactions should have been updated.
	let req_after_pending = r#"{
		"jsonrpc": "2.0",
		"method": "eth_getTransactionCount",
		"params": [""#.to_owned() + format!("0x{:x}", address).as_ref() + r#"", "pending"],
		"id": 18
	}"#;

	let res_after_pending = r#"{"jsonrpc":"2.0","result":"0x1","id":18}"#;

	assert_eq!(&tester.handler.handle_request_sync(&req_after_pending).unwrap(), res_after_pending);
}

fn verify_transaction_counts(name: String, chain: BlockChain) {
	struct PanicHandler(String);
	impl Drop for PanicHandler {
		fn drop(&mut self) {
			if ::std::thread::panicking() {
				println!("Test failed: {}", self.0);
			}
		}
	}

	let _panic = PanicHandler(name);

	fn by_hash(hash: H256, count: usize, id: &mut usize) -> (String, String) {
		let req = r#"{
			"jsonrpc": "2.0",
			"method": "eth_getBlockTransactionCountByHash",
			"params": [
				""#.to_owned() + format!("0x{:x}", hash).as_ref() + r#""
			],
			"id": "# + format!("{}", *id).as_ref() + r#"
		}"#;

		let res = r#"{"jsonrpc":"2.0","result":""#.to_owned()
			+ format!("0x{:x}", count).as_ref()
			+ r#"","id":"#
			+ format!("{}", *id).as_ref() + r#"}"#;
		*id += 1;
		(req, res)
	}

	fn by_number(num: u64, count: usize, id: &mut usize) -> (String, String) {
		let req = r#"{
			"jsonrpc": "2.0",
			"method": "eth_getBlockTransactionCountByNumber",
			"params": [
				"#.to_owned() + &::serde_json::to_string(&U256::from(num)).unwrap() + r#"
			],
			"id": "# + format!("{}", *id).as_ref() + r#"
		}"#;

		let res = r#"{"jsonrpc":"2.0","result":""#.to_owned()
			+ format!("0x{:x}", count).as_ref()
			+ r#"","id":"#
			+ format!("{}", *id).as_ref() + r#"}"#;
		*id += 1;
		(req, res)
	}

	let tester = EthTester::from_chain(&chain);

	let mut id = 1;
	for b in chain.blocks_rlp().into_iter().filter_map(|b| Unverified::from_rlp(b).ok()) {
		let count = b.transactions.len();

		let hash = b.header.hash();
		let number = b.header.number();

		let (req, res) = by_hash(hash, count, &mut id);
		assert_eq!(tester.handler.handle_request_sync(&req), Some(res));

		// uncles can share block numbers, so skip them.
		if tester.client.block_hash(BlockId::Number(number)) == Some(hash) {
			let (req, res) = by_number(number, count, &mut id);
			assert_eq!(tester.handler.handle_request_sync(&req), Some(res));
		}
	}
}

#[test]
fn starting_nonce_test() {
	let tester = EthTester::from_spec(Spec::load(&env::temp_dir(), POSITIVE_NONCE_SPEC).expect("invalid chain spec"));
	let address = Address::from(10);

	let sample = tester.handler.handle_request_sync(&(r#"
		{
			"jsonrpc": "2.0",
			"method": "eth_getTransactionCount",
			"params": [""#.to_owned() + format!("0x{:x}", address).as_ref() + r#"", "latest"],
			"id": 15
		}
		"#)
	).unwrap();

	assert_eq!(r#"{"jsonrpc":"2.0","result":"0x100","id":15}"#, &sample);
}

register_test!(eth_transaction_count_1, verify_transaction_counts, "BlockchainTests/bcWalletTest/wallet2outOf3txs");
register_test!(eth_transaction_count_2, verify_transaction_counts, "BlockchainTests/bcTotalDifficultyTest/sideChainWithMoreTransactions");
register_test!(eth_transaction_count_3, verify_transaction_counts, "BlockchainTests/bcGasPricerTest/RPC_API_Test");
