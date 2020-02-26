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

//! A mutable state representation suitable to execute transactions.
//! Generic over a `Backend`. Deals with `Account`s.
//! Unconfirmed sub-states are managed with `checkpoint`s which may be canonicalized
//! or rolled back.

use std::cell::{RefCell, RefMut};
use std::collections::hash_map::Entry;
use std::collections::{HashMap, BTreeMap, BTreeSet, HashSet};
use std::fmt;
use std::sync::Arc;
use hash::{KECCAK_NULL_RLP, KECCAK_EMPTY};

use types::receipt::{Receipt, TransactionOutcome};
use machine::EthereumMachine as Machine;
use vm::EnvInfo;
use error::Error;
use executive::{Executive, TransactOptions};
use factory::Factories;
use trace::{self, FlatTrace, VMTrace};
use pod_account::*;
use pod_state::{self, PodState};
use types::basic_account::BasicAccount;
use executed::{Executed, ExecutionError};
use types::state_diff::StateDiff;
use types::transaction::SignedTransaction;
use state_db::StateDB;
use factory::VmFactory;

use ethereum_types::{H256, U256, Address};
use hash_db::{HashDB, AsHashDB};
use keccak_hasher::KeccakHasher;
use kvdb::DBValue;
use bytes::Bytes;

use trie::{Trie, TrieError, Recorder};
use ethtrie::{TrieDB, Result as TrieResult};

mod account;
mod substate;

pub mod backend;

pub use self::account::Account;
pub use self::backend::Backend;
pub use self::substate::Substate;

/// Used to return information about an `State::apply` operation.
pub struct ApplyOutcome<T, V> {
	/// The receipt for the applied transaction.
	pub receipt: Receipt,
	/// The output of the applied transaction.
	pub output: Bytes,
	/// The trace for the applied transaction, empty if tracing was not produced.
	pub trace: Vec<T>,
	/// The VM trace for the applied transaction, None if tracing was not produced.
	pub vm_trace: Option<V>
}

/// Result type for the execution ("application") of a transaction.
pub type ApplyResult<T, V> = Result<ApplyOutcome<T, V>, Error>;

/// Return type of proof validity check.
#[derive(Debug, Clone)]
pub enum ProvedExecution {
	/// Proof wasn't enough to complete execution.
	BadProof,
	/// The transaction failed, but not due to a bad proof.
	Failed(ExecutionError),
	/// The transaction successfully completed with the given proof.
	Complete(Box<Executed>),
}

#[derive(Eq, PartialEq, Clone, Copy, Debug)]
/// Account modification state. Used to check if the account was
/// Modified in between commits and overall.
enum AccountState {
	/// Account was loaded from disk and never modified in this state object.
	CleanFresh,
	/// Account was loaded from the global cache and never modified.
	CleanCached,
	/// Account has been modified and is not committed to the trie yet.
	/// This is set if any of the account data is changed, including
	/// storage and code.
	Dirty,
	/// Account was modified and committed to the trie.
	Committed,
}

#[derive(Debug)]
/// In-memory copy of the account data. Holds the optional account
/// and the modification status.
/// Account entry can contain existing (`Some`) or non-existing
/// account (`None`)
struct AccountEntry {
	/// Account entry. `None` if account known to be non-existant.
	account: Option<Account>,
	/// Unmodified account balance.
	old_balance: Option<U256>,
	/// Entry state.
	state: AccountState,
}

// Account cache item. Contains account data and
// modification state
impl AccountEntry {
	fn is_dirty(&self) -> bool {
		self.state == AccountState::Dirty
	}

	fn exists_and_is_null(&self) -> bool {
		self.account.as_ref().map_or(false, |a| a.is_null())
	}

	/// Clone dirty data into new `AccountEntry`. This includes
	/// basic account data and modified storage keys.
	/// Returns None if clean.
	fn clone_if_dirty(&self) -> Option<AccountEntry> {
		match self.is_dirty() {
			true => Some(self.clone_dirty()),
			false => None,
		}
	}

	/// Clone dirty data into new `AccountEntry`. This includes
	/// basic account data and modified storage keys.
	fn clone_dirty(&self) -> AccountEntry {
		AccountEntry {
			old_balance: self.old_balance,
			account: self.account.as_ref().map(Account::clone_dirty),
			state: self.state,
		}
	}

	// Create a new account entry and mark it as dirty.
	fn new_dirty(account: Option<Account>) -> AccountEntry {
		AccountEntry {
			old_balance: account.as_ref().map(|a| a.balance().clone()),
			account: account,
			state: AccountState::Dirty,
		}
	}

	// Create a new account entry and mark it as clean.
	fn new_clean(account: Option<Account>) -> AccountEntry {
		AccountEntry {
			old_balance: account.as_ref().map(|a| a.balance().clone()),
			account: account,
			state: AccountState::CleanFresh,
		}
	}

	// Create a new account entry and mark it as clean and cached.
	fn new_clean_cached(account: Option<Account>) -> AccountEntry {
		AccountEntry {
			old_balance: account.as_ref().map(|a| a.balance().clone()),
			account: account,
			state: AccountState::CleanCached,
		}
	}

	// Replace data with another entry but preserve storage cache.
	fn overwrite_with(&mut self, other: AccountEntry) {
		self.state = other.state;
		match other.account {
			Some(acc) => {
				if let Some(ref mut ours) = self.account {
					ours.overwrite_with(acc);
				} else {
					self.account = Some(acc);
				}
			},
			None => self.account = None,
		}
	}
}

/// Check the given proof of execution.
/// `Err(ExecutionError::Internal)` indicates failure, everything else indicates
/// a successful proof (as the transaction itself may be poorly chosen).
pub fn check_proof(
	proof: &[DBValue],
	root: H256,
	transaction: &SignedTransaction,
	machine: &Machine,
	env_info: &EnvInfo,
) -> ProvedExecution {
	let backend = self::backend::ProofCheck::new(proof);
	let mut factories = Factories::default();
	factories.accountdb = ::account_db::Factory::Plain;

	let res = State::from_existing(
		backend,
		root,
		machine.account_start_nonce(env_info.number),
		factories
	);

	let mut state = match res {
		Ok(state) => state,
		Err(_) => return ProvedExecution::BadProof,
	};

	let options = TransactOptions::with_no_tracing().save_output_from_contract();
	match state.execute(env_info, machine, transaction, options, true) {
		Ok(executed) => ProvedExecution::Complete(Box::new(executed)),
		Err(ExecutionError::Internal(_)) => ProvedExecution::BadProof,
		Err(e) => ProvedExecution::Failed(e),
	}
}

/// Prove a `virtual` transaction on the given state.
/// Returns `None` when the transacion could not be proved,
/// and a proof otherwise.
pub fn prove_transaction_virtual<H: AsHashDB<KeccakHasher, DBValue> + Send + Sync>(
	db: H,
	root: H256,
	transaction: &SignedTransaction,
	machine: &Machine,
	env_info: &EnvInfo,
	factories: Factories,
) -> Option<(Bytes, Vec<DBValue>)> {
	use self::backend::Proving;

	let backend = Proving::new(db);
	let res = State::from_existing(
		backend,
		root,
		machine.account_start_nonce(env_info.number),
		factories,
	);

	let mut state = match res {
		Ok(state) => state,
		Err(_) => return None,
	};

	let options = TransactOptions::with_no_tracing().dont_check_nonce().save_output_from_contract();
	match state.execute(env_info, machine, transaction, options, true) {
		Err(ExecutionError::Internal(_)) => None,
		Err(e) => {
			trace!(target: "state", "Proved call failed: {}", e);
			Some((Vec::new(), state.drop().1.extract_proof()))
		}
		Ok(res) => Some((res.output, state.drop().1.extract_proof())),
	}
}

/// Representation of the entire state of all accounts in the system.
///
/// `State` can work together with `StateDB` to share account cache.
///
/// Local cache contains changes made locally and changes accumulated
/// locally from previous commits. Global cache reflects the database
/// state and never contains any changes.
///
/// Cache items contains account data, or the flag that account does not exist
/// and modification state (see `AccountState`)
///
/// Account data can be in the following cache states:
/// * In global but not local - something that was queried from the database,
/// but never modified
/// * In local but not global - something that was just added (e.g. new account)
/// * In both with the same value - something that was changed to a new value,
/// but changed back to a previous block in the same block (same State instance)
/// * In both with different values - something that was overwritten with a
/// new value.
///
/// All read-only state queries check local cache/modifications first,
/// then global state cache. If data is not found in any of the caches
/// it is loaded from the DB to the local cache.
///
/// **** IMPORTANT *************************************************************
/// All the modifications to the account data must set the `Dirty` state in the
/// `AccountEntry`. This is done in `require` and `require_or_from`. So just
/// use that.
/// ****************************************************************************
///
/// Upon destruction all the local cache data propagated into the global cache.
/// Propagated items might be rejected if current state is non-canonical.
///
/// State checkpointing.
///
/// A new checkpoint can be created with `checkpoint()`. checkpoints can be
/// created in a hierarchy.
/// When a checkpoint is active all changes are applied directly into
/// `cache` and the original value is copied into an active checkpoint.
/// Reverting a checkpoint with `revert_to_checkpoint` involves copying
/// original values from the latest checkpoint back into `cache`. The code
/// takes care not to overwrite cached storage while doing that.
/// checkpoint can be discarded with `discard_checkpoint`. All of the orignal
/// backed-up values are moved into a parent checkpoint (if any).
///
pub struct State<B> {
	db: B,
	root: H256,
	cache: RefCell<HashMap<Address, AccountEntry>>,
	// The original account is preserved in
	checkpoints: RefCell<Vec<HashMap<Address, Option<AccountEntry>>>>,
	account_start_nonce: U256,
	factories: Factories,
}

#[derive(Copy, Clone)]
enum RequireCache {
	None,
	CodeSize,
	Code,
}

/// Mode of dealing with null accounts.
#[derive(PartialEq)]
pub enum CleanupMode<'a> {
	/// Create accounts which would be null.
	ForceCreate,
	/// Don't delete null accounts upon touching, but also don't create them.
	NoEmpty,
	/// Mark all touched accounts.
	TrackTouched(&'a mut HashSet<Address>),
}

/// Provides subset of `State` methods to query state information
pub trait StateInfo {
	/// Get the nonce of account `a`.
	fn nonce(&self, a: &Address) -> TrieResult<U256>;

	/// Get the balance of account `a`.
	fn balance(&self, a: &Address) -> TrieResult<U256>;

	/// Mutate storage of account `address` so that it is `value` for `key`.
	fn storage_at(&self, address: &Address, key: &H256) -> TrieResult<H256>;

	/// Get accounts' code.
	fn code(&self, a: &Address) -> TrieResult<Option<Arc<Bytes>>>;
}

impl<B: Backend> StateInfo for State<B> {
	fn nonce(&self, a: &Address) -> TrieResult<U256> { State::nonce(self, a) }
	fn balance(&self, a: &Address) -> TrieResult<U256> { State::balance(self, a) }
	fn storage_at(&self, address: &Address, key: &H256) -> TrieResult<H256> { State::storage_at(self, address, key) }
	fn code(&self, address: &Address) -> TrieResult<Option<Arc<Bytes>>> { State::code(self, address) }
}

const SEC_TRIE_DB_UNWRAP_STR: &'static str = "A state can only be created with valid root. Creating a SecTrieDB with a valid root will not fail. \
			 Therefore creating a SecTrieDB with this state's root will not fail.";

impl<B: Backend> State<B> {
	/// Creates new state with empty state root
	/// Used for tests.
	pub fn new(mut db: B, account_start_nonce: U256, factories: Factories) -> State<B> {
		let mut root = H256::new();
		{
			// init trie and reset root to null
			let _ = factories.trie.create(db.as_hash_db_mut(), &mut root);
		}

		State {
			db: db,
			root: root,
			cache: RefCell::new(HashMap::new()),
			checkpoints: RefCell::new(Vec::new()),
			account_start_nonce: account_start_nonce,
			factories: factories,
		}
	}

	/// Creates new state with existing state root
	pub fn from_existing(db: B, root: H256, account_start_nonce: U256, factories: Factories) -> TrieResult<State<B>> {
		if !db.as_hash_db().contains(&root) {
			return Err(Box::new(TrieError::InvalidStateRoot(root)));
		}

		let state = State {
			db: db,
			root: root,
			cache: RefCell::new(HashMap::new()),
			checkpoints: RefCell::new(Vec::new()),
			account_start_nonce: account_start_nonce,
			factories: factories
		};

		Ok(state)
	}

	/// Get a VM factory that can execute on this state.
	pub fn vm_factory(&self) -> VmFactory {
		self.factories.vm.clone()
	}

	/// Create a recoverable checkpoint of this state. Return the checkpoint index.
	pub fn checkpoint(&mut self) -> usize {
		let checkpoints = self.checkpoints.get_mut();
		let index = checkpoints.len();
		checkpoints.push(HashMap::new());
		index
	}

	/// Merge last checkpoint with previous.
	pub fn discard_checkpoint(&mut self) {
		// merge with previous checkpoint
		let last = self.checkpoints.get_mut().pop();
		if let Some(mut checkpoint) = last {
			if let Some(ref mut prev) = self.checkpoints.get_mut().last_mut() {
				if prev.is_empty() {
					**prev = checkpoint;
				} else {
					for (k, v) in checkpoint.drain() {
						prev.entry(k).or_insert(v);
					}
				}
			}
		}
	}

