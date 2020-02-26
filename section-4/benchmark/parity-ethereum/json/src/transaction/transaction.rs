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

//! Transaction test transaction deserialization.

use uint::Uint;
use bytes::Bytes;
use hash::Address;
use maybe::MaybeEmpty;

/// Transaction test transaction deserialization.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transaction {
	/// Transaction data.
	pub data: Bytes,
	/// Gas limit.
	pub gas_limit: Uint,
	/// Gas price.
	pub gas_price: Uint,
	/// Nonce.
	pub nonce: Uint,
	/// To.
	pub to: MaybeEmpty<Address>,
	/// Value.
	pub value: Uint,
	/// R.
	pub r: Uint,
	/// S.
	pub s: Uint,
	/// V.
	pub v: Uint,
}

#[cfg(test)]
mod tests {
	use serde_json;
	use transaction::Transaction;

	#[test]
	fn transaction_deserialization() {
		let s = r#"{
			"data" : "0x",
			"gasLimit" : "0xf388",
			"gasPrice" : "0x09184e72a000",
			"nonce" : "0x00",
			"r" : "0x2c",
			"s" : "0x04",
			"to" : "",
			"v" : "0x1b",
			"value" : "0x00"
		}"#;
		let _deserialized: Transaction = serde_json::from_str(s).unwrap();
		// TODO: validate all fields
	}
}
