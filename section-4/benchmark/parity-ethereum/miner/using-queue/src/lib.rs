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

//! Queue-like datastructure including notion of usage.

/// Special queue-like datastructure that includes the notion of
/// usage to avoid items that were queued but never used from making it into
/// the queue.
pub struct UsingQueue<T> {
	/// Not yet being sealed by a miner, but if one asks for work, we'd prefer they do this.
	pending: Option<T>,
	/// Currently being sealed by miners.
	in_use: Vec<T>,
	/// The maximum allowable number of items in_use.
	max_size: usize,
}

/// Take an item or just clone it?
pub enum GetAction {
	/// Remove the item, faster but you can't get it back.
	Take,
	/// Clone the item, slower but you can get it again.
	Clone,
}

impl<T> UsingQueue<T> {
	/// Create a new struct with a maximum size of `max_size`.
	pub fn new(max_size: usize) -> UsingQueue<T> {
		UsingQueue {
			pending: None,
			in_use: vec![],
			max_size: max_size,
		}
	}

	/// Return a reference to the item at the top of the queue (or `None` if the queue is empty);
	/// it doesn't constitute noting that the item is used.
	pub fn peek_last_ref(&self) -> Option<&T> {
		self.pending.as_ref().or(self.in_use.last())
	}

	/// Return a reference to the item at the top of the queue (or `None` if the queue is empty);
	/// this constitutes using the item and will remain in the queue for at least another
	/// `max_size` invocations of `set_pending() + use_last_ref()`.
	pub fn use_last_ref(&mut self) -> Option<&T> {
		if let Some(x) = self.pending.take() {
			self.in_use.push(x);
			if self.in_use.len() > self.max_size {
				self.in_use.remove(0);
			}
		}
		self.in_use.last()
	}

	/// Place an item on the end of the queue. The previously pending item will be removed
	/// if `use_last_ref()` since it was set.
	pub fn set_pending(&mut self, b: T) {
		self.pending = Some(b);
	}

	/// Is there anything in the queue currently?
	pub fn is_in_use(&self) -> bool { self.in_use.len() > 0 }

	/// Clears everything; the queue is entirely reset.
	pub fn reset(&mut self) {
		self.pending = None;
		self.in_use.clear();
	}

	/// Returns `Some` item which is the first that `f` returns `true` with a reference to it
	/// as a parameter or `None` if no such item exists in the queue.
	fn take_used_if<P>(&mut self, predicate: P) -> Option<T> where P: Fn(&T) -> bool {
		self.in_use.iter().position(|r| predicate(r)).map(|i| self.in_use.remove(i))
	}

	/// Returns `Some` item which is the first that `f` returns `true` with a reference to it
	/// as a parameter or `None` if no such item exists in the queue.
	fn clone_used_if<P>(&mut self, predicate: P) -> Option<T> where P: Fn(&T) -> bool, T: Clone {
		self.in_use.iter().find(|r| predicate(r)).cloned()
	}

	/// Fork-function for `take_used_if` and `clone_used_if`.
	pub fn get_used_if<P>(&mut self, action: GetAction, predicate: P) -> Option<T> where P: Fn(&T) -> bool, T: Clone {
		match action {
			GetAction::Take => self.take_used_if(predicate),
			GetAction::Clone => self.clone_used_if(predicate),
		}
	}

	/// Returns a clone of the pending block if `f` returns `true` with a reference to it as
	/// a parameter, otherwise `None`.
	///
	/// If pending block is not available will clone the first of the used blocks that match the predicate.
	pub fn get_pending_if<P>(&mut self, predicate: P) -> Option<T> where P: Fn(&T) -> bool, T: Clone {
		// a bit clumsy - TODO: think about a nicer way of expressing this.
		if let Some(ref x) = self.pending {
			if predicate(x) {
				Some(x.clone())
			} else {
				None
			}
		} else {
			self.in_use.last().into_iter().filter(|x| predicate(x)).next().cloned()
		}
	}
}

#[test]
fn should_not_find_when_pushed() {
	let mut q = UsingQueue::new(2);
	q.set_pending(1);
	assert!(q.take_used_if(|i| i == &1).is_none());
}

#[test]
fn should_not_find_when_pushed_with_clone() {
	let mut q = UsingQueue::new(2);
	q.set_pending(1);
	assert!(q.clone_used_if(|i| i == &1).is_none());
}

#[test]
fn should_find_when_pushed_and_used() {
	let mut q = UsingQueue::new(2);
	q.set_pending(1);
	q.use_last_ref();
	assert!(q.take_used_if(|i| i == &1).unwrap() == 1);
}

