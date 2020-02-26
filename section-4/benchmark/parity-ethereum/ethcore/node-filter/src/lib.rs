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

//! Smart contract based node filter.

extern crate ethabi;
extern crate ethcore;
extern crate ethcore_network as network;
extern crate ethcore_network_devp2p as devp2p;
extern crate ethereum_types;
extern crate lru_cache;
extern crate parking_lot;

#[macro_use]
extern crate ethabi_derive;
#[macro_use]
extern crate ethabi_contract;
#[cfg(test)]
extern crate ethcore_io as io;
#[cfg(test)]
extern crate kvdb_memorydb;
#[cfg(test)]
extern crate tempdir;
#[macro_use]
extern crate log;

use std::sync::Weak;

use ethcore::client::{BlockChainClient, BlockId};
use ethereum_types::{H256, Address};
use ethabi::FunctionOutputDecoder;
use network::{ConnectionFilter, ConnectionDirection};
use devp2p::NodeId;

use_contract!(peer_set, "res/peer_set.json");

/// Connection filter that uses a contract to manage permissions.
pub struct NodeFilter {
	client: Weak<BlockChainClient>,
	contract_address: Address,
}

impl NodeFilter {
	/// Create a new instance. Accepts a contract address.
	pub fn new(client: Weak<BlockChainClient>, contract_address: Address) -> NodeFilter {
		NodeFilter {
			client,
			contract_address,
		}
	}
}

impl ConnectionFilter for NodeFilter {
	fn connection_allowed(&self, own_id: &NodeId, connecting_id: &NodeId, _direction: ConnectionDirection) -> bool {
		let client = match self.client.upgrade() {
			Some(client) => client,
			None => return false,
		};

		let address = self.contract_address;
		let own_low = H256::from_slice(&own_id[0..32]);
		let own_high = H256::from_slice(&own_id[32..64]);
		let id_low = H256::from_slice(&connecting_id[0..32]);
		let id_high = H256::from_slice(&connecting_id[32..64]);

		let (data, decoder) = peer_set::functions::connection_allowed::call(own_low, own_high, id_low, id_high);
		let allowed = client.call_contract(BlockId::Latest, address, data)
			.and_then(|value| decoder.decode(&value).map_err(|e| e.to_string()))
			.unwrap_or_else(|e| {
				debug!("Error callling peer set contract: {:?}", e);
				false
			});

		allowed
	}
}

#[cfg(test)]
mod test {
	use std::sync::{Arc, Weak};
	use ethcore::spec::Spec;
	use ethcore::client::{BlockChainClient, Client, ClientConfig};
	use ethcore::miner::Miner;
	use ethcore::test_helpers;
	use network::{ConnectionDirection, ConnectionFilter, NodeId};
	use io::IoChannel;
	use super::NodeFilter;
	use tempdir::TempDir;

	/// Contract code: https://gist.github.com/arkpar/467dbcc73cbb85b0997a7a10ffa0695f
	#[test]
	fn node_filter() {
		let contract_addr = "0000000000000000000000000000000000000005".into();
		let data = include_bytes!("../res/node_filter.json");
		let tempdir = TempDir::new("").unwrap();
		let spec = Spec::load(&tempdir.path(), &data[..]).unwrap();
		let client_db = test_helpers::new_db();

		let client = Client::new(
			ClientConfig::default(),
			&spec,
			client_db,
			Arc::new(Miner::new_for_tests(&spec, None)),
			IoChannel::disconnected(),
		).unwrap();
		let filter = NodeFilter::new(Arc::downgrade(&client) as Weak<BlockChainClient>, contract_addr);
		let self1: NodeId = "00000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000002".into();
		let self2: NodeId = "00000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000003".into();
		let node1: NodeId = "00000000000000000000000000000000000000000000000000000000000000110000000000000000000000000000000000000000000000000000000000000012".into();
		let node2: NodeId = "00000000000000000000000000000000000000000000000000000000000000210000000000000000000000000000000000000000000000000000000000000022".into();
		let nodex: NodeId = "77000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000".into();

		assert!(filter.connection_allowed(&self1, &node1, ConnectionDirection::Inbound));
		assert!(filter.connection_allowed(&self1, &nodex, ConnectionDirection::Inbound));
		assert!(filter.connection_allowed(&self2, &node1, ConnectionDirection::Inbound));
		assert!(filter.connection_allowed(&self2, &node2, ConnectionDirection::Inbound));
	}
}