	/// Revert to the last checkpoint and discard it.
	pub fn revert_to_checkpoint(&mut self) {
		if let Some(mut checkpoint) = self.checkpoints.get_mut().pop() {
			for (k, v) in checkpoint.drain() {
				match v {
					Some(v) => {
						match self.cache.get_mut().entry(k) {
							Entry::Occupied(mut e) => {
								// Merge checkpointed changes back into the main account
								// storage preserving the cache.
								e.get_mut().overwrite_with(v);
							},
							Entry::Vacant(e) => {
								e.insert(v);
							}
						}
					},
					None => {
						if let Entry::Occupied(e) = self.cache.get_mut().entry(k) {
							if e.get().is_dirty() {
								e.remove();
							}
						}
					}
				}
			}
		}
	}

	fn insert_cache(&self, address: &Address, account: AccountEntry) {
		// Dirty account which is not in the cache means this is a new account.
		// It goes directly into the checkpoint as there's nothing to rever to.
		//
		// In all other cases account is read as clean first, and after that made
		// dirty in and added to the checkpoint with `note_cache`.
		let is_dirty = account.is_dirty();
		let old_value = self.cache.borrow_mut().insert(*address, account);
		if is_dirty {
			if let Some(ref mut checkpoint) = self.checkpoints.borrow_mut().last_mut() {
				checkpoint.entry(*address).or_insert(old_value);
			}
		}
	}

	fn note_cache(&self, address: &Address) {
		if let Some(ref mut checkpoint) = self.checkpoints.borrow_mut().last_mut() {
			checkpoint.entry(*address)
				.or_insert_with(|| self.cache.borrow().get(address).map(AccountEntry::clone_dirty));
		}
	}

	/// Destroy the current object and return root and database.
	pub fn drop(mut self) -> (H256, B) {
		self.propagate_to_global_cache();
		(self.root, self.db)
	}

	/// Destroy the current object and return single account data.
	pub fn into_account(self, account: &Address) -> TrieResult<(Option<Arc<Bytes>>, HashMap<H256, H256>)> {
		// TODO: deconstruct without cloning.
		let account = self.require(account, true)?;
		Ok((account.code().clone(), account.storage_changes().clone()))
	}

	/// Return reference to root
	pub fn root(&self) -> &H256 {
		&self.root
	}

	/// Create a new contract at address `contract`. If there is already an account at the address
	/// it will have its code reset, ready for `init_code()`.
	pub fn new_contract(&mut self, contract: &Address, balance: U256, nonce_offset: U256) -> TrieResult<()> {
		let original_storage_root = self.original_storage_root(contract)?;
		let (nonce, overflow) = self.account_start_nonce.overflowing_add(nonce_offset);
		if overflow {
			return Err(Box::new(TrieError::DecoderError(H256::from(contract),
				rlp::DecoderError::Custom("Nonce overflow".into()))));
		}
		self.insert_cache(contract, AccountEntry::new_dirty(Some(Account::new_contract(balance, nonce, original_storage_root))));
		Ok(())
	}

	/// Remove an existing account.
	pub fn kill_account(&mut self, account: &Address) {
		self.insert_cache(account, AccountEntry::new_dirty(None));
	}

	/// Determine whether an account exists.
	pub fn exists(&self, a: &Address) -> TrieResult<bool> {
		// Bloom filter does not contain empty accounts, so it is important here to
		// check if account exists in the database directly before EIP-161 is in effect.
		self.ensure_cached(a, RequireCache::None, false, |a| a.is_some())
	}

	/// Determine whether an account exists and if not empty.
	pub fn exists_and_not_null(&self, a: &Address) -> TrieResult<bool> {
		self.ensure_cached(a, RequireCache::None, false, |a| a.map_or(false, |a| !a.is_null()))
	}

	/// Determine whether an account exists and has code or non-zero nonce.
	pub fn exists_and_has_code_or_nonce(&self, a: &Address) -> TrieResult<bool> {
		self.ensure_cached(a, RequireCache::CodeSize, false,
			|a| a.map_or(false, |a| a.code_hash() != KECCAK_EMPTY || *a.nonce() != self.account_start_nonce))
	}

	/// Get the balance of account `a`.
	pub fn balance(&self, a: &Address) -> TrieResult<U256> {
		self.ensure_cached(a, RequireCache::None, true,
			|a| a.as_ref().map_or(U256::zero(), |account| *account.balance()))
	}

	/// Get the nonce of account `a`.
	pub fn nonce(&self, a: &Address) -> TrieResult<U256> {
		self.ensure_cached(a, RequireCache::None, true,
			|a| a.as_ref().map_or(self.account_start_nonce, |account| *account.nonce()))
	}

	/// Whether the base storage root of an account remains unchanged.
	pub fn is_base_storage_root_unchanged(&self, a: &Address) -> TrieResult<bool> {
		Ok(self.ensure_cached(a, RequireCache::None, true,
			|a| a.as_ref().map(|account| account.is_base_storage_root_unchanged()))?
			.unwrap_or(true))
	}

	/// Get the storage root of account `a`.
	pub fn storage_root(&self, a: &Address) -> TrieResult<Option<H256>> {
		self.ensure_cached(a, RequireCache::None, true,
			|a| a.as_ref().and_then(|account| account.storage_root()))
	}

	/// Get the original storage root since last commit of account `a`.
	pub fn original_storage_root(&self, a: &Address) -> TrieResult<H256> {
		Ok(self.ensure_cached(a, RequireCache::None, true,
			|a| a.as_ref().map(|account| account.original_storage_root()))?
			.unwrap_or(KECCAK_NULL_RLP))
	}

	/// Get the value of storage at a specific checkpoint.
	pub fn checkpoint_storage_at(&self, start_checkpoint_index: usize, address: &Address, key: &H256) -> TrieResult<Option<H256>> {
		#[must_use]
		enum ReturnKind {
			/// Use original storage at value at this address.
			OriginalAt,
			/// The checkpoint storage value is the same as the checkpoint storage value at the next checkpoint.
			SameAsNext,
		}

		let kind = {
			let checkpoints = self.checkpoints.borrow();

			if start_checkpoint_index >= checkpoints.len() {
				// The checkpoint was not found. Return None.
				return Ok(None);
			}

			let mut kind = None;

			for checkpoint in checkpoints.iter().skip(start_checkpoint_index) {
				match checkpoint.get(address) {
					// The account exists at this checkpoint.
					Some(Some(AccountEntry { account: Some(ref account), .. })) => {
						if let Some(value) = account.cached_storage_at(key) {
							return Ok(Some(value));
						} else {
							// This account has checkpoint entry, but the key is not in the entry's cache. We can use
							// original_storage_at if current account's original storage root is the same as checkpoint
							// account's original storage root. Otherwise, the account must be a newly created contract.
							if account.base_storage_root() == self.original_storage_root(address)? {
								kind = Some(ReturnKind::OriginalAt);
								break
							} else {
								// If account base storage root is different from the original storage root since last
								// commit, then it can only be created from a new contract, where the base storage root
								// would always be empty. Note that this branch is actually never called, because
								// `cached_storage_at` handled this case.
								warn!(target: "state", "Trying to get an account's cached storage value, but base storage root does not equal to original storage root! Assuming the value is empty.");
								return Ok(Some(H256::new()));
							}
						}
					},
					// The account didn't exist at that point. Return empty value.
					Some(Some(AccountEntry { account: None, .. })) => return Ok(Some(H256::new())),
					// The value was not cached at that checkpoint, meaning it was not modified at all.
					Some(None) => {
						kind = Some(ReturnKind::OriginalAt);
						break
					},
					// This key does not have a checkpoint entry.
					None => {
						kind = Some(ReturnKind::SameAsNext);
					},
				}
			}

			kind.expect("start_checkpoint_index is checked to be below checkpoints_len; for loop above must have been executed at least once; it will either early return, or set the kind value to Some; qed")
		};

		match kind {
			ReturnKind::SameAsNext => {
				// If we reached here, all previous SameAsNext failed to early return. It means that the value we want
				// to fetch is the same as current.
				Ok(Some(self.storage_at(address, key)?))
			},
			ReturnKind::OriginalAt => Ok(Some(self.original_storage_at(address, key)?)),
		}
	}

	fn storage_at_inner<FCachedStorageAt, FStorageAt>(
		&self, address: &Address, key: &H256, f_cached_at: FCachedStorageAt, f_at: FStorageAt,
	) -> TrieResult<H256> where
		FCachedStorageAt: Fn(&Account, &H256) -> Option<H256>,
		FStorageAt: Fn(&Account, &HashDB<KeccakHasher, DBValue>, &H256) -> TrieResult<H256>
	{
		// Storage key search and update works like this:
		// 1. If there's an entry for the account in the local cache check for the key and return it if found.
		// 2. If there's an entry for the account in the global cache check for the key or load it into that account.
		// 3. If account is missing in the global cache load it into the local cache and cache the key there.

		{
			// check local cache first without updating
			let local_cache = self.cache.borrow_mut();
			let mut local_account = None;
			if let Some(maybe_acc) = local_cache.get(address) {
				match maybe_acc.account {
					Some(ref account) => {
						if let Some(value) = f_cached_at(account, key) {
							return Ok(value);
						} else {
							local_account = Some(maybe_acc);
						}
					},
					_ => return Ok(H256::new()),
				}
			}
			// check the global cache and and cache storage key there if found,
			let trie_res = self.db.get_cached(address, |acc| match acc {
				None => Ok(H256::new()),
				Some(a) => {
					let account_db = self.factories.accountdb.readonly(self.db.as_hash_db(), a.address_hash(address));
					f_at(a, account_db.as_hash_db(), key)
				}
			});

			if let Some(res) = trie_res {
				return res;
			}

			// otherwise cache the account localy and cache storage key there.
			if let Some(ref mut acc) = local_account {
				if let Some(ref account) = acc.account {
					let account_db = self.factories.accountdb.readonly(self.db.as_hash_db(), account.address_hash(address));
					return f_at(account, account_db.as_hash_db(), key)
				} else {
					return Ok(H256::new())
				}
			}
		}

		// check if the account could exist before any requests to trie
		if self.db.is_known_null(address) { return Ok(H256::zero()) }

		// account is not found in the global cache, get from the DB and insert into local
		let db = &self.db.as_hash_db();
		let db = self.factories.trie.readonly(db, &self.root).expect(SEC_TRIE_DB_UNWRAP_STR);
		let from_rlp = |b: &[u8]| Account::from_rlp(b).expect("decoding db value failed");
		let maybe_acc = db.get_with(address, from_rlp)?;
		let r = maybe_acc.as_ref().map_or(Ok(H256::new()), |a| {
			let account_db = self.factories.accountdb.readonly(self.db.as_hash_db(), a.address_hash(address));
			f_at(a, account_db.as_hash_db(), key)
		});
		self.insert_cache(address, AccountEntry::new_clean(maybe_acc));
		r
	}

	/// Mutate storage of account `address` so that it is `value` for `key`.
	pub fn storage_at(&self, address: &Address, key: &H256) -> TrieResult<H256> {
		self.storage_at_inner(
			address,
			key,
			|account, key| { account.cached_storage_at(key) },
			|account, db, key| { account.storage_at(db, key) },
		)
	}

	/// Get the value of storage after last state commitment.
	pub fn original_storage_at(&self, address: &Address, key: &H256) -> TrieResult<H256> {
		self.storage_at_inner(
			address,
			key,
			|account, key| { account.cached_original_storage_at(key) },
			|account, db, key| { account.original_storage_at(db, key) },
		)
	}

	/// Get accounts' code.
	pub fn code(&self, a: &Address) -> TrieResult<Option<Arc<Bytes>>> {
		self.ensure_cached(a, RequireCache::Code, true,
			|a| a.as_ref().map_or(None, |a| a.code().clone()))
	}

	/// Get an account's code hash.
	pub fn code_hash(&self, a: &Address) -> TrieResult<Option<H256>> {
		self.ensure_cached(a, RequireCache::None, true,
			|a| a.as_ref().map(|a| a.code_hash()))
	}

	/// Get accounts' code size.
	pub fn code_size(&self, a: &Address) -> TrieResult<Option<usize>> {
		self.ensure_cached(a, RequireCache::CodeSize, true,
			|a| a.as_ref().and_then(|a| a.code_size()))
	}

	/// Add `incr` to the balance of account `a`.
	pub fn add_balance(&mut self, a: &Address, incr: &U256, cleanup_mode: CleanupMode) -> TrieResult<()> {
		trace!(target: "state", "add_balance({}, {}): {}", a, incr, self.balance(a)?);
		let is_value_transfer = !incr.is_zero();
		if is_value_transfer || (cleanup_mode == CleanupMode::ForceCreate && !self.exists(a)?) {
			self.require(a, false)?.add_balance(incr);
		} else if let CleanupMode::TrackTouched(set) = cleanup_mode {
			if self.exists(a)? {
				set.insert(*a);
				self.touch(a)?;
			}
		}
		Ok(())
	}