#[test]
fn should_have_same_semantics_for_get_take_clone() {
	let mut q = UsingQueue::new(2);
	q.set_pending(1);
	assert!(q.get_used_if(GetAction::Clone, |i| i == &1).is_none());
	assert!(q.get_used_if(GetAction::Take, |i| i == &1).is_none());
	q.use_last_ref();
	assert!(q.get_used_if(GetAction::Clone, |i| i == &1).unwrap() == 1);
	assert!(q.get_used_if(GetAction::Clone, |i| i == &1).unwrap() == 1);
	assert!(q.get_used_if(GetAction::Take, |i| i == &1).unwrap() == 1);
	assert!(q.get_used_if(GetAction::Clone, |i| i == &1).is_none());
	assert!(q.get_used_if(GetAction::Take, |i| i == &1).is_none());
}

#[test]
fn should_find_when_pushed_and_used_with_clone() {
	let mut q = UsingQueue::new(2);
	q.set_pending(1);
	q.use_last_ref();
	assert!(q.clone_used_if(|i| i == &1).unwrap() == 1);
}

#[test]
fn should_not_find_again_when_pushed_and_taken() {
	let mut q = UsingQueue::new(2);
	q.set_pending(1);
	q.use_last_ref();
	assert!(q.take_used_if(|i| i == &1).unwrap() == 1);
	assert!(q.clone_used_if(|i| i == &1).is_none());
}

#[test]
fn should_find_again_when_pushed_and_cloned() {
	let mut q = UsingQueue::new(2);
	q.set_pending(1);
	q.use_last_ref();
	assert!(q.clone_used_if(|i| i == &1).unwrap() == 1);
	assert!(q.clone_used_if(|i| i == &1).unwrap() == 1);
	assert!(q.take_used_if(|i| i == &1).unwrap() == 1);
}

#[test]
fn should_find_when_others_used() {
	let mut q = UsingQueue::new(2);
	q.set_pending(1);
	q.use_last_ref();
	q.set_pending(2);
	q.use_last_ref();
	assert!(q.take_used_if(|i| i == &1).is_some());
}

#[test]
fn should_not_find_when_too_many_used() {
	let mut q = UsingQueue::new(1);
	q.set_pending(1);
	q.use_last_ref();
	q.set_pending(2);
	q.use_last_ref();
	assert!(q.take_used_if(|i| i == &1).is_none());
}

#[test]
fn should_not_find_when_not_used_and_then_pushed() {
	let mut q = UsingQueue::new(3);
	q.set_pending(1);
	q.set_pending(2);
	q.use_last_ref();
	assert!(q.take_used_if(|i| i == &1).is_none());
}

#[test]
fn should_peek_correctly_after_push() {
	let mut q = UsingQueue::new(3);
	q.set_pending(1);
	assert_eq!(q.peek_last_ref(), Some(&1));
	q.set_pending(2);
	assert_eq!(q.peek_last_ref(), Some(&2));
}

#[test]
fn should_inspect_correctly() {
	let mut q = UsingQueue::new(3);
	q.set_pending(1);
	assert_eq!(q.use_last_ref(), Some(&1));
	assert_eq!(q.peek_last_ref(), Some(&1));
	q.set_pending(2);
	assert_eq!(q.use_last_ref(), Some(&2));
	assert_eq!(q.peek_last_ref(), Some(&2));
}

#[test]
fn should_not_find_when_not_used_peeked_and_then_pushed() {
	let mut q = UsingQueue::new(3);
	q.set_pending(1);
	q.peek_last_ref();
	q.set_pending(2);
	q.use_last_ref();
	assert!(q.take_used_if(|i| i == &1).is_none());
}

#[test]
fn should_pop_used() {
	let mut q = UsingQueue::new(3);
	q.set_pending(1);
	q.use_last_ref();
	let popped = q.get_pending_if(|i| i == &1);
	assert_eq!(popped, Some(1));
}

#[test]
fn should_not_pop_last_pending() {
	let mut q = UsingQueue::new(3);
	q.set_pending(1);
	assert_eq!(q.get_pending_if(|i| i == &1), Some(1));
	assert_eq!(q.get_pending_if(|i| i == &1), Some(1));
}

#[test]
fn should_not_pop_unused_before_used() {
	let mut q = UsingQueue::new(3);
	q.set_pending(1);
	q.set_pending(2);
	let popped = q.get_pending_if(|i| i == &1);
	assert_eq!(popped, None);
}

#[test]
fn should_not_remove_used_popped() {
	let mut q = UsingQueue::new(3);
	q.set_pending(1);
	q.use_last_ref();
	assert_eq!(q.get_pending_if(|i| i == &1), Some(1));
	assert_eq!(q.get_pending_if(|i| i == &1), Some(1));
}
