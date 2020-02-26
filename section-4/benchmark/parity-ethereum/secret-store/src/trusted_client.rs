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

use std::sync::{Arc, Weak};
use bytes::Bytes;
use call_contract::RegistryInfo;
use common_types::transaction::{Transaction, SignedTransaction, Action};
use ethereum_types::Address;
use ethcore::client::{Client, BlockChainClient, ChainInfo, Nonce, BlockId};
use ethcore::miner::{Miner, MinerService};
use sync::SyncProvider;
use helpers::{get_confirmed_block_hash, REQUEST_CONFIRMATIONS_REQUIRED};
use {Error, NodeKeyPair, ContractAddress};

#[derive(Clone)]
/// 'Trusted' client weak reference.
pub struct TrustedClient {
	/// This key server node key pair.
	self_key_pair: Arc<NodeKeyPair>,
	/// Blockchain client.
	client: Weak<Client>,
	/// Sync provider.
	sync: Weak<SyncProvider>,
	/// Miner service.
	miner: Weak<Miner>,
}

impl TrustedClient {
	/// Create new trusted client.
	pub fn new(self_key_pair: Arc<NodeKeyPair>, client: Arc<Client>, sync: Arc<SyncProvider>, miner: Arc<Miner>) -> Self {
		TrustedClient {
			self_key_pair: self_key_pair,
			client: Arc::downgrade(&client),
			sync: Arc::downgrade(&sync),
			miner: Arc::downgrade(&miner),
		}
	}

	/// Get 'trusted' `Client` reference only if it is synchronized && trusted.
	pub fn get(&self) -> Option<Arc<Client>> {
		self.client.upgrade()
			.and_then(|client| self.sync.upgrade().map(|sync| (client, sync)))
			.and_then(|(client, sync)| {
				let is_synced = !sync.status().is_syncing(client.queue_info());
				let is_trusted = client.chain_info().security_level().is_full();
				match is_synced && is_trusted {
					true => Some(client),
					false => None,
				}
			})
	}

	/// Get untrusted `Client` reference.
	pub fn get_untrusted(&self) -> Option<Arc<Client>> {
		self.client.upgrade()
	}

	/// Transact contract.
	pub fn transact_contract(&self, contract: Address, tx_data: Bytes) -> Result<(), Error> {
		let client = self.client.upgrade().ok_or_else(|| Error::Internal("cannot submit tx when client is offline".into()))?;
		let miner = self.miner.upgrade().ok_or_else(|| Error::Internal("cannot submit tx when miner is offline".into()))?;
		let engine = client.engine();
		let transaction = Transaction {
			nonce: client.latest_nonce(&self.self_key_pair.address()),
			action: Action::Call(contract),
			gas: miner.authoring_params().gas_range_target.0,
			gas_price: miner.sensible_gas_price(),
			value: Default::default(),
			data: tx_data,
		};
		let chain_id = engine.signing_chain_id(&client.latest_env_info());
		let signature = self.self_key_pair.sign(&transaction.hash(chain_id))?;
		let signed = SignedTransaction::new(transaction.with_signature(signature, chain_id))?;
		miner.import_own_transaction(&*client, signed.into())
			.map_err(|e| Error::Internal(format!("failed to import tx: {}", e)))
	}

	/// Read contract address. If address source is registry, address only returned if current client state is
	/// trusted. Address from registry is read from registry from block latest block with
	/// REQUEST_CONFIRMATIONS_REQUIRED confirmations.
	pub fn read_contract_address(&self, registry_name: String, address: &ContractAddress) -> Option<Address> {
		match *address {
			ContractAddress::Address(ref address) => Some(address.clone()),
			ContractAddress::Registry => self.get().and_then(|client|
				get_confirmed_block_hash(&*client, REQUEST_CONFIRMATIONS_REQUIRED)
					.and_then(|block| client.registry_address(registry_name, BlockId::Hash(block)))
			),
		}
	}
}