	/// Subtract `decr` from the balance of account `a`.
	pub fn sub_balance(&mut self, a: &Address, decr: &U256, cleanup_mode: &mut CleanupMode) -> TrieResult<()> {
		trace!(target: "state", "sub_balance({}, {}): {}", a, decr, self.balance(a)?);
		if !decr.is_zero() || !self.exists(a)? {
			self.require(a, false)?.sub_balance(decr);
		}
		if let CleanupMode::TrackTouched(ref mut set) = *cleanup_mode {
			set.insert(*a);
		}
		Ok(())
	}

	/// Subtracts `by` from the balance of `from` and adds it to that of `to`.
	pub fn transfer_balance(&mut self, from: &Address, to: &Address, by: &U256, mut cleanup_mode: CleanupMode) -> TrieResult<()> {
		self.sub_balance(from, by, &mut cleanup_mode)?;
		self.add_balance(to, by, cleanup_mode)?;
		Ok(())
	}

	/// Increment the nonce of account `a` by 1.
	pub fn inc_nonce(&mut self, a: &Address) -> TrieResult<()> {
		self.require(a, false).map(|mut x| x.inc_nonce())
	}

	/// Mutate storage of account `a` so that it is `value` for `key`.
	pub fn set_storage(&mut self, a: &Address, key: H256, value: H256) -> TrieResult<()> {
		trace!(target: "state", "set_storage({}:{:x} to {:x})", a, key, value);
		if self.storage_at(a, &key)? != value {
			self.require(a, false)?.set_storage(key, value)
		}

		Ok(())
	}

	/// Initialise the code of account `a` so that it is `code`.
	/// NOTE: Account should have been created with `new_contract`.
	pub fn init_code(&mut self, a: &Address, code: Bytes) -> TrieResult<()> {
		self.require_or_from(a, true, || Account::new_contract(0.into(), self.account_start_nonce, KECCAK_NULL_RLP), |_| {})?.init_code(code);
		Ok(())
	}

	/// Reset the code of account `a` so that it is `code`.
	pub fn reset_code(&mut self, a: &Address, code: Bytes) -> TrieResult<()> {
		self.require_or_from(a, true, || Account::new_contract(0.into(), self.account_start_nonce, KECCAK_NULL_RLP), |_| {})?.reset_code(code);
		Ok(())
	}

	/// Execute a given transaction, producing a receipt and an optional trace.
	/// This will change the state accordingly.
	pub fn apply(&mut self, env_info: &EnvInfo, machine: &Machine, t: &SignedTransaction, tracing: bool) -> ApplyResult<FlatTrace, VMTrace> {
		if tracing {
			let options = TransactOptions::with_tracing();
			self.apply_with_tracing(env_info, machine, t, options.tracer, options.vm_tracer)
		} else {
			let options = TransactOptions::with_no_tracing();
			self.apply_with_tracing(env_info, machine, t, options.tracer, options.vm_tracer)
		}
	}

	/// Execute a given transaction with given tracer and VM tracer producing a receipt and an optional trace.
	/// This will change the state accordingly.
	pub fn apply_with_tracing<V, T>(
		&mut self,
		env_info: &EnvInfo,
		machine: &Machine,
		t: &SignedTransaction,
		tracer: T,
		vm_tracer: V,
	) -> ApplyResult<T::Output, V::Output> where
		T: trace::Tracer,
		V: trace::VMTracer,
	{
		let options = TransactOptions::new(tracer, vm_tracer);
		let e = self.execute(env_info, machine, t, options, false)?;
		let params = machine.params();

		let eip658 = env_info.number >= params.eip658_transition;
		let no_intermediate_commits =
			eip658 ||
			(env_info.number >= params.eip98_transition && env_info.number >= params.validate_receipts_transition);

		let outcome = if no_intermediate_commits {
			if eip658 {
				TransactionOutcome::StatusCode(if e.exception.is_some() { 0 } else { 1 })
			} else {
				TransactionOutcome::Unknown
			}
		} else {
			self.commit()?;
			TransactionOutcome::StateRoot(self.root().clone())
		};

		let output = e.output;
		let receipt = Receipt::new(outcome, e.cumulative_gas_used, e.logs);
		trace!(target: "state", "Transaction receipt: {:?}", receipt);

		Ok(ApplyOutcome {
			receipt,
			output,
			trace: e.trace,
			vm_trace: e.vm_trace,
		})
	}

	// Execute a given transaction without committing changes.
	//
	// `virt` signals that we are executing outside of a block set and restrictions like
	// gas limits and gas costs should be lifted.
	fn execute<T, V>(&mut self, env_info: &EnvInfo, machine: &Machine, t: &SignedTransaction, options: TransactOptions<T, V>, virt: bool)
		-> Result<Executed<T::Output, V::Output>, ExecutionError> where T: trace::Tracer, V: trace::VMTracer,
	{
		let schedule = machine.schedule(env_info.number);
		let mut e = Executive::new(self, env_info, machine, &schedule);

		match virt {
			true => e.transact_virtual(t, options),
			false => e.transact(t, options),
		}
	}

	fn touch(&mut self, a: &Address) -> TrieResult<()> {
		self.require(a, false)?;
		Ok(())
	}

	/// Commits our cached account changes into the trie.
	pub fn commit(&mut self) -> Result<(), Error> {
		assert!(self.checkpoints.borrow().is_empty());
		// first, commit the sub trees.
		let mut accounts = self.cache.borrow_mut();
		for (address, ref mut a) in accounts.iter_mut().filter(|&(_, ref a)| a.is_dirty()) {
			if let Some(ref mut account) = a.account {
				let addr_hash = account.address_hash(address);
				{
					let mut account_db = self.factories.accountdb.create(self.db.as_hash_db_mut(), addr_hash);
					account.commit_storage(&self.factories.trie, account_db.as_hash_db_mut())?;
					account.commit_code(account_db.as_hash_db_mut());
				}
				if !account.is_empty() {
					self.db.note_non_null_account(address);
				}
			}
		}

		{
			let mut trie = self.factories.trie.from_existing(self.db.as_hash_db_mut(), &mut self.root)?;
			for (address, ref mut a) in accounts.iter_mut().filter(|&(_, ref a)| a.is_dirty()) {
				a.state = AccountState::Committed;
				match a.account {
					Some(ref mut account) => {
						trie.insert(address, &account.rlp())?;
					},
					None => {
						trie.remove(address)?;
					},
				};
			}
		}

		Ok(())
	}

	/// Propagate local cache into shared canonical state cache.
	fn propagate_to_global_cache(&mut self) {
		let mut addresses = self.cache.borrow_mut();
		trace!("Committing cache {:?} entries", addresses.len());
		for (address, a) in addresses.drain().filter(|&(_, ref a)| a.state == AccountState::Committed || a.state == AccountState::CleanFresh) {
			self.db.add_to_account_cache(address, a.account, a.state == AccountState::Committed);
		}
	}

	/// Clear state cache
	pub fn clear(&mut self) {
		assert!(self.checkpoints.borrow().is_empty());
		self.cache.borrow_mut().clear();
	}

	/// Remove any touched empty or dust accounts.
	pub fn kill_garbage(&mut self, touched: &HashSet<Address>, remove_empty_touched: bool, min_balance: &Option<U256>, kill_contracts: bool) -> TrieResult<()> {
		let to_kill: HashSet<_> = {
			self.cache.borrow().iter().filter_map(|(address, ref entry)|
			if touched.contains(address) && // Check all touched accounts
				((remove_empty_touched && entry.exists_and_is_null()) // Remove all empty touched accounts.
				|| min_balance.map_or(false, |ref balance| entry.account.as_ref().map_or(false, |account|
					(account.is_basic() || kill_contracts) // Remove all basic and optionally contract accounts where balance has been decreased.
					&& account.balance() < balance && entry.old_balance.as_ref().map_or(false, |b| account.balance() < b)))) {

				Some(address.clone())
			} else { None }).collect()
		};
		for address in to_kill {
			self.kill_account(&address);
		}
		Ok(())
	}

	/// Populate the state from `accounts`.
	/// Used for tests.
	pub fn populate_from(&mut self, accounts: PodState) {
		assert!(self.checkpoints.borrow().is_empty());
		for (add, acc) in accounts.drain().into_iter() {
			self.cache.borrow_mut().insert(add, AccountEntry::new_dirty(Some(Account::from_pod(acc))));
		}
	}

	/// Populate a PodAccount map from this state.
	fn to_pod_cache(&self) -> PodState {
		assert!(self.checkpoints.borrow().is_empty());
		PodState::from(self.cache.borrow().iter().fold(BTreeMap::new(), |mut m, (add, opt)| {
			if let Some(ref acc) = opt.account {
				m.insert(*add, PodAccount::from_account(acc));
			}
			m
		}))
	}

	#[cfg(feature="to-pod-full")]
	/// Populate a PodAccount map from this state.
	/// Warning this is not for real time use.
	/// Use of this method requires FatDB mode to be able
	/// to iterate on accounts.
	pub fn to_pod_full(&self) -> Result<PodState, Error> {

		assert!(self.checkpoints.borrow().is_empty());
		assert!(self.factories.trie.is_fat());

		let mut result = BTreeMap::new();

		let db = &self.db.as_hash_db();
		let trie = self.factories.trie.readonly(db, &self.root)?;

		// put trie in cache
		for item in trie.iter()? {
			if let Ok((addr, _dbval)) = item {
				let address = Address::from_slice(&addr);
				let _ = self.require(&address, true);
			}
		}

		// Resolve missing part
		for (add, opt) in self.cache.borrow().iter() {
			if let Some(ref acc) = opt.account {
				let pod_account = self.account_to_pod_account(acc, add)?;
				result.insert(add.clone(), pod_account);
			}
		}

		Ok(PodState::from(result))
	}

	/// Create a PodAccount from an account.
	/// Differs from existing method by including all storage
	/// values of the account to the PodAccount.
	/// This function is only intended for use in small tests or with fresh accounts.
	/// It requires FatDB.
	#[cfg(feature="to-pod-full")]
	fn account_to_pod_account(&self, account: &Account, address: &Address) -> Result<PodAccount, Error> {
		let mut pod_storage = BTreeMap::new();
		let addr_hash = account.address_hash(address);
		let accountdb = self.factories.accountdb.readonly(self.db.as_hash_db(), addr_hash);
		let root = account.base_storage_root();

		let accountdb = &accountdb.as_hash_db();
		let trie = self.factories.trie.readonly(accountdb, &root)?;
		for o_kv in trie.iter()? {
			if let Ok((key, val)) = o_kv {
				pod_storage.insert(key[..].into(), rlp::decode::<U256>(&val[..]).expect("Decoded from trie which was encoded from the same type; qed").into());
			}
		}

		let mut pod_account = PodAccount::from_account(&account);
		// cached one first
		pod_storage.append(&mut pod_account.storage);
		pod_account.storage = pod_storage;
		Ok(pod_account)
	}

	/// Populate a PodAccount map from this state, with another state as the account and storage query.
	fn to_pod_diff<X: Backend>(&mut self, query: &State<X>) -> TrieResult<PodState> {
		assert!(self.checkpoints.borrow().is_empty());

		// Merge PodAccount::to_pod for cache of self and `query`.
		let all_addresses = self.cache.borrow().keys().cloned()
			.chain(query.cache.borrow().keys().cloned())
			.collect::<BTreeSet<_>>();

		Ok(PodState::from(all_addresses.into_iter().fold(Ok(BTreeMap::new()), |m: TrieResult<_>, address| {
			let mut m = m?;

			let account = self.ensure_cached(&address, RequireCache::Code, true, |acc| {
				acc.map(|acc| {
					// Merge all modified storage keys.
					let all_keys = {
						let self_keys = acc.storage_changes().keys().cloned()
							.collect::<BTreeSet<_>>();

						if let Some(ref query_storage) = query.cache.borrow().get(&address)
							.and_then(|opt| {
								Some(opt.account.as_ref()?.storage_changes().keys().cloned()
									 .collect::<BTreeSet<_>>())
							})
						{
							self_keys.union(&query_storage).cloned().collect::<Vec<_>>()
						} else {
							self_keys.into_iter().collect::<Vec<_>>()
						}
					};

					// Storage must be fetched after ensure_cached to avoid borrow problem.
					(*acc.balance(), *acc.nonce(), all_keys, acc.code().map(|x| x.to_vec()))
				})
			})?;

			if let Some((balance, nonce, storage_keys, code)) = account {
				let storage = storage_keys.into_iter().fold(Ok(BTreeMap::new()), |s: TrieResult<_>, key| {
					let mut s = s?;

					s.insert(key, self.storage_at(&address, &key)?);
					Ok(s)
				})?;

				m.insert(address, PodAccount {
					balance, nonce, storage, code
				});
			}

			Ok(m)
		})?))
	}

	/// Returns a `StateDiff` describing the difference from `orig` to `self`.
	/// Consumes self.
	pub fn diff_from<X: Backend>(&self, mut orig: State<X>) -> TrieResult<StateDiff> {
		let pod_state_post = self.to_pod_cache();
		let pod_state_pre = orig.to_pod_diff(self)?;
		Ok(pod_state::diff_pod(&pod_state_pre, &pod_state_post))
	}

