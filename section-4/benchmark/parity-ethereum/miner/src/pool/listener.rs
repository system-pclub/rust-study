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

//! Notifier for new transaction hashes.

use std::fmt;
use std::sync::Arc;

use ethereum_types::H256;
use txpool::{self, VerifiedTransaction};

use pool::VerifiedTransaction as Transaction;

type Listener = Box<Fn(&[H256]) + Send + Sync>;

/// Manages notifications to pending transaction listeners.
#[derive(Default)]
pub struct Notifier {
	listeners: Vec<Listener>,
	pending: Vec<H256>,
}

impl fmt::Debug for Notifier {
	fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
		fmt.debug_struct("Notifier")
			.field("listeners", &self.listeners.len())
			.field("pending", &self.pending)
			.finish()
	}
}

impl Notifier {
	/// Add new listener to receive notifications.
	pub fn add(&mut self, f: Listener) {
		self.listeners.push(f)
	}

	/// Notify listeners about all currently pending transactions.
	pub fn notify(&mut self) {
		if self.pending.is_empty() {
			return;
		}

		for l in &self.listeners {
			(l)(&self.pending);
		}

		self.pending.clear();
	}
}

impl txpool::Listener<Transaction> for Notifier {
	fn added(&mut self, tx: &Arc<Transaction>, _old: Option<&Arc<Transaction>>) {
		self.pending.push(*tx.hash());
	}
}

/// Transaction pool logger.
#[derive(Default, Debug)]
pub struct Logger;

impl txpool::Listener<Transaction> for Logger {
	fn added(&mut self, tx: &Arc<Transaction>, old: Option<&Arc<Transaction>>) {
		debug!(target: "txqueue", "[{:?}] Added to the pool.", tx.hash());
		debug!(
			target: "txqueue",
			"[{hash:?}] Sender: {sender}, nonce: {nonce}, gasPrice: {gas_price}, gas: {gas}, value: {value}, dataLen: {data}))",
			hash = tx.hash(),
			sender = tx.sender(),
			nonce = tx.signed().nonce,
			gas_price = tx.signed().gas_price,
			gas = tx.signed().gas,
			value = tx.signed().value,
			data = tx.signed().data.len(),
		);

		if let Some(old) = old {
			debug!(target: "txqueue", "[{:?}] Dropped. Replaced by [{:?}]", old.hash(), tx.hash());
		}
	}

	fn rejected(&mut self, _tx: &Arc<Transaction>, reason: &txpool::ErrorKind) {
		trace!(target: "txqueue", "Rejected {}.", reason);
	}

	fn dropped(&mut self, tx: &Arc<Transaction>, new: Option<&Transaction>) {
		match new {
			Some(new) => debug!(target: "txqueue", "[{:?}] Pushed out by [{:?}]", tx.hash(), new.hash()),
			None => debug!(target: "txqueue", "[{:?}] Dropped.", tx.hash()),
		}
	}

	fn invalid(&mut self, tx: &Arc<Transaction>) {
		debug!(target: "txqueue", "[{:?}] Marked as invalid by executor.", tx.hash());
	}

	fn canceled(&mut self, tx: &Arc<Transaction>) {
		debug!(target: "txqueue", "[{:?}] Canceled by the user.", tx.hash());
	}

	fn culled(&mut self, tx: &Arc<Transaction>) {
		debug!(target: "txqueue", "[{:?}] Culled or mined.", tx.hash());
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use parking_lot::Mutex;
	use types::transaction;
	use txpool::Listener;

	#[test]
	fn should_notify_listeners() {
		// given
		let received = Arc::new(Mutex::new(vec![]));
		let r = received.clone();
		let listener = Box::new(move |hashes: &[H256]| {
			*r.lock() = hashes.iter().map(|x| *x).collect();
		});

		let mut tx_listener = Notifier::default();
		tx_listener.add(listener);

		// when
		let tx = new_tx();
		tx_listener.added(&tx, None);
		assert_eq!(*received.lock(), vec![]);

		// then
		tx_listener.notify();
		assert_eq!(
			*received.lock(),
			vec!["13aff4201ac1dc49daf6a7cf07b558ed956511acbaabf9502bdacc353953766d".parse().unwrap()]
		);
	}

	fn new_tx() -> Arc<Transaction> {
		let signed = transaction::Transaction {
			action: transaction::Action::Create,
			data: vec![1, 2, 3],
			nonce: 5.into(),
			gas: 21_000.into(),
			gas_price: 5.into(),
			value: 0.into(),
		}.fake_sign(5.into());

		Arc::new(Transaction::from_pending_block_transaction(signed))
	}
}
