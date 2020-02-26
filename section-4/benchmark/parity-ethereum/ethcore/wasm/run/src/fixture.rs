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

use std::borrow::Cow;
use ethjson::uint::Uint;
use ethjson::hash::{Address, H256};
use ethjson::bytes::Bytes;

#[derive(Deserialize)]
#[serde(untagged)]
pub enum Source {
	Raw(Cow<'static, String>),
	Constructor {
		#[serde(rename = "constructor")]
		source: Cow<'static, String>,
		arguments: Bytes,
		sender: Address,
		at: Address,
	},
}

impl Source {
	pub fn as_ref(&self) -> &str {
		match *self {
			Source::Raw(ref r) => r.as_ref(),
			Source::Constructor { ref source, .. } => source.as_ref(),
		}
	}
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fixture {
	pub caption: Cow<'static, String>,
	pub source: Source,
	pub address: Option<Address>,
	pub sender: Option<Address>,
	pub value: Option<Uint>,
	pub gas_limit: Option<u64>,
	pub payload: Option<Bytes>,
	pub storage: Option<Vec<StorageEntry>>,
	pub asserts: Vec<Assert>,
}

#[derive(Deserialize, Debug)]
pub struct StorageEntry {
	pub key: Uint,
	pub value: Uint,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CallLocator {
	pub sender: Option<Address>,
	pub receiver: Option<Address>,
	pub value: Option<Uint>,
	pub data: Option<Bytes>,
	pub code_address: Option<Address>,
}

#[derive(Deserialize, Debug)]
pub struct StorageAssert {
	pub key: H256,
	pub value: H256,
}

#[derive(Deserialize, Debug)]
pub enum Assert {
	HasCall(CallLocator),
	HasStorage(StorageAssert),
	UsedGas(u64),
	Return(Bytes),
}