	/// Load required account data from the databases. Returns whether the cache succeeds.
	#[must_use]
	fn update_account_cache(require: RequireCache, account: &mut Account, state_db: &B, db: &HashDB<KeccakHasher, DBValue>) -> bool {
		if let RequireCache::None = require {
			return true;
		}

		if account.is_cached() {
			return true;
		}

		// if there's already code in the global cache, always cache it localy
		let hash = account.code_hash();
		match state_db.get_cached_code(&hash) {
			Some(code) => {
				account.cache_given_code(code);
				true
			},
			None => match require {
				RequireCache::None => true,
				RequireCache::Code => {
					if let Some(code) = account.cache_code(db) {
						// propagate code loaded from the database to
						// the global code cache.
						state_db.cache_code(hash, code);
						true
					} else {
						false
					}
				},
				RequireCache::CodeSize => {
					account.cache_code_size(db)
				}
			}
		}
	}

	/// Check caches for required data
	/// First searches for account in the local, then the shared cache.
	/// Populates local cache if nothing found.
	fn ensure_cached<F, U>(&self, a: &Address, require: RequireCache, check_null: bool, f: F) -> TrieResult<U>
		where F: Fn(Option<&Account>) -> U {
		// check local cache first
		if let Some(ref mut maybe_acc) = self.cache.borrow_mut().get_mut(a) {
			if let Some(ref mut account) = maybe_acc.account {
				let accountdb = self.factories.accountdb.readonly(self.db.as_hash_db(), account.address_hash(a));
				if Self::update_account_cache(require, account, &self.db, accountdb.as_hash_db()) {
					return Ok(f(Some(account)));
				} else {
					return Err(Box::new(TrieError::IncompleteDatabase(H256::from(a))));
				}
			}
			return Ok(f(None));
		}
		// check global cache
		let result = self.db.get_cached(a, |mut acc| {
			if let Some(ref mut account) = acc {
				let accountdb = self.factories.accountdb.readonly(self.db.as_hash_db(), account.address_hash(a));
				if !Self::update_account_cache(require, account, &self.db, accountdb.as_hash_db()) {
					return Err(Box::new(TrieError::IncompleteDatabase(H256::from(a))));
				}
			}
			Ok(f(acc.map(|a| &*a)))
		});
		match result {
			Some(r) => Ok(r?),
			None => {
				// first check if it is not in database for sure
				if check_null && self.db.is_known_null(a) { return Ok(f(None)); }

				// not found in the global cache, get from the DB and insert into local
				let db = &self.db.as_hash_db();
				let db = self.factories.trie.readonly(db, &self.root)?;
				let from_rlp = |b: &[u8]| Account::from_rlp(b).expect("decoding db value failed");
				let mut maybe_acc = db.get_with(a, from_rlp)?;
				if let Some(ref mut account) = maybe_acc.as_mut() {
					let accountdb = self.factories.accountdb.readonly(self.db.as_hash_db(), account.address_hash(a));
					if !Self::update_account_cache(require, account, &self.db, accountdb.as_hash_db()) {
						return Err(Box::new(TrieError::IncompleteDatabase(H256::from(a))));
					}
				}
				let r = f(maybe_acc.as_ref());
				self.insert_cache(a, AccountEntry::new_clean(maybe_acc));
				Ok(r)
			}
		}
	}

	/// Pull account `a` in our cache from the trie DB. `require_code` requires that the code be cached, too.
	fn require<'a>(&'a self, a: &Address, require_code: bool) -> TrieResult<RefMut<'a, Account>> {
		self.require_or_from(a, require_code, || Account::new_basic(0u8.into(), self.account_start_nonce), |_| {})
	}

	/// Pull account `a` in our cache from the trie DB. `require_code` requires that the code be cached, too.
	/// If it doesn't exist, make account equal the evaluation of `default`.
	fn require_or_from<'a, F, G>(&'a self, a: &Address, require_code: bool, default: F, not_default: G) -> TrieResult<RefMut<'a, Account>>
		where F: FnOnce() -> Account, G: FnOnce(&mut Account),
	{
		let contains_key = self.cache.borrow().contains_key(a);
		if !contains_key {
			match self.db.get_cached_account(a) {
				Some(acc) => self.insert_cache(a, AccountEntry::new_clean_cached(acc)),
				None => {
					let maybe_acc = if !self.db.is_known_null(a) {
						let db = &self.db.as_hash_db();
						let db = self.factories.trie.readonly(db, &self.root)?;
						let from_rlp = |b:&[u8]| { Account::from_rlp(b).expect("decoding db value failed") };
						AccountEntry::new_clean(db.get_with(a, from_rlp)?)
					} else {
						AccountEntry::new_clean(None)
					};
					self.insert_cache(a, maybe_acc);
				}
			}
		}
		self.note_cache(a);

		// at this point the entry is guaranteed to be in the cache.
		let mut account = RefMut::map(self.cache.borrow_mut(), |c| {
			let entry = c.get_mut(a).expect("entry known to exist in the cache; qed");

			match &mut entry.account {
				&mut Some(ref mut acc) => not_default(acc),
				slot => *slot = Some(default()),
			}

			// set the dirty flag after changing account data.
			entry.state = AccountState::Dirty;
			entry.account.as_mut().expect("Required account must always exist; qed")
		});

		if require_code {
			let addr_hash = account.address_hash(a);
			let accountdb = self.factories.accountdb.readonly(self.db.as_hash_db(), addr_hash);

			if !Self::update_account_cache(RequireCache::Code, &mut account, &self.db, accountdb.as_hash_db()) {
				return Err(Box::new(TrieError::IncompleteDatabase(H256::from(a))))
			}
		}

		Ok(account)
	}

	/// Replace account code and storage. Creates account if it does not exist.
	pub fn patch_account(&self, a: &Address, code: Arc<Bytes>, storage: HashMap<H256, H256>) -> TrieResult<()> {
		Ok(self.require(a, false)?.reset_code_and_storage(code, storage))
	}
}

// State proof implementations; useful for light client protocols.
impl<B: Backend> State<B> {
	/// Prove an account's existence or nonexistence in the state trie.
	/// Returns a merkle proof of the account's trie node omitted or an encountered trie error.
	/// If the account doesn't exist in the trie, prove that and return defaults.
	/// Requires a secure trie to be used for accurate results.
	/// `account_key` == keccak(address)
	pub fn prove_account(&self, account_key: H256) -> TrieResult<(Vec<Bytes>, BasicAccount)> {
		let mut recorder = Recorder::new();
		let db = &self.db.as_hash_db();
		let trie = TrieDB::new(db, &self.root)?;
		let maybe_account: Option<BasicAccount> = {
			let panicky_decoder = |bytes: &[u8]| {
				::rlp::decode(bytes).unwrap_or_else(|_| panic!("prove_account, could not query trie for account key={}", &account_key))
			};
			let query = (&mut recorder, panicky_decoder);
			trie.get_with(&account_key, query)?
		};
		let account = maybe_account.unwrap_or_else(|| BasicAccount {
			balance: 0.into(),
			nonce: self.account_start_nonce,
			code_hash: KECCAK_EMPTY,
			storage_root: KECCAK_NULL_RLP,
		});

		Ok((recorder.drain().into_iter().map(|r| r.data).collect(), account))
	}

	/// Prove an account's storage key's existence or nonexistence in the state.
	/// Returns a merkle proof of the account's storage trie.
	/// Requires a secure trie to be used for correctness.
	/// `account_key` == keccak(address)
	/// `storage_key` == keccak(key)
	pub fn prove_storage(&self, account_key: H256, storage_key: H256) -> TrieResult<(Vec<Bytes>, H256)> {
		// TODO: probably could look into cache somehow but it's keyed by
		// address, not keccak(address).
		let db = &self.db.as_hash_db();
		let trie = TrieDB::new(db, &self.root)?;
		let from_rlp = |b: &[u8]| Account::from_rlp(b).expect("decoding db value failed");
		let acc = match trie.get_with(&account_key, from_rlp)? {
			Some(acc) => acc,
			None => return Ok((Vec::new(), H256::new())),
		};

		let account_db = self.factories.accountdb.readonly(self.db.as_hash_db(), account_key);
		acc.prove_storage(account_db.as_hash_db(), storage_key)
	}
}

impl<B: Backend> fmt::Debug for State<B> {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{:?}", self.cache.borrow())
	}
}

impl State<StateDB> {
	/// Get a reference to the underlying state DB.
	pub fn db(&self) -> &StateDB {
		&self.db
	}
}

// TODO: cloning for `State` shouldn't be possible in general; Remove this and use
// checkpoints where possible.
impl Clone for State<StateDB> {
	fn clone(&self) -> State<StateDB> {
		let cache = {
			let mut cache: HashMap<Address, AccountEntry> = HashMap::new();
			for (key, val) in self.cache.borrow().iter() {
				if let Some(entry) = val.clone_if_dirty() {
					cache.insert(key.clone(), entry);
				}
			}
			cache
		};

		State {
			db: self.db.boxed_clone(),
			root: self.root.clone(),
			cache: RefCell::new(cache),
			checkpoints: RefCell::new(Vec::new()),
			account_start_nonce: self.account_start_nonce.clone(),
			factories: self.factories.clone(),
		}
	}
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;
	use std::str::FromStr;
	use rustc_hex::FromHex;
	use hash::{keccak, KECCAK_NULL_RLP};
	use super::*;
	use ethkey::Secret;
	use ethereum_types::{H256, U256, Address};
	use test_helpers::{get_temp_state, get_temp_state_db};
	use machine::EthereumMachine;
	use vm::EnvInfo;
	use spec::*;
	use types::transaction::*;
	use trace::{FlatTrace, TraceError, trace};
	use evm::CallType;

	fn secret() -> Secret {
		keccak("").into()
	}

	fn make_frontier_machine(max_depth: usize) -> EthereumMachine {
		let mut machine = ::ethereum::new_frontier_test_machine();
		machine.set_schedule_creation_rules(Box::new(move |s, _| s.max_depth = max_depth));
		machine
	}

	#[test]
	fn should_apply_create_transaction() {
		let _ = env_logger::try_init();

		let mut state = get_temp_state();

		let mut info = EnvInfo::default();
		info.gas_limit = 1_000_000.into();
		let machine = make_frontier_machine(5);

		let t = Transaction {
			nonce: 0.into(),
			gas_price: 0.into(),
			gas: 100_000.into(),
			action: Action::Create,
			value: 100.into(),
			data: FromHex::from_hex("601080600c6000396000f3006000355415600957005b60203560003555").unwrap(),
		}.sign(&secret(), None);

		state.add_balance(&t.sender(), &(100.into()), CleanupMode::NoEmpty).unwrap();
		let result = state.apply(&info, &machine, &t, true).unwrap();
		let expected_trace = vec![FlatTrace {
			trace_address: Default::default(),
			subtraces: 0,
			action: trace::Action::Create(trace::Create {
				from: "9cce34f7ab185c7aba1b7c8140d620b4bda941d6".into(),
				value: 100.into(),
				gas: 77412.into(),
				init: vec![96, 16, 128, 96, 12, 96, 0, 57, 96, 0, 243, 0, 96, 0, 53, 84, 21, 96, 9, 87, 0, 91, 96, 32, 53, 96, 0, 53, 85],
			}),
			result: trace::Res::Create(trace::CreateResult {
				gas_used: U256::from(3224),
				address: Address::from_str("8988167e088c87cd314df6d3c2b83da5acb93ace").unwrap(),
				code: vec![96, 0, 53, 84, 21, 96, 9, 87, 0, 91, 96, 32, 53, 96, 0, 53]
			}),
		}];

		assert_eq!(result.trace, expected_trace);
	}

	#[test]
	fn should_work_when_cloned() {
		let _ = env_logger::try_init();

		let a = Address::zero();

		let mut state = {
			let mut state = get_temp_state();
			assert_eq!(state.exists(&a).unwrap(), false);
			state.inc_nonce(&a).unwrap();
			state.commit().unwrap();
			state.clone()
		};

		state.inc_nonce(&a).unwrap();
		state.commit().unwrap();
	}

	#[test]
	fn should_trace_failed_create_transaction() {
		let _ = env_logger::try_init();

		let mut state = get_temp_state();

		let mut info = EnvInfo::default();
		info.gas_limit = 1_000_000.into();
		let machine = make_frontier_machine(5);

		let t = Transaction {
			nonce: 0.into(),
			gas_price: 0.into(),
			gas: 100_000.into(),
			action: Action::Create,
			value: 100.into(),
			data: FromHex::from_hex("5b600056").unwrap(),
		}.sign(&secret(), None);

		state.add_balance(&t.sender(), &(100.into()), CleanupMode::NoEmpty).unwrap();
		let result = state.apply(&info, &machine, &t, true).unwrap();
		let expected_trace = vec![FlatTrace {
			trace_address: Default::default(),
			action: trace::Action::Create(trace::Create {
				from: "9cce34f7ab185c7aba1b7c8140d620b4bda941d6".into(),
				value: 100.into(),
				gas: 78792.into(),
				init: vec![91, 96, 0, 86],
			}),
			result: trace::Res::FailedCreate(TraceError::OutOfGas),
			subtraces: 0
		}];

		assert_eq!(result.trace, expected_trace);
	}

