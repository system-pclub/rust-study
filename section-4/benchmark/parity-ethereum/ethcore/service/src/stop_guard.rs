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

//! Stop guard mod

use std::sync::Arc;
use std::sync::atomic::*;

/// Stop guard that will set a stop flag on drop
pub struct StopGuard {
	flag: Arc<AtomicBool>,
}

impl StopGuard {
	/// Create a stop guard
	pub fn new() -> StopGuard {
		StopGuard {
			flag: Arc::new(AtomicBool::new(false))
		}
	}
}

impl Drop for StopGuard {
	fn drop(&mut self) {
		self.flag.store(true, Ordering::Relaxed)
	}
}
