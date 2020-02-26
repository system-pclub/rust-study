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

//! Encryption providers.

use std::io::Read;
use std::str::FromStr;
use std::sync::Arc;
use std::iter::repeat;
use std::time::{Instant, Duration};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use parking_lot::Mutex;
use ethereum_types::{H128, H256, Address};
use ethjson;
use ethkey::{Signature, Public};
use crypto;
use futures::Future;
use fetch::{Fetch, Client as FetchClient, Method, BodyReader, Request};
use bytes::{Bytes, ToPretty};
use error::Error;
use url::Url;
use super::Signer;
use super::key_server_keys::address_to_key;

/// Initialization vector length.
const INIT_VEC_LEN: usize = 16;

/// Duration of storing retrieved keys (in ms)
const ENCRYPTION_SESSION_DURATION: u64 = 30 * 1000;

/// Trait for encryption/decryption operations.
pub trait Encryptor: Send + Sync + 'static {
	/// Generate unique contract key && encrypt passed data. Encryption can only be performed once.
	fn encrypt(
		&self,
		contract_address: &Address,
		initialisation_vector: &H128,
		plain_data: &[u8],
	) -> Result<Bytes, Error>;

	/// Decrypt data using previously generated contract key.
	fn decrypt(
		&self,
		contract_address: &Address,
		cypher: &[u8],
	) -> Result<Bytes, Error>;
}

/// Configurtion for key server encryptor
#[derive(Default, PartialEq, Debug, Clone)]
pub struct EncryptorConfig {
	/// URL to key server
	pub base_url: Option<String>,
	/// Key server's threshold
	pub threshold: u32,
	/// Account used for signing requests to key server
	pub key_server_account: Option<Address>,
}

struct EncryptionSession {
	key: Bytes,
	end_time: Instant,
}

/// SecretStore-based encryption/decryption operations.
pub struct SecretStoreEncryptor {
	config: EncryptorConfig,
	client: FetchClient,
	sessions: Mutex<HashMap<Address, EncryptionSession>>,
	signer: Arc<Signer>,
}

impl SecretStoreEncryptor {
	/// Create new encryptor
	pub fn new(
		config: EncryptorConfig,
		client: FetchClient,
		signer: Arc<Signer>,
	) -> Result<Self, Error> {
		Ok(SecretStoreEncryptor {
			config,
			client,
			signer,
			sessions: Mutex::default(),
		})
	}

	/// Ask secret store for key && decrypt the key.
	fn retrieve_key(
		&self,
		url_suffix: &str,
		use_post: bool,
		contract_address: &Address,
	) -> Result<Bytes, Error> {
		// check if the key was already cached
		if let Some(key) = self.obtained_key(contract_address) {
			return Ok(key);
		}
		let contract_address_signature = self.sign_contract_address(contract_address)?;
		let requester = self.config.key_server_account.ok_or_else(|| Error::KeyServerAccountNotSet)?;

		// key id in SS is H256 && we have H160 here => expand with assitional zeros
		let contract_address_extended: H256 = contract_address.into();
		let base_url = self.config.base_url.clone().ok_or_else(|| Error::KeyServerNotSet)?;

		// prepare request url
		let url = format!("{}/{}/{}{}",
				base_url,
				contract_address_extended.to_hex(),
				contract_address_signature,
				url_suffix,
			);

		// send HTTP request
		let method = if use_post {
			Method::POST
		} else {
			Method::GET
		};

		let url = Url::from_str(&url).map_err(|e| Error::Encrypt(e.to_string()))?;
		let response = self.client.fetch(Request::new(url, method), Default::default()).wait()
			.map_err(|e| Error::Encrypt(e.to_string()))?;

		if response.is_not_found() {
			return Err(Error::EncryptionKeyNotFound(*contract_address));
		}

		if !response.is_success() {
			return Err(Error::Encrypt(response.status().canonical_reason().unwrap_or("unknown").into()));
		}

		// read HTTP response
		let mut result = String::new();
		BodyReader::new(response).read_to_string(&mut result)?;

		// response is JSON string (which is, in turn, hex-encoded, encrypted Public)
		let encrypted_bytes: ethjson::bytes::Bytes = result.trim_matches('\"').parse().map_err(|e| Error::Encrypt(e))?;

		// decrypt Public
		let decrypted_bytes = self.signer.decrypt(requester, &crypto::DEFAULT_MAC, &encrypted_bytes)?;
		let decrypted_key = Public::from_slice(&decrypted_bytes);

		// and now take x coordinate of Public as a key
		let key: Bytes = (*decrypted_key)[..INIT_VEC_LEN].into();

		// cache the key in the session and clear expired sessions
		self.sessions.lock().insert(*contract_address, EncryptionSession{
			key: key.clone(),
			end_time: Instant::now() + Duration::from_millis(ENCRYPTION_SESSION_DURATION),
		});
		self.clean_expired_sessions();
		Ok(key)
	}