	#[test]
	fn should_trace_call_transaction() {
		let _ = env_logger::try_init();

		let mut state = get_temp_state();

		let mut info = EnvInfo::default();
		info.gas_limit = 1_000_000.into();
		let machine = make_frontier_machine(5);

		let t = Transaction {
			nonce: 0.into(),
			gas_price: 0.into(),
			gas: 100_000.into(),
			action: Action::Call(0xa.into()),
			value: 100.into(),
			data: vec![],
		}.sign(&secret(), None);

		state.init_code(&0xa.into(), FromHex::from_hex("6000").unwrap()).unwrap();
		state.add_balance(&t.sender(), &(100.into()), CleanupMode::NoEmpty).unwrap();
		let result = state.apply(&info, &machine, &t, true).unwrap();
		let expected_trace = vec![FlatTrace {
			trace_address: Default::default(),
			action: trace::Action::Call(trace::Call {
				from: "9cce34f7ab185c7aba1b7c8140d620b4bda941d6".into(),
				to: 0xa.into(),
				value: 100.into(),
				gas: 79000.into(),
				input: vec![],
				call_type: CallType::Call,
			}),
			result: trace::Res::Call(trace::CallResult {
				gas_used: U256::from(3),
				output: vec![]
			}),
			subtraces: 0,
		}];

		assert_eq!(result.trace, expected_trace);
	}

	#[test]
	fn should_trace_basic_call_transaction() {
		let _ = env_logger::try_init();

		let mut state = get_temp_state();

		let mut info = EnvInfo::default();
		info.gas_limit = 1_000_000.into();
		let machine = make_frontier_machine(5);

		let t = Transaction {
			nonce: 0.into(),
			gas_price: 0.into(),
			gas: 100_000.into(),
			action: Action::Call(0xa.into()),
			value: 100.into(),
			data: vec![],
		}.sign(&secret(), None);

		state.add_balance(&t.sender(), &(100.into()), CleanupMode::NoEmpty).unwrap();
		let result = state.apply(&info, &machine, &t, true).unwrap();
		let expected_trace = vec![FlatTrace {
			trace_address: Default::default(),
			action: trace::Action::Call(trace::Call {
				from: "9cce34f7ab185c7aba1b7c8140d620b4bda941d6".into(),
				to: 0xa.into(),
				value: 100.into(),
				gas: 79000.into(),
				input: vec![],
				call_type: CallType::Call,
			}),
			result: trace::Res::Call(trace::CallResult {
				gas_used: U256::from(0),
				output: vec![]
			}),
			subtraces: 0,
		}];

		assert_eq!(result.trace, expected_trace);
	}

	#[test]
	fn should_trace_call_transaction_to_builtin() {
		let _ = env_logger::try_init();

		let mut state = get_temp_state();

		let mut info = EnvInfo::default();
		info.gas_limit = 1_000_000.into();
		let machine = Spec::new_test_machine();

		let t = Transaction {
			nonce: 0.into(),
			gas_price: 0.into(),
			gas: 100_000.into(),
			action: Action::Call(0x1.into()),
			value: 0.into(),
			data: vec![],
		}.sign(&secret(), None);

		let result = state.apply(&info, &machine, &t, true).unwrap();

		let expected_trace = vec![FlatTrace {
			trace_address: Default::default(),
			action: trace::Action::Call(trace::Call {
				from: "9cce34f7ab185c7aba1b7c8140d620b4bda941d6".into(),
				to: "0000000000000000000000000000000000000001".into(),
				value: 0.into(),
				gas: 79_000.into(),
				input: vec![],
				call_type: CallType::Call,
			}),
			result: trace::Res::Call(trace::CallResult {
				gas_used: U256::from(3000),
				output: vec![]
			}),
			subtraces: 0,
		}];

		assert_eq!(result.trace, expected_trace);
	}

	#[test]
	fn should_not_trace_subcall_transaction_to_builtin() {
		let _ = env_logger::try_init();

		let mut state = get_temp_state();

		let mut info = EnvInfo::default();
		info.gas_limit = 1_000_000.into();
		let machine = Spec::new_test_machine();

		let t = Transaction {
			nonce: 0.into(),
			gas_price: 0.into(),
			gas: 100_000.into(),
			action: Action::Call(0xa.into()),
			value: 0.into(),
			data: vec![],
		}.sign(&secret(), None);

		state.init_code(&0xa.into(), FromHex::from_hex("600060006000600060006001610be0f1").unwrap()).unwrap();
		let result = state.apply(&info, &machine, &t, true).unwrap();

		let expected_trace = vec![FlatTrace {
			trace_address: Default::default(),
			action: trace::Action::Call(trace::Call {
				from: "9cce34f7ab185c7aba1b7c8140d620b4bda941d6".into(),
				to: 0xa.into(),
				value: 0.into(),
				gas: 79000.into(),
				input: vec![],
				call_type: CallType::Call,
			}),
			result: trace::Res::Call(trace::CallResult {
				gas_used: U256::from(3_721), // in post-eip150
				output: vec![]
			}),
			subtraces: 0,
		}];

		assert_eq!(result.trace, expected_trace);
	}

	#[test]
	fn should_trace_callcode_properly() {
		let _ = env_logger::try_init();

		let mut state = get_temp_state();

		let mut info = EnvInfo::default();
		info.gas_limit = 1_000_000.into();
		let machine = Spec::new_test_machine();

		let t = Transaction {
			nonce: 0.into(),
			gas_price: 0.into(),
			gas: 100_000.into(),
			action: Action::Call(0xa.into()),
			value: 0.into(),
			data: vec![],
		}.sign(&secret(), None);

		state.init_code(&0xa.into(), FromHex::from_hex("60006000600060006000600b611000f2").unwrap()).unwrap();
		state.init_code(&0xb.into(), FromHex::from_hex("6000").unwrap()).unwrap();
		let result = state.apply(&info, &machine, &t, true).unwrap();

		let expected_trace = vec![FlatTrace {
			trace_address: Default::default(),
			subtraces: 1,
			action: trace::Action::Call(trace::Call {
				from: "9cce34f7ab185c7aba1b7c8140d620b4bda941d6".into(),
				to: 0xa.into(),
				value: 0.into(),
				gas: 79000.into(),
				input: vec![],
				call_type: CallType::Call,
			}),
			result: trace::Res::Call(trace::CallResult {
				gas_used: 724.into(), // in post-eip150
				output: vec![]
			}),
		}, FlatTrace {
			trace_address: vec![0].into_iter().collect(),
			subtraces: 0,
			action: trace::Action::Call(trace::Call {
				from: 0xa.into(),
				to: 0xb.into(),
				value: 0.into(),
				gas: 4096.into(),
				input: vec![],
				call_type: CallType::CallCode,
			}),
			result: trace::Res::Call(trace::CallResult {
				gas_used: 3.into(),
				output: vec![],
			}),
		}];

		assert_eq!(result.trace, expected_trace);
	}

	#[test]
	fn should_trace_delegatecall_properly() {
		let _ = env_logger::try_init();

		let mut state = get_temp_state();

		let mut info = EnvInfo::default();
		info.gas_limit = 1_000_000.into();
		info.number = 0x789b0;
		let machine = Spec::new_test_machine();

		let t = Transaction {
			nonce: 0.into(),
			gas_price: 0.into(),
			gas: 100_000.into(),
			action: Action::Call(0xa.into()),
			value: 0.into(),
			data: vec![],
		}.sign(&secret(), None);

		state.init_code(&0xa.into(), FromHex::from_hex("6000600060006000600b618000f4").unwrap()).unwrap();
		state.init_code(&0xb.into(), FromHex::from_hex("60056000526001601ff3").unwrap()).unwrap();
		let result = state.apply(&info, &machine, &t, true).unwrap();

		let expected_trace = vec![FlatTrace {
			trace_address: Default::default(),
			subtraces: 1,
			action: trace::Action::Call(trace::Call {
				from: "9cce34f7ab185c7aba1b7c8140d620b4bda941d6".into(),
				to: 0xa.into(),
				value: 0.into(),
				gas: 79000.into(),
				input: vec![],
				call_type: CallType::Call,
			}),
			result: trace::Res::Call(trace::CallResult {
				gas_used: U256::from(736), // in post-eip150
				output: vec![]
			}),
		}, FlatTrace {
			trace_address: vec![0].into_iter().collect(),
			subtraces: 0,
			action: trace::Action::Call(trace::Call {
				from: 0xa.into(),
				to: 0xb.into(),
				value: 0.into(),
				gas: 32768.into(),
				input: vec![],
				call_type: CallType::DelegateCall,
			}),
			result: trace::Res::Call(trace::CallResult {
				gas_used: 18.into(),
				output: vec![5],
			}),
		}];

		assert_eq!(result.trace, expected_trace);
	}

	#[test]
	fn should_trace_failed_call_transaction() {
		let _ = env_logger::try_init();

		let mut state = get_temp_state();

		let mut info = EnvInfo::default();
		info.gas_limit = 1_000_000.into();
		let machine = make_frontier_machine(5);

		let t = Transaction {
			nonce: 0.into(),
			gas_price: 0.into(),
			gas: 100_000.into(),
			action: Action::Call(0xa.into()),
			value: 100.into(),
			data: vec![],
		}.sign(&secret(), None);

		state.init_code(&0xa.into(), FromHex::from_hex("5b600056").unwrap()).unwrap();
		state.add_balance(&t.sender(), &(100.into()), CleanupMode::NoEmpty).unwrap();
		let result = state.apply(&info, &machine, &t, true).unwrap();
		let expected_trace = vec![FlatTrace {
			trace_address: Default::default(),
			action: trace::Action::Call(trace::Call {
				from: "9cce34f7ab185c7aba1b7c8140d620b4bda941d6".into(),
				to: 0xa.into(),
				value: 100.into(),
				gas: 79000.into(),
				input: vec![],
				call_type: CallType::Call,
			}),
			result: trace::Res::FailedCall(TraceError::OutOfGas),
			subtraces: 0,
		}];

		assert_eq!(result.trace, expected_trace);
	}

	#[test]
	fn should_trace_call_with_subcall_transaction() {
		let _ = env_logger::try_init();

		let mut state = get_temp_state();

		let mut info = EnvInfo::default();
		info.gas_limit = 1_000_000.into();
		let machine = make_frontier_machine(5);

		let t = Transaction {
			nonce: 0.into(),
			gas_price: 0.into(),
			gas: 100_000.into(),
			action: Action::Call(0xa.into()),
			value: 100.into(),
			data: vec![],
		}.sign(&secret(), None);

		state.init_code(&0xa.into(), FromHex::from_hex("60006000600060006000600b602b5a03f1").unwrap()).unwrap();
		state.init_code(&0xb.into(), FromHex::from_hex("6000").unwrap()).unwrap();
		state.add_balance(&t.sender(), &(100.into()), CleanupMode::NoEmpty).unwrap();
		let result = state.apply(&info, &machine, &t, true).unwrap();

		let expected_trace = vec![FlatTrace {
			trace_address: Default::default(),
			subtraces: 1,
			action: trace::Action::Call(trace::Call {
				from: "9cce34f7ab185c7aba1b7c8140d620b4bda941d6".into(),
				to: 0xa.into(),
				value: 100.into(),
				gas: 79000.into(),
				input: vec![],
				call_type: CallType::Call,
			}),
			result: trace::Res::Call(trace::CallResult {
				gas_used: U256::from(69),
				output: vec![]
			}),
		}, FlatTrace {
			trace_address: vec![0].into_iter().collect(),
			subtraces: 0,
			action: trace::Action::Call(trace::Call {
				from: 0xa.into(),
				to: 0xb.into(),
				value: 0.into(),
				gas: 78934.into(),
				input: vec![],
				call_type: CallType::Call,
			}),
			result: trace::Res::Call(trace::CallResult {
				gas_used: U256::from(3),
				output: vec![]
			}),
		}];

		assert_eq!(result.trace, expected_trace);
	}

	#[test]
	fn should_trace_call_with_basic_subcall_transaction() {
		let _ = env_logger::try_init();

		let mut state = get_temp_state();

		let mut info = EnvInfo::default();
		info.gas_limit = 1_000_000.into();
		let machine = make_frontier_machine(5);

		let t = Transaction {
			nonce: 0.into(),
			gas_price: 0.into(),
			gas: 100_000.into(),
			action: Action::Call(0xa.into()),
			value: 100.into(),
			data: vec![],
		}.sign(&secret(), None);

		state.init_code(&0xa.into(), FromHex::from_hex("60006000600060006045600b6000f1").unwrap()).unwrap();
		state.add_balance(&t.sender(), &(100.into()), CleanupMode::NoEmpty).unwrap();
		let result = state.apply(&info, &machine, &t, true).unwrap();
		let expected_trace = vec![FlatTrace {
			trace_address: Default::default(),
			subtraces: 1,
			action: trace::Action::Call(trace::Call {
				from: "9cce34f7ab185c7aba1b7c8140d620b4bda941d6".into(),
				to: 0xa.into(),
				value: 100.into(),
				gas: 79000.into(),
				input: vec![],
				call_type: CallType::Call,
			}),
			result: trace::Res::Call(trace::CallResult {
				gas_used: U256::from(31761),
				output: vec![]
			}),
		}, FlatTrace {
			trace_address: vec![0].into_iter().collect(),
			subtraces: 0,
			action: trace::Action::Call(trace::Call {
				from: 0xa.into(),
				to: 0xb.into(),
				value: 69.into(),
				gas: 2300.into(),
				input: vec![],
				call_type: CallType::Call,
			}),
			result: trace::Res::Call(trace::CallResult::default()),
		}];

		assert_eq!(result.trace, expected_trace);
	}

