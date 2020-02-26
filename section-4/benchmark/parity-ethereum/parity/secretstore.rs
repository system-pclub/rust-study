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

use std::collections::BTreeMap;
use std::sync::Arc;
use account_utils::AccountProvider;
use dir::default_data_path;
use dir::helpers::replace_home;
use ethcore::client::Client;
use ethcore::miner::Miner;
use ethkey::{Secret, Public, Password};
use sync::SyncProvider;
use ethereum_types::Address;
use parity_runtime::Executor;

/// This node secret key.
#[derive(Debug, PartialEq, Clone)]
pub enum NodeSecretKey {
	/// Stored as plain text in configuration file.
	Plain(Secret),
	/// Stored as account in key store.
	#[cfg(feature = "accounts")]
	KeyStore(Address),
}

/// Secret store service contract address.
#[derive(Debug, PartialEq, Clone)]
pub enum ContractAddress {
	/// Contract address is read from registry.
	Registry,
	/// Contract address is specified.
	Address(Address),
}

#[derive(Debug, PartialEq, Clone)]
/// Secret store configuration
pub struct Configuration {
	/// Is secret store functionality enabled?
	pub enabled: bool,
	/// Is HTTP API enabled?
	pub http_enabled: bool,
	/// Is auto migrate enabled.
	pub auto_migrate_enabled: bool,
	/// ACL check contract address.
	pub acl_check_contract_address: Option<ContractAddress>,
	/// Service contract address.
	pub service_contract_address: Option<ContractAddress>,
	/// Server key generation service contract address.
	pub service_contract_srv_gen_address: Option<ContractAddress>,
	/// Server key retrieval service contract address.
	pub service_contract_srv_retr_address: Option<ContractAddress>,
	/// Document key store service contract address.
	pub service_contract_doc_store_address: Option<ContractAddress>,
	/// Document key shadow retrieval service contract address.
	pub service_contract_doc_sretr_address: Option<ContractAddress>,
	/// This node secret.
	pub self_secret: Option<NodeSecretKey>,
	/// Other nodes IDs + addresses.
	pub nodes: BTreeMap<Public, (String, u16)>,
	/// Key Server Set contract address. If None, 'nodes' map is used.
	pub key_server_set_contract_address: Option<ContractAddress>,
	/// Interface to listen to
	pub interface: String,
	/// Port to listen to
	pub port: u16,
	/// Interface to listen to
	pub http_interface: String,
	/// Port to listen to
	pub http_port: u16,
	/// Data directory path for secret store
	pub data_path: String,
	/// Administrator public key.
	pub admin_public: Option<Public>,
}

/// Secret store dependencies
pub struct Dependencies<'a> {
	/// Blockchain client.
	pub client: Arc<Client>,
	/// Sync provider.
	pub sync: Arc<SyncProvider>,
	/// Miner service.
	pub miner: Arc<Miner>,
	/// Account provider.
	pub account_provider: Arc<AccountProvider>,
	/// Passed accounts passwords.
	pub accounts_passwords: &'a [Password],
}

#[cfg(not(feature = "secretstore"))]
mod server {
	use super::{Configuration, Dependencies, Executor};

	/// Noop key server implementation
	pub struct KeyServer;

	impl KeyServer {
		/// Create new noop key server
		pub fn new(_conf: Configuration, _deps: Dependencies, _executor: Executor) -> Result<Self, String> {
			Ok(KeyServer)
		}
	}
}

#[cfg(feature = "secretstore")]
mod server {
	use std::sync::Arc;
	use ethcore_secretstore;
	use ethkey::KeyPair;
	use ansi_term::Colour::{Red, White};
	use db;
	use super::{Configuration, Dependencies, NodeSecretKey, ContractAddress, Executor};

	fn into_service_contract_address(address: ContractAddress) -> ethcore_secretstore::ContractAddress {
		match address {
			ContractAddress::Registry => ethcore_secretstore::ContractAddress::Registry,
			ContractAddress::Address(address) => ethcore_secretstore::ContractAddress::Address(address),
		}
	}

	/// Key server
	pub struct KeyServer {
		_key_server: Box<ethcore_secretstore::KeyServer>,
	}