	fn clean_expired_sessions(&self) {
		let mut sessions = self.sessions.lock();
		sessions.retain(|_, session| session.end_time < Instant::now());
	}

	fn obtained_key(&self, contract_address: &Address) -> Option<Bytes> {
		let mut sessions = self.sessions.lock();
		let stored_session = sessions.entry(*contract_address);
		match stored_session {
			Entry::Occupied(session) => {
				if Instant::now() > session.get().end_time {
					session.remove_entry();
					None
				} else {
					Some(session.get().key.clone())
				}
			}
			Entry::Vacant(_) => None,
		}
	}

	fn sign_contract_address(&self, contract_address: &Address) -> Result<Signature, Error> {
		let key_server_account = self.config.key_server_account.ok_or_else(|| Error::KeyServerAccountNotSet)?;
		Ok(self.signer.sign(key_server_account, address_to_key(contract_address))?)
	}
}

impl Encryptor for SecretStoreEncryptor {
	fn encrypt(
		&self,
		contract_address: &Address,
		initialisation_vector: &H128,
		plain_data: &[u8],
	) -> Result<Bytes, Error> {
		// retrieve the key, try to generate it if it doesn't exist yet
		let key = match self.retrieve_key("", false, contract_address) {
			Ok(key) => Ok(key),
			Err(Error::EncryptionKeyNotFound(_)) => {
				trace!(target: "privatetx", "Key for account wasnt found in sstore. Creating. Address: {:?}", contract_address);
				self.retrieve_key(&format!("/{}", self.config.threshold), true, contract_address)
			}
			Err(err) => Err(err),
		}?;

		// encrypt data
		let mut cypher = Vec::with_capacity(plain_data.len() + initialisation_vector.len());
		cypher.extend(repeat(0).take(plain_data.len()));
		crypto::aes::encrypt_128_ctr(&key, initialisation_vector, plain_data, &mut cypher)
			.map_err(|e| Error::Encrypt(e.to_string()))?;
		cypher.extend_from_slice(&initialisation_vector);

		Ok(cypher)
	}

	/// Decrypt data using previously generated contract key.
	fn decrypt(
		&self,
		contract_address: &Address,
		cypher: &[u8],
	) -> Result<Bytes, Error> {
		// initialization vector takes INIT_VEC_LEN bytes
		let cypher_len = cypher.len();
		if cypher_len < INIT_VEC_LEN {
			return Err(Error::Decrypt("Invalid cypher".into()));
		}

		// retrieve existing key
		let key = self.retrieve_key("", false, contract_address)?;

		// use symmetric decryption to decrypt document
		let (cypher, iv) = cypher.split_at(cypher_len - INIT_VEC_LEN);
		let mut plain_data = Vec::with_capacity(cypher_len - INIT_VEC_LEN);
		plain_data.extend(repeat(0).take(cypher_len - INIT_VEC_LEN));
		crypto::aes::decrypt_128_ctr(&key, &iv, cypher, &mut plain_data)
			.map_err(|e| Error::Decrypt(e.to_string()))?;
		Ok(plain_data)
	}
}

/// Dummy encryptor.
#[derive(Default)]
pub struct NoopEncryptor;

impl Encryptor for NoopEncryptor {
	fn encrypt(
		&self,
		_contract_address: &Address,
		_initialisation_vector: &H128,
		data: &[u8],
	) -> Result<Bytes, Error> {
		Ok(data.to_vec())
	}

	fn decrypt(
		&self,
		_contract_address: &Address,
		data: &[u8],
	) -> Result<Bytes, Error> {
		Ok(data.to_vec())
	}
}