	#[test]
	fn should_not_trace_call_with_invalid_basic_subcall_transaction() {
		let _ = env_logger::try_init();

		let mut state = get_temp_state();

		let mut info = EnvInfo::default();
		info.gas_limit = 1_000_000.into();
		let machine = make_frontier_machine(5);

		let t = Transaction {
			nonce: 0.into(),
			gas_price: 0.into(),
			gas: 100_000.into(),
			action: Action::Call(0xa.into()),
			value: 100.into(),
			data: vec![],
		}.sign(&secret(), None);

		state.init_code(&0xa.into(), FromHex::from_hex("600060006000600060ff600b6000f1").unwrap()).unwrap();	// not enough funds.
		state.add_balance(&t.sender(), &(100.into()), CleanupMode::NoEmpty).unwrap();
		let result = state.apply(&info, &machine, &t, true).unwrap();
		let expected_trace = vec![FlatTrace {
			trace_address: Default::default(),
			subtraces: 0,
			action: trace::Action::Call(trace::Call {
				from: "9cce34f7ab185c7aba1b7c8140d620b4bda941d6".into(),
				to: 0xa.into(),
				value: 100.into(),
				gas: 79000.into(),
				input: vec![],
				call_type: CallType::Call,
			}),
			result: trace::Res::Call(trace::CallResult {
				gas_used: U256::from(31761),
				output: vec![]
			}),
		}];

		assert_eq!(result.trace, expected_trace);
	}

	#[test]
	fn should_trace_failed_subcall_transaction() {
		let _ = env_logger::try_init();

		let mut state = get_temp_state();

		let mut info = EnvInfo::default();
		info.gas_limit = 1_000_000.into();
		let machine = make_frontier_machine(5);

		let t = Transaction {
			nonce: 0.into(),
			gas_price: 0.into(),
			gas: 100_000.into(),
			action: Action::Call(0xa.into()),
			value: 100.into(),
			data: vec![],//600480600b6000396000f35b600056
		}.sign(&secret(), None);

		state.init_code(&0xa.into(), FromHex::from_hex("60006000600060006000600b602b5a03f1").unwrap()).unwrap();
		state.init_code(&0xb.into(), FromHex::from_hex("5b600056").unwrap()).unwrap();
		state.add_balance(&t.sender(), &(100.into()), CleanupMode::NoEmpty).unwrap();
		let result = state.apply(&info, &machine, &t, true).unwrap();
		let expected_trace = vec![FlatTrace {
			trace_address: Default::default(),
			subtraces: 1,
			action: trace::Action::Call(trace::Call {
				from: "9cce34f7ab185c7aba1b7c8140d620b4bda941d6".into(),
				to: 0xa.into(),
				value: 100.into(),
				gas: 79000.into(),
				input: vec![],
				call_type: CallType::Call,
			}),
			result: trace::Res::Call(trace::CallResult {
				gas_used: U256::from(79_000),
				output: vec![]
			}),
		}, FlatTrace {
			trace_address: vec![0].into_iter().collect(),
			subtraces: 0,
			action: trace::Action::Call(trace::Call {
				from: 0xa.into(),
				to: 0xb.into(),
				value: 0.into(),
				gas: 78934.into(),
				input: vec![],
				call_type: CallType::Call,
			}),
			result: trace::Res::FailedCall(TraceError::OutOfGas),
		}];

		assert_eq!(result.trace, expected_trace);
	}

	#[test]
	fn should_trace_call_with_subcall_with_subcall_transaction() {
		let _ = env_logger::try_init();

		let mut state = get_temp_state();

		let mut info = EnvInfo::default();
		info.gas_limit = 1_000_000.into();
		let machine = make_frontier_machine(5);

		let t = Transaction {
			nonce: 0.into(),
			gas_price: 0.into(),
			gas: 100_000.into(),
			action: Action::Call(0xa.into()),
			value: 100.into(),
			data: vec![],
		}.sign(&secret(), None);

		state.init_code(&0xa.into(), FromHex::from_hex("60006000600060006000600b602b5a03f1").unwrap()).unwrap();
		state.init_code(&0xb.into(), FromHex::from_hex("60006000600060006000600c602b5a03f1").unwrap()).unwrap();
		state.init_code(&0xc.into(), FromHex::from_hex("6000").unwrap()).unwrap();
		state.add_balance(&t.sender(), &(100.into()), CleanupMode::NoEmpty).unwrap();
		let result = state.apply(&info, &machine, &t, true).unwrap();
		let expected_trace = vec![FlatTrace {
			trace_address: Default::default(),
			subtraces: 1,
			action: trace::Action::Call(trace::Call {
				from: "9cce34f7ab185c7aba1b7c8140d620b4bda941d6".into(),
				to: 0xa.into(),
				value: 100.into(),
				gas: 79000.into(),
				input: vec![],
				call_type: CallType::Call,
			}),
			result: trace::Res::Call(trace::CallResult {
				gas_used: U256::from(135),
				output: vec![]
			}),
		}, FlatTrace {
			trace_address: vec![0].into_iter().collect(),
			subtraces: 1,
			action: trace::Action::Call(trace::Call {
				from: 0xa.into(),
				to: 0xb.into(),
				value: 0.into(),
				gas: 78934.into(),
				input: vec![],
				call_type: CallType::Call,
			}),
			result: trace::Res::Call(trace::CallResult {
				gas_used: U256::from(69),
				output: vec![]
			}),
		}, FlatTrace {
			trace_address: vec![0, 0].into_iter().collect(),
			subtraces: 0,
			action: trace::Action::Call(trace::Call {
				from: 0xb.into(),
				to: 0xc.into(),
				value: 0.into(),
				gas: 78868.into(),
				input: vec![],
				call_type: CallType::Call,
			}),
			result: trace::Res::Call(trace::CallResult {
				gas_used: U256::from(3),
				output: vec![]
			}),
		}];

		assert_eq!(result.trace, expected_trace);
	}

	#[test]
	fn should_trace_failed_subcall_with_subcall_transaction() {
		let _ = env_logger::try_init();

		let mut state = get_temp_state();

		let mut info = EnvInfo::default();
		info.gas_limit = 1_000_000.into();
		let machine = make_frontier_machine(5);

		let t = Transaction {
			nonce: 0.into(),
			gas_price: 0.into(),
			gas: 100_000.into(),
			action: Action::Call(0xa.into()),
			value: 100.into(),
			data: vec![],//600480600b6000396000f35b600056
		}.sign(&secret(), None);

		state.init_code(&0xa.into(), FromHex::from_hex("60006000600060006000600b602b5a03f1").unwrap()).unwrap();
		state.init_code(&0xb.into(), FromHex::from_hex("60006000600060006000600c602b5a03f1505b601256").unwrap()).unwrap();
		state.init_code(&0xc.into(), FromHex::from_hex("6000").unwrap()).unwrap();
		state.add_balance(&t.sender(), &(100.into()), CleanupMode::NoEmpty).unwrap();
		let result = state.apply(&info, &machine, &t, true).unwrap();

		let expected_trace = vec![FlatTrace {
			trace_address: Default::default(),
			subtraces: 1,
			action: trace::Action::Call(trace::Call {
				from: "9cce34f7ab185c7aba1b7c8140d620b4bda941d6".into(),
				to: 0xa.into(),
				value: 100.into(),
				gas: 79000.into(),
				input: vec![],
				call_type: CallType::Call,
			}),
			result: trace::Res::Call(trace::CallResult {
				gas_used: U256::from(79_000),
				output: vec![]
			})
		}, FlatTrace {
			trace_address: vec![0].into_iter().collect(),
			subtraces: 1,
				action: trace::Action::Call(trace::Call {
				from: 0xa.into(),
				to: 0xb.into(),
				value: 0.into(),
				gas: 78934.into(),
				input: vec![],
				call_type: CallType::Call,
			}),
			result: trace::Res::FailedCall(TraceError::OutOfGas),
		}, FlatTrace {
			trace_address: vec![0, 0].into_iter().collect(),
			subtraces: 0,
			action: trace::Action::Call(trace::Call {
				from: 0xb.into(),
				to: 0xc.into(),
				value: 0.into(),
				gas: 78868.into(),
				call_type: CallType::Call,
				input: vec![],
			}),
			result: trace::Res::Call(trace::CallResult {
				gas_used: U256::from(3),
				output: vec![]
			}),
		}];

		assert_eq!(result.trace, expected_trace);
	}

	#[test]
	fn should_trace_suicide() {
		let _ = env_logger::try_init();

		let mut state = get_temp_state();

		let mut info = EnvInfo::default();
		info.gas_limit = 1_000_000.into();
		let machine = make_frontier_machine(5);

		let t = Transaction {
			nonce: 0.into(),
			gas_price: 0.into(),
			gas: 100_000.into(),
			action: Action::Call(0xa.into()),
			value: 100.into(),
			data: vec![],
		}.sign(&secret(), None);

		state.init_code(&0xa.into(), FromHex::from_hex("73000000000000000000000000000000000000000bff").unwrap()).unwrap();
		state.add_balance(&0xa.into(), &50.into(), CleanupMode::NoEmpty).unwrap();
		state.add_balance(&t.sender(), &100.into(), CleanupMode::NoEmpty).unwrap();
		let result = state.apply(&info, &machine, &t, true).unwrap();
		let expected_trace = vec![FlatTrace {
			trace_address: Default::default(),
			subtraces: 1,
			action: trace::Action::Call(trace::Call {
				from: "9cce34f7ab185c7aba1b7c8140d620b4bda941d6".into(),
				to: 0xa.into(),
				value: 100.into(),
				gas: 79000.into(),
				input: vec![],
				call_type: CallType::Call,
			}),
			result: trace::Res::Call(trace::CallResult {
				gas_used: 3.into(),
				output: vec![]
			}),
		}, FlatTrace {
			trace_address: vec![0].into_iter().collect(),
			subtraces: 0,
			action: trace::Action::Suicide(trace::Suicide {
				address: 0xa.into(),
				refund_address: 0xb.into(),
				balance: 150.into(),
			}),
			result: trace::Res::None,
		}];

		assert_eq!(result.trace, expected_trace);
	}

	#[test]
	fn code_from_database() {
		let a = Address::zero();
		let (root, db) = {
			let mut state = get_temp_state();
			state.require_or_from(&a, false, || Account::new_contract(42.into(), 0.into(), KECCAK_NULL_RLP), |_|{}).unwrap();
			state.init_code(&a, vec![1, 2, 3]).unwrap();
			assert_eq!(state.code(&a).unwrap(), Some(Arc::new(vec![1u8, 2, 3])));
			state.commit().unwrap();
			assert_eq!(state.code(&a).unwrap(), Some(Arc::new(vec![1u8, 2, 3])));
			state.drop()
		};

		let state = State::from_existing(db, root, U256::from(0u8), Default::default()).unwrap();
		assert_eq!(state.code(&a).unwrap(), Some(Arc::new(vec![1u8, 2, 3])));
	}

	#[test]
	fn storage_at_from_database() {
		let a = Address::zero();
		let (root, db) = {
			let mut state = get_temp_state();
			state.set_storage(&a, H256::from(&U256::from(1u64)), H256::from(&U256::from(69u64))).unwrap();
			state.commit().unwrap();
			state.drop()
		};

		let s = State::from_existing(db, root, U256::from(0u8), Default::default()).unwrap();
		assert_eq!(s.storage_at(&a, &H256::from(&U256::from(1u64))).unwrap(), H256::from(&U256::from(69u64)));
	}

	#[test]
	fn get_from_database() {
		let a = Address::zero();
		let (root, db) = {
			let mut state = get_temp_state();
			state.inc_nonce(&a).unwrap();
			state.add_balance(&a, &U256::from(69u64), CleanupMode::NoEmpty).unwrap();
			state.commit().unwrap();
			assert_eq!(state.balance(&a).unwrap(), U256::from(69u64));
			state.drop()
		};

		let state = State::from_existing(db, root, U256::from(0u8), Default::default()).unwrap();
		assert_eq!(state.balance(&a).unwrap(), U256::from(69u64));
		assert_eq!(state.nonce(&a).unwrap(), U256::from(1u64));
	}

	#[test]
	fn remove() {
		let a = Address::zero();
		let mut state = get_temp_state();
		assert_eq!(state.exists(&a).unwrap(), false);
		assert_eq!(state.exists_and_not_null(&a).unwrap(), false);
		state.inc_nonce(&a).unwrap();
		assert_eq!(state.exists(&a).unwrap(), true);
		assert_eq!(state.exists_and_not_null(&a).unwrap(), true);
		assert_eq!(state.nonce(&a).unwrap(), U256::from(1u64));
		state.kill_account(&a);
		assert_eq!(state.exists(&a).unwrap(), false);
		assert_eq!(state.exists_and_not_null(&a).unwrap(), false);
		assert_eq!(state.nonce(&a).unwrap(), U256::from(0u64));
	}