	impl KeyServer {
		/// Create new key server
		pub fn new(mut conf: Configuration, deps: Dependencies, executor: Executor) -> Result<Self, String> {
			let self_secret: Arc<ethcore_secretstore::NodeKeyPair> = match conf.self_secret.take() {
				Some(NodeSecretKey::Plain(secret)) => Arc::new(ethcore_secretstore::PlainNodeKeyPair::new(
					KeyPair::from_secret(secret).map_err(|e| format!("invalid secret: {}", e))?)),
				#[cfg(feature = "accounts")]
				Some(NodeSecretKey::KeyStore(account)) => {
					// Check if account exists
					if !deps.account_provider.has_account(account.clone()) {
						return Err(format!("Account {} passed as secret store node key is not found", account));
					}

					// Check if any passwords have been read from the password file(s)
					if deps.accounts_passwords.is_empty() {
						return Err(format!("No password found for the secret store node account {}", account));
					}

					// Attempt to sign in the engine signer.
					let password = deps.accounts_passwords.iter()
						.find(|p| deps.account_provider.sign(account.clone(), Some((*p).clone()), Default::default()).is_ok())
						.ok_or_else(|| format!("No valid password for the secret store node account {}", account))?;
					Arc::new(ethcore_secretstore::KeyStoreNodeKeyPair::new(deps.account_provider, account, password.clone())
						.map_err(|e| format!("{}", e))?)
				},
				None => return Err("self secret is required when using secretstore".into()),
			};

			info!("Starting SecretStore node: {}", White.bold().paint(format!("{:?}", self_secret.public())));
			if conf.acl_check_contract_address.is_none() {
				warn!("Running SecretStore with disabled ACL check: {}", Red.bold().paint("everyone has access to stored keys"));
			}

			let key_server_name = format!("{}:{}", conf.interface, conf.port);
			let mut cconf = ethcore_secretstore::ServiceConfiguration {
				listener_address: if conf.http_enabled { Some(ethcore_secretstore::NodeAddress {
					address: conf.http_interface.clone(),
					port: conf.http_port,
				}) } else { None },
				service_contract_address: conf.service_contract_address.map(into_service_contract_address),
				service_contract_srv_gen_address: conf.service_contract_srv_gen_address.map(into_service_contract_address),
				service_contract_srv_retr_address: conf.service_contract_srv_retr_address.map(into_service_contract_address),
				service_contract_doc_store_address: conf.service_contract_doc_store_address.map(into_service_contract_address),
				service_contract_doc_sretr_address: conf.service_contract_doc_sretr_address.map(into_service_contract_address),
				acl_check_contract_address: conf.acl_check_contract_address.map(into_service_contract_address),
				cluster_config: ethcore_secretstore::ClusterConfiguration {
					listener_address: ethcore_secretstore::NodeAddress {
						address: conf.interface.clone(),
						port: conf.port,
					},
					nodes: conf.nodes.into_iter().map(|(p, (ip, port))| (p, ethcore_secretstore::NodeAddress {
						address: ip,
						port: port,
					})).collect(),
					key_server_set_contract_address: conf.key_server_set_contract_address.map(into_service_contract_address),
					allow_connecting_to_higher_nodes: true,
					admin_public: conf.admin_public,
					auto_migrate_enabled: conf.auto_migrate_enabled,
				},
			};

			cconf.cluster_config.nodes.insert(self_secret.public().clone(), cconf.cluster_config.listener_address.clone());

			let db = db::open_secretstore_db(&conf.data_path)?;
			let key_server = ethcore_secretstore::start(deps.client, deps.sync, deps.miner, self_secret, cconf, db, executor)
				.map_err(|e| format!("Error starting KeyServer {}: {}", key_server_name, e))?;

			Ok(KeyServer {
				_key_server: key_server,
			})
		}
	}
}

pub use self::server::KeyServer;

impl Default for Configuration {
	fn default() -> Self {
		let data_dir = default_data_path();
		Configuration {
			enabled: true,
			http_enabled: true,
			auto_migrate_enabled: true,
			acl_check_contract_address: Some(ContractAddress::Registry),
			service_contract_address: None,
			service_contract_srv_gen_address: None,
			service_contract_srv_retr_address: None,
			service_contract_doc_store_address: None,
			service_contract_doc_sretr_address: None,
			self_secret: None,
			admin_public: None,
			nodes: BTreeMap::new(),
			key_server_set_contract_address: Some(ContractAddress::Registry),
			interface: "127.0.0.1".to_owned(),
			port: 8083,
			http_interface: "127.0.0.1".to_owned(),
			http_port: 8082,
			data_path: replace_home(&data_dir, "$BASE/secretstore"),
		}
	}
}

/// Start secret store-related functionality
pub fn start(conf: Configuration, deps: Dependencies, executor: Executor) -> Result<Option<KeyServer>, String> {
	if !conf.enabled {
		return Ok(None);
	}

	KeyServer::new(conf, deps, executor)
		.map(|s| Some(s))
}