	#[test]
	fn empty_account_is_not_created() {
		let a = Address::zero();
		let db = get_temp_state_db();
		let (root, db) = {
			let mut state = State::new(db, U256::from(0), Default::default());
			state.add_balance(&a, &U256::default(), CleanupMode::NoEmpty).unwrap(); // create an empty account
			state.commit().unwrap();
			state.drop()
		};
		let state = State::from_existing(db, root, U256::from(0u8), Default::default()).unwrap();
		assert!(!state.exists(&a).unwrap());
		assert!(!state.exists_and_not_null(&a).unwrap());
	}

	#[test]
	fn empty_account_exists_when_creation_forced() {
		let a = Address::zero();
		let db = get_temp_state_db();
		let (root, db) = {
			let mut state = State::new(db, U256::from(0), Default::default());
			state.add_balance(&a, &U256::default(), CleanupMode::ForceCreate).unwrap(); // create an empty account
			state.commit().unwrap();
			state.drop()
		};
		let state = State::from_existing(db, root, U256::from(0u8), Default::default()).unwrap();
		assert!(state.exists(&a).unwrap());
		assert!(!state.exists_and_not_null(&a).unwrap());
	}

	#[test]
	fn remove_from_database() {
		let a = Address::zero();
		let (root, db) = {
			let mut state = get_temp_state();
			state.inc_nonce(&a).unwrap();
			state.commit().unwrap();
			assert_eq!(state.exists(&a).unwrap(), true);
			assert_eq!(state.nonce(&a).unwrap(), U256::from(1u64));
			state.drop()
		};

		let (root, db) = {
			let mut state = State::from_existing(db, root, U256::from(0u8), Default::default()).unwrap();
			assert_eq!(state.exists(&a).unwrap(), true);
			assert_eq!(state.nonce(&a).unwrap(), U256::from(1u64));
			state.kill_account(&a);
			state.commit().unwrap();
			assert_eq!(state.exists(&a).unwrap(), false);
			assert_eq!(state.nonce(&a).unwrap(), U256::from(0u64));
			state.drop()
		};

		let state = State::from_existing(db, root, U256::from(0u8), Default::default()).unwrap();
		assert_eq!(state.exists(&a).unwrap(), false);
		assert_eq!(state.nonce(&a).unwrap(), U256::from(0u64));
	}

	#[test]
	fn alter_balance() {
		let mut state = get_temp_state();
		let a = Address::zero();
		let b = 1u64.into();
		state.add_balance(&a, &U256::from(69u64), CleanupMode::NoEmpty).unwrap();
		assert_eq!(state.balance(&a).unwrap(), U256::from(69u64));
		state.commit().unwrap();
		assert_eq!(state.balance(&a).unwrap(), U256::from(69u64));
		state.sub_balance(&a, &U256::from(42u64), &mut CleanupMode::NoEmpty).unwrap();
		assert_eq!(state.balance(&a).unwrap(), U256::from(27u64));
		state.commit().unwrap();
		assert_eq!(state.balance(&a).unwrap(), U256::from(27u64));
		state.transfer_balance(&a, &b, &U256::from(18u64), CleanupMode::NoEmpty).unwrap();
		assert_eq!(state.balance(&a).unwrap(), U256::from(9u64));
		assert_eq!(state.balance(&b).unwrap(), U256::from(18u64));
		state.commit().unwrap();
		assert_eq!(state.balance(&a).unwrap(), U256::from(9u64));
		assert_eq!(state.balance(&b).unwrap(), U256::from(18u64));
	}

	#[test]
	fn alter_nonce() {
		let mut state = get_temp_state();
		let a = Address::zero();
		state.inc_nonce(&a).unwrap();
		assert_eq!(state.nonce(&a).unwrap(), U256::from(1u64));
		state.inc_nonce(&a).unwrap();
		assert_eq!(state.nonce(&a).unwrap(), U256::from(2u64));
		state.commit().unwrap();
		assert_eq!(state.nonce(&a).unwrap(), U256::from(2u64));
		state.inc_nonce(&a).unwrap();
		assert_eq!(state.nonce(&a).unwrap(), U256::from(3u64));
		state.commit().unwrap();
		assert_eq!(state.nonce(&a).unwrap(), U256::from(3u64));
	}

	#[test]
	fn balance_nonce() {
		let mut state = get_temp_state();
		let a = Address::zero();
		assert_eq!(state.balance(&a).unwrap(), U256::from(0u64));
		assert_eq!(state.nonce(&a).unwrap(), U256::from(0u64));
		state.commit().unwrap();
		assert_eq!(state.balance(&a).unwrap(), U256::from(0u64));
		assert_eq!(state.nonce(&a).unwrap(), U256::from(0u64));
	}

	#[test]
	fn ensure_cached() {
		let mut state = get_temp_state();
		let a = Address::zero();
		state.require(&a, false).unwrap();
		state.commit().unwrap();
		assert_eq!(*state.root(), "0ce23f3c809de377b008a4a3ee94a0834aac8bec1f86e28ffe4fdb5a15b0c785".into());
	}

	#[test]
	fn checkpoint_basic() {
		let mut state = get_temp_state();
		let a = Address::zero();
		state.checkpoint();
		state.add_balance(&a, &U256::from(69u64), CleanupMode::NoEmpty).unwrap();
		assert_eq!(state.balance(&a).unwrap(), U256::from(69u64));
		state.discard_checkpoint();
		assert_eq!(state.balance(&a).unwrap(), U256::from(69u64));
		state.checkpoint();
		state.add_balance(&a, &U256::from(1u64), CleanupMode::NoEmpty).unwrap();
		assert_eq!(state.balance(&a).unwrap(), U256::from(70u64));
		state.revert_to_checkpoint();
		assert_eq!(state.balance(&a).unwrap(), U256::from(69u64));
	}

	#[test]
	fn checkpoint_nested() {
		let mut state = get_temp_state();
		let a = Address::zero();
		state.checkpoint();
		state.checkpoint();
		state.add_balance(&a, &U256::from(69u64), CleanupMode::NoEmpty).unwrap();
		assert_eq!(state.balance(&a).unwrap(), U256::from(69u64));
		state.discard_checkpoint();
		assert_eq!(state.balance(&a).unwrap(), U256::from(69u64));
		state.revert_to_checkpoint();
		assert_eq!(state.balance(&a).unwrap(), U256::from(0));
	}

	#[test]
	fn checkpoint_revert_to_get_storage_at() {
		let mut state = get_temp_state();
		let a = Address::zero();
		let k = H256::from(U256::from(0));

		let c0 = state.checkpoint();
		let c1 = state.checkpoint();
		state.set_storage(&a, k, H256::from(U256::from(1))).unwrap();

		assert_eq!(state.checkpoint_storage_at(c0, &a, &k).unwrap(), Some(H256::from(U256::from(0))));
		assert_eq!(state.checkpoint_storage_at(c1, &a, &k).unwrap(), Some(H256::from(U256::from(0))));
		assert_eq!(state.storage_at(&a, &k).unwrap(), H256::from(U256::from(1)));

		state.revert_to_checkpoint(); // Revert to c1.
		assert_eq!(state.checkpoint_storage_at(c0, &a, &k).unwrap(), Some(H256::from(U256::from(0))));
		assert_eq!(state.storage_at(&a, &k).unwrap(), H256::from(U256::from(0)));
	}

	#[test]
	fn checkpoint_from_empty_get_storage_at() {
		let mut state = get_temp_state();
		let a = Address::zero();
		let k = H256::from(U256::from(0));
		let k2 = H256::from(U256::from(1));

		assert_eq!(state.storage_at(&a, &k).unwrap(), H256::from(U256::from(0)));
		state.clear();

		let c0 = state.checkpoint();
		state.new_contract(&a, U256::zero(), U256::zero()).unwrap();
		let c1 = state.checkpoint();
		state.set_storage(&a, k, H256::from(U256::from(1))).unwrap();
		let c2 = state.checkpoint();
		let c3 = state.checkpoint();
		state.set_storage(&a, k2, H256::from(U256::from(3))).unwrap();
		state.set_storage(&a, k, H256::from(U256::from(3))).unwrap();
		let c4 = state.checkpoint();
		state.set_storage(&a, k, H256::from(U256::from(4))).unwrap();
		let c5 = state.checkpoint();

		assert_eq!(state.checkpoint_storage_at(c0, &a, &k).unwrap(), Some(H256::from(U256::from(0))));
		assert_eq!(state.checkpoint_storage_at(c1, &a, &k).unwrap(), Some(H256::from(U256::from(0))));
		assert_eq!(state.checkpoint_storage_at(c2, &a, &k).unwrap(), Some(H256::from(U256::from(1))));
		assert_eq!(state.checkpoint_storage_at(c3, &a, &k).unwrap(), Some(H256::from(U256::from(1))));
		assert_eq!(state.checkpoint_storage_at(c4, &a, &k).unwrap(), Some(H256::from(U256::from(3))));
		assert_eq!(state.checkpoint_storage_at(c5, &a, &k).unwrap(), Some(H256::from(U256::from(4))));

		state.discard_checkpoint(); // Commit/discard c5.
		assert_eq!(state.checkpoint_storage_at(c0, &a, &k).unwrap(), Some(H256::from(U256::from(0))));
		assert_eq!(state.checkpoint_storage_at(c1, &a, &k).unwrap(), Some(H256::from(U256::from(0))));
		assert_eq!(state.checkpoint_storage_at(c2, &a, &k).unwrap(), Some(H256::from(U256::from(1))));
		assert_eq!(state.checkpoint_storage_at(c3, &a, &k).unwrap(), Some(H256::from(U256::from(1))));
		assert_eq!(state.checkpoint_storage_at(c4, &a, &k).unwrap(), Some(H256::from(U256::from(3))));

		state.revert_to_checkpoint(); // Revert to c4.
		assert_eq!(state.checkpoint_storage_at(c0, &a, &k).unwrap(), Some(H256::from(U256::from(0))));
		assert_eq!(state.checkpoint_storage_at(c1, &a, &k).unwrap(), Some(H256::from(U256::from(0))));
		assert_eq!(state.checkpoint_storage_at(c2, &a, &k).unwrap(), Some(H256::from(U256::from(1))));
		assert_eq!(state.checkpoint_storage_at(c3, &a, &k).unwrap(), Some(H256::from(U256::from(1))));

		state.discard_checkpoint(); // Commit/discard c3.
		assert_eq!(state.checkpoint_storage_at(c0, &a, &k).unwrap(), Some(H256::from(U256::from(0))));
		assert_eq!(state.checkpoint_storage_at(c1, &a, &k).unwrap(), Some(H256::from(U256::from(0))));
		assert_eq!(state.checkpoint_storage_at(c2, &a, &k).unwrap(), Some(H256::from(U256::from(1))));

		state.revert_to_checkpoint(); // Revert to c2.
		assert_eq!(state.checkpoint_storage_at(c0, &a, &k).unwrap(), Some(H256::from(U256::from(0))));
		assert_eq!(state.checkpoint_storage_at(c1, &a, &k).unwrap(), Some(H256::from(U256::from(0))));

		state.discard_checkpoint(); // Commit/discard c1.
		assert_eq!(state.checkpoint_storage_at(c0, &a, &k).unwrap(), Some(H256::from(U256::from(0))));
	}

	#[test]
	fn checkpoint_get_storage_at() {
		let mut state = get_temp_state();
		let a = Address::zero();
		let k = H256::from(U256::from(0));
		let k2 = H256::from(U256::from(1));

		state.set_storage(&a, k, H256::from(U256::from(0xffff))).unwrap();
		state.commit().unwrap();
		state.clear();

		assert_eq!(state.storage_at(&a, &k).unwrap(), H256::from(U256::from(0xffff)));
		state.clear();

		let cm1 = state.checkpoint();
		let c0 = state.checkpoint();
		state.new_contract(&a, U256::zero(), U256::zero()).unwrap();
		let c1 = state.checkpoint();
		state.set_storage(&a, k, H256::from(U256::from(1))).unwrap();
		let c2 = state.checkpoint();
		let c3 = state.checkpoint();
		state.set_storage(&a, k2, H256::from(U256::from(3))).unwrap();
		state.set_storage(&a, k, H256::from(U256::from(3))).unwrap();
		let c4 = state.checkpoint();
		state.set_storage(&a, k, H256::from(U256::from(4))).unwrap();
		let c5 = state.checkpoint();

		assert_eq!(state.checkpoint_storage_at(cm1, &a, &k).unwrap(), Some(H256::from(U256::from(0xffff))));
		assert_eq!(state.checkpoint_storage_at(c0, &a, &k).unwrap(), Some(H256::from(U256::from(0xffff))));
		assert_eq!(state.checkpoint_storage_at(c1, &a, &k).unwrap(), Some(H256::from(U256::from(0))));
		assert_eq!(state.checkpoint_storage_at(c2, &a, &k).unwrap(), Some(H256::from(U256::from(1))));
		assert_eq!(state.checkpoint_storage_at(c3, &a, &k).unwrap(), Some(H256::from(U256::from(1))));
		assert_eq!(state.checkpoint_storage_at(c4, &a, &k).unwrap(), Some(H256::from(U256::from(3))));
		assert_eq!(state.checkpoint_storage_at(c5, &a, &k).unwrap(), Some(H256::from(U256::from(4))));

		state.discard_checkpoint(); // Commit/discard c5.
		assert_eq!(state.checkpoint_storage_at(cm1, &a, &k).unwrap(), Some(H256::from(U256::from(0xffff))));
		assert_eq!(state.checkpoint_storage_at(c0, &a, &k).unwrap(), Some(H256::from(U256::from(0xffff))));
		assert_eq!(state.checkpoint_storage_at(c1, &a, &k).unwrap(), Some(H256::from(U256::from(0))));
		assert_eq!(state.checkpoint_storage_at(c2, &a, &k).unwrap(), Some(H256::from(U256::from(1))));
		assert_eq!(state.checkpoint_storage_at(c3, &a, &k).unwrap(), Some(H256::from(U256::from(1))));
		assert_eq!(state.checkpoint_storage_at(c4, &a, &k).unwrap(), Some(H256::from(U256::from(3))));

		state.revert_to_checkpoint(); // Revert to c4.
		assert_eq!(state.checkpoint_storage_at(cm1, &a, &k).unwrap(), Some(H256::from(U256::from(0xffff))));
		assert_eq!(state.checkpoint_storage_at(c0, &a, &k).unwrap(), Some(H256::from(U256::from(0xffff))));
		assert_eq!(state.checkpoint_storage_at(c1, &a, &k).unwrap(), Some(H256::from(U256::from(0))));
		assert_eq!(state.checkpoint_storage_at(c2, &a, &k).unwrap(), Some(H256::from(U256::from(1))));
		assert_eq!(state.checkpoint_storage_at(c3, &a, &k).unwrap(), Some(H256::from(U256::from(1))));

		state.discard_checkpoint(); // Commit/discard c3.
		assert_eq!(state.checkpoint_storage_at(cm1, &a, &k).unwrap(), Some(H256::from(U256::from(0xffff))));
		assert_eq!(state.checkpoint_storage_at(c0, &a, &k).unwrap(), Some(H256::from(U256::from(0xffff))));
		assert_eq!(state.checkpoint_storage_at(c1, &a, &k).unwrap(), Some(H256::from(U256::from(0))));
		assert_eq!(state.checkpoint_storage_at(c2, &a, &k).unwrap(), Some(H256::from(U256::from(1))));

		state.revert_to_checkpoint(); // Revert to c2.
		assert_eq!(state.checkpoint_storage_at(cm1, &a, &k).unwrap(), Some(H256::from(U256::from(0xffff))));
		assert_eq!(state.checkpoint_storage_at(c0, &a, &k).unwrap(), Some(H256::from(U256::from(0xffff))));
		assert_eq!(state.checkpoint_storage_at(c1, &a, &k).unwrap(), Some(H256::from(U256::from(0))));

		state.discard_checkpoint(); // Commit/discard c1.
		assert_eq!(state.checkpoint_storage_at(cm1, &a, &k).unwrap(), Some(H256::from(U256::from(0xffff))));
		assert_eq!(state.checkpoint_storage_at(c0, &a, &k).unwrap(), Some(H256::from(U256::from(0xffff))));
	}

	#[test]
	fn kill_account_with_checkpoints() {
		let mut state = get_temp_state();
		let a = Address::zero();
		let k = H256::from(U256::from(0));
		state.checkpoint();
		state.set_storage(&a, k, H256::from(U256::from(1))).unwrap();
		state.checkpoint();
		state.kill_account(&a);

		assert_eq!(state.storage_at(&a, &k).unwrap(), H256::from(U256::from(0)));
		state.revert_to_checkpoint();
		assert_eq!(state.storage_at(&a, &k).unwrap(), H256::from(U256::from(1)));
	}

	#[test]
	fn create_contract_fail() {
		let mut state = get_temp_state();
		let orig_root = state.root().clone();
		let a: Address = 1000.into();

		state.checkpoint(); // c1
		state.new_contract(&a, U256::zero(), U256::zero()).unwrap();
		state.add_balance(&a, &U256::from(1), CleanupMode::ForceCreate).unwrap();
		state.checkpoint(); // c2
		state.add_balance(&a, &U256::from(1), CleanupMode::ForceCreate).unwrap();
		state.discard_checkpoint(); // discard c2
		state.revert_to_checkpoint(); // revert to c1
		assert_eq!(state.exists(&a).unwrap(), false);

		state.commit().unwrap();
		assert_eq!(orig_root, state.root().clone());
	}

	#[test]
	fn create_contract_fail_previous_storage() {
		let mut state = get_temp_state();
		let a: Address = 1000.into();
		let k = H256::from(U256::from(0));

		state.set_storage(&a, k, H256::from(U256::from(0xffff))).unwrap();
		state.commit().unwrap();
		state.clear();

		let orig_root = state.root().clone();
		assert_eq!(state.storage_at(&a, &k).unwrap(), H256::from(U256::from(0xffff)));
		state.clear();

		state.checkpoint(); // c1
		state.new_contract(&a, U256::zero(), U256::zero()).unwrap();
		state.checkpoint(); // c2
		state.set_storage(&a, k, H256::from(U256::from(2))).unwrap();
		state.revert_to_checkpoint(); // revert to c2
		assert_eq!(state.storage_at(&a, &k).unwrap(), H256::from(U256::from(0)));
		state.revert_to_checkpoint(); // revert to c1
		assert_eq!(state.storage_at(&a, &k).unwrap(), H256::from(U256::from(0xffff)));

		state.commit().unwrap();
		assert_eq!(orig_root, state.root().clone());
	}

	#[test]
	fn create_empty() {
		let mut state = get_temp_state();
		state.commit().unwrap();
		assert_eq!(*state.root(), "56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421".into());
	}

	#[test]
	fn should_not_panic_on_state_diff_with_storage() {
		let mut state = get_temp_state();

		let a: Address = 0xa.into();
		state.init_code(&a, b"abcdefg".to_vec()).unwrap();;
		state.add_balance(&a, &256.into(), CleanupMode::NoEmpty).unwrap();
		state.set_storage(&a, 0xb.into(), 0xc.into()).unwrap();

		let mut new_state = state.clone();
		new_state.set_storage(&a, 0xb.into(), 0xd.into()).unwrap();

		new_state.diff_from(state).unwrap();
	}

	#[test]
	fn should_kill_garbage() {
		let a = 10.into();
		let b = 20.into();
		let c = 30.into();
		let d = 40.into();
		let e = 50.into();
		let x = 0.into();
		let db = get_temp_state_db();
		let (root, db) = {
			let mut state = State::new(db, U256::from(0), Default::default());
			state.add_balance(&a, &U256::default(), CleanupMode::ForceCreate).unwrap(); // create an empty account
			state.add_balance(&b, &100.into(), CleanupMode::ForceCreate).unwrap(); // create a dust account
			state.add_balance(&c, &101.into(), CleanupMode::ForceCreate).unwrap(); // create a normal account
			state.add_balance(&d, &99.into(), CleanupMode::ForceCreate).unwrap(); // create another dust account
			state.new_contract(&e, 100.into(), 1.into()).unwrap(); // create a contract account
			state.init_code(&e, vec![0x00]).unwrap();
			state.commit().unwrap();
			state.drop()
		};

		let mut state = State::from_existing(db, root, U256::from(0u8), Default::default()).unwrap();
		let mut touched = HashSet::new();
		state.add_balance(&a, &U256::default(), CleanupMode::TrackTouched(&mut touched)).unwrap(); // touch an account
		state.transfer_balance(&b, &x, &1.into(), CleanupMode::TrackTouched(&mut touched)).unwrap(); // touch an account decreasing its balance
		state.transfer_balance(&c, &x, &1.into(), CleanupMode::TrackTouched(&mut touched)).unwrap(); // touch an account decreasing its balance
		state.transfer_balance(&e, &x, &1.into(), CleanupMode::TrackTouched(&mut touched)).unwrap(); // touch an account decreasing its balance
		state.kill_garbage(&touched, true, &None, false).unwrap();
		assert!(!state.exists(&a).unwrap());
		assert!(state.exists(&b).unwrap());
		state.kill_garbage(&touched, true, &Some(100.into()), false).unwrap();
		assert!(!state.exists(&b).unwrap());
		assert!(state.exists(&c).unwrap());
		assert!(state.exists(&d).unwrap());
		assert!(state.exists(&e).unwrap());
		state.kill_garbage(&touched, true, &Some(100.into()), true).unwrap();
		assert!(state.exists(&c).unwrap());
		assert!(state.exists(&d).unwrap());
		assert!(!state.exists(&e).unwrap());
	}

	#[test]
	fn should_trace_diff_suicided_accounts() {
		use pod_account;

		let a = 10.into();
		let db = get_temp_state_db();
		let (root, db) = {
			let mut state = State::new(db, U256::from(0), Default::default());
			state.add_balance(&a, &100.into(), CleanupMode::ForceCreate).unwrap();
			state.commit().unwrap();
			state.drop()
		};

		let mut state = State::from_existing(db, root, U256::from(0u8), Default::default()).unwrap();
		let original = state.clone();
		state.kill_account(&a);

		let diff = state.diff_from(original).unwrap();
		let diff_map = diff.get();
		assert_eq!(diff_map.len(), 1);
		assert!(diff_map.get(&a).is_some());
		assert_eq!(diff_map.get(&a),
				pod_account::diff_pod(Some(&PodAccount {
					balance: U256::from(100),
					nonce: U256::zero(),
					code: Some(Default::default()),
					storage: Default::default()
				}), None).as_ref());
	}

	#[test]
	fn should_trace_diff_unmodified_storage() {
		use pod_account;

		let a = 10.into();
		let db = get_temp_state_db();

		let (root, db) = {
			let mut state = State::new(db, U256::from(0), Default::default());
			state.set_storage(&a, H256::from(&U256::from(1u64)), H256::from(&U256::from(20u64))).unwrap();
			state.commit().unwrap();
			state.drop()
		};

		let mut state = State::from_existing(db, root, U256::from(0u8), Default::default()).unwrap();
		let original = state.clone();
		state.set_storage(&a, H256::from(&U256::from(1u64)), H256::from(&U256::from(100u64))).unwrap();

		let diff = state.diff_from(original).unwrap();
		let diff_map = diff.get();
		assert_eq!(diff_map.len(), 1);
		assert!(diff_map.get(&a).is_some());
		assert_eq!(diff_map.get(&a),
				pod_account::diff_pod(Some(&PodAccount {
					balance: U256::zero(),
					nonce: U256::zero(),
					code: Some(Default::default()),
					storage: vec![(H256::from(&U256::from(1u64)), H256::from(&U256::from(20u64)))]
						.into_iter().collect(),
				}), Some(&PodAccount {
					balance: U256::zero(),
					nonce: U256::zero(),
					code: Some(Default::default()),
					storage: vec![(H256::from(&U256::from(1u64)), H256::from(&U256::from(100u64)))]
						.into_iter().collect(),
				})).as_ref());
	}

	#[cfg(feature="to-pod-full")]
	#[test]
	fn should_get_full_pod_storage_values() {
		use trie::{TrieFactory, TrieSpec};

		let a = 10.into();
		let db = get_temp_state_db();

		let factories = Factories {
			vm: Default::default(),
			trie: TrieFactory::new(TrieSpec::Fat),
			accountdb: Default::default(),
		};

		let get_pod_state_val = |pod_state : &PodState, ak, k| {
			pod_state.get().get(ak).unwrap().storage.get(&k).unwrap().clone()
		};

		let storage_address = H256::from(&U256::from(1u64));

		let (root, db) = {
			let mut state = State::new(db, U256::from(0), factories.clone());
			state.set_storage(&a, storage_address.clone(), H256::from(&U256::from(20u64))).unwrap();
			let dump = state.to_pod_full().unwrap();
			assert_eq!(get_pod_state_val(&dump, &a, storage_address.clone()), H256::from(&U256::from(20u64)));
			state.commit().unwrap();
			let dump = state.to_pod_full().unwrap();
			assert_eq!(get_pod_state_val(&dump, &a, storage_address.clone()), H256::from(&U256::from(20u64)));
			state.drop()
		};

		let mut state = State::from_existing(db, root, U256::from(0u8), factories).unwrap();
		let dump = state.to_pod_full().unwrap();
		assert_eq!(get_pod_state_val(&dump, &a, storage_address.clone()), H256::from(&U256::from(20u64)));
		state.set_storage(&a, storage_address.clone(), H256::from(&U256::from(21u64))).unwrap();
		let dump = state.to_pod_full().unwrap();
		assert_eq!(get_pod_state_val(&dump, &a, storage_address.clone()), H256::from(&U256::from(21u64)));
		state.commit().unwrap();
		state.set_storage(&a, storage_address.clone(), H256::from(&U256::from(0u64))).unwrap();
		let dump = state.to_pod_full().unwrap();
		assert_eq!(get_pod_state_val(&dump, &a, storage_address.clone()), H256::from(&U256::from(0u64)));

	}

}
