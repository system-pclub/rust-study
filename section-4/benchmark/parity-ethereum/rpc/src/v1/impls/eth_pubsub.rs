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

//! Eth PUB-SUB rpc implementation.

use std::sync::{Arc, Weak};
use std::collections::BTreeMap;

use jsonrpc_core::{BoxFuture, Result, Error};
use jsonrpc_core::futures::{self, Future, IntoFuture};
use jsonrpc_pubsub::{SubscriptionId, typed::{Sink, Subscriber}};

use v1::helpers::{errors, limit_logs, Subscribers};
use v1::helpers::light_fetch::LightFetch;
use v1::metadata::Metadata;
use v1::traits::EthPubSub;
use v1::types::{pubsub, RichHeader, Log};

use ethcore::client::{BlockChainClient, ChainNotify, NewBlocks, ChainRouteType, BlockId};
use ethereum_types::H256;
use light::cache::Cache;
use light::client::{LightChainClient, LightChainNotify};
use light::on_demand::OnDemandRequester;
use parity_runtime::Executor;
use parking_lot::{RwLock, Mutex};

use sync::{LightSyncProvider, LightNetworkDispatcher, ManageNetwork};

use types::encoded;
use types::filter::Filter as EthFilter;

type Client = Sink<pubsub::Result>;

/// Eth PubSub implementation.
pub struct EthPubSubClient<C> {
	handler: Arc<ChainNotificationHandler<C>>,
	heads_subscribers: Arc<RwLock<Subscribers<Client>>>,
	logs_subscribers: Arc<RwLock<Subscribers<(Client, EthFilter)>>>,
	transactions_subscribers: Arc<RwLock<Subscribers<Client>>>,
}

impl<C> EthPubSubClient<C> {
	/// Creates new `EthPubSubClient`.
	pub fn new(client: Arc<C>, executor: Executor) -> Self {
		let heads_subscribers = Arc::new(RwLock::new(Subscribers::default()));
		let logs_subscribers = Arc::new(RwLock::new(Subscribers::default()));
		let transactions_subscribers = Arc::new(RwLock::new(Subscribers::default()));

		EthPubSubClient {
			handler: Arc::new(ChainNotificationHandler {
				client,
				executor,
				heads_subscribers: heads_subscribers.clone(),
				logs_subscribers: logs_subscribers.clone(),
				transactions_subscribers: transactions_subscribers.clone(),
			}),
			heads_subscribers,
			logs_subscribers,
			transactions_subscribers,
		}
	}

	/// Creates new `EthPubSubCient` with deterministic subscription ids.
	#[cfg(test)]
	pub fn new_test(client: Arc<C>, executor: Executor) -> Self {
		let client = Self::new(client, executor);
		*client.heads_subscribers.write() = Subscribers::new_test();
		*client.logs_subscribers.write() = Subscribers::new_test();
		*client.transactions_subscribers.write() = Subscribers::new_test();
		client
	}

	/// Returns a chain notification handler.
	pub fn handler(&self) -> Weak<ChainNotificationHandler<C>> {
		Arc::downgrade(&self.handler)
	}
}

impl<S, OD> EthPubSubClient<LightFetch<S, OD>>
where
	S: LightSyncProvider + LightNetworkDispatcher + ManageNetwork + 'static,
	OD: OnDemandRequester + 'static
{
	/// Creates a new `EthPubSubClient` for `LightClient`.
	pub fn light(
		client: Arc<LightChainClient>,
		on_demand: Arc<OD>,
		sync: Arc<S>,
		cache: Arc<Mutex<Cache>>,
		executor: Executor,
		gas_price_percentile: usize,
	) -> Self {
		let fetch = LightFetch {
			client,
			on_demand,
			sync,
			cache,
			gas_price_percentile,
		};
		EthPubSubClient::new(Arc::new(fetch), executor)
	}
}

/// PubSub Notification handler.
pub struct ChainNotificationHandler<C> {
	client: Arc<C>,
	executor: Executor,
	heads_subscribers: Arc<RwLock<Subscribers<Client>>>,
	logs_subscribers: Arc<RwLock<Subscribers<(Client, EthFilter)>>>,
	transactions_subscribers: Arc<RwLock<Subscribers<Client>>>,
}

impl<C> ChainNotificationHandler<C> {
	fn notify(executor: &Executor, subscriber: &Client, result: pubsub::Result) {
		executor.spawn(subscriber
			.notify(Ok(result))
			.map(|_| ())
			.map_err(|e| warn!(target: "rpc", "Unable to send notification: {}", e))
		);
	}

	fn notify_heads(&self, headers: &[(encoded::Header, BTreeMap<String, String>)]) {
		for subscriber in self.heads_subscribers.read().values() {
			for &(ref header, ref extra_info) in headers {
				Self::notify(&self.executor, subscriber, pubsub::Result::Header(Box::new(RichHeader {
					inner: header.into(),
					extra_info: extra_info.clone(),
				})));
			}
		}
	}

	fn notify_logs<F, T, Ex>(&self, enacted: &[(H256, Ex)], logs: F) where
		F: Fn(EthFilter, &Ex) -> T,
		Ex: Send,
		T: IntoFuture<Item = Vec<Log>, Error = Error>,
		T::Future: Send + 'static,
	{
		for &(ref subscriber, ref filter) in self.logs_subscribers.read().values() {
			let logs = futures::future::join_all(enacted
				.iter()
				.map(|&(hash, ref ex)| {
					let mut filter = filter.clone();
					filter.from_block = BlockId::Hash(hash);
					filter.to_block = filter.from_block;
					logs(filter, ex).into_future()
				})
				.collect::<Vec<_>>()
			);
			let limit = filter.limit;
			let executor = self.executor.clone();
			let subscriber = subscriber.clone();
			self.executor.spawn(logs
				.map(move |logs| {
					let logs = logs.into_iter().flat_map(|log| log).collect();

					for log in limit_logs(logs, limit) {
						Self::notify(&executor, &subscriber, pubsub::Result::Log(Box::new(log)))
					}
				})
				.map_err(|e| warn!("Unable to fetch latest logs: {:?}", e))
			);
		}
	}

	/// Notify all subscribers about new transaction hashes.
	pub fn notify_new_transactions(&self, hashes: &[H256]) {
		for subscriber in self.transactions_subscribers.read().values() {
			for hash in hashes {
				Self::notify(&self.executor, subscriber, pubsub::Result::TransactionHash(*hash));
			}
		}
	}
}

/// A light client wrapper struct.
pub trait LightClient: Send + Sync {
	/// Get a recent block header.
	fn block_header(&self, id: BlockId) -> Option<encoded::Header>;

	/// Fetch logs.
	fn logs(&self, filter: EthFilter) -> BoxFuture<Vec<Log>>;
}

impl<S, OD> LightClient for LightFetch<S, OD>
where
	S: LightSyncProvider + LightNetworkDispatcher + ManageNetwork + 'static,
	OD: OnDemandRequester + 'static
{
	fn block_header(&self, id: BlockId) -> Option<encoded::Header> {
		self.client.block_header(id)
	}

	fn logs(&self, filter: EthFilter) -> BoxFuture<Vec<Log>> {
		Box::new(LightFetch::logs(self, filter)) as BoxFuture<_>
	}
}

impl<C: LightClient> LightChainNotify for ChainNotificationHandler<C> {
	fn new_headers(&self, enacted: &[H256]) {
		let headers = enacted
			.iter()
			.filter_map(|hash| self.client.block_header(BlockId::Hash(*hash)))
			.map(|header| (header, Default::default()))
			.collect::<Vec<_>>();

		self.notify_heads(&headers);
		self.notify_logs(&enacted.iter().map(|h| (*h, ())).collect::<Vec<_>>(), |filter, _| self.client.logs(filter))
	}
}

impl<C: BlockChainClient> ChainNotify for ChainNotificationHandler<C> {
	fn new_blocks(&self, new_blocks: NewBlocks) {
		if self.heads_subscribers.read().is_empty() && self.logs_subscribers.read().is_empty() { return }
		const EXTRA_INFO_PROOF: &str = "Object exists in in blockchain (fetched earlier), extra_info is always available if object exists; qed";
		let headers = new_blocks.route.route()
			.iter()
			.filter_map(|&(hash, ref typ)| {
				match typ {
					ChainRouteType::Retracted => None,
					ChainRouteType::Enacted => self.client.block_header(BlockId::Hash(hash))
				}
			})
			.map(|header| {
				let hash = header.hash();
				(header, self.client.block_extra_info(BlockId::Hash(hash)).expect(EXTRA_INFO_PROOF))
			})
			.collect::<Vec<_>>();

		// Headers
		self.notify_heads(&headers);

		// We notify logs enacting and retracting as the order in route.
		self.notify_logs(new_blocks.route.route(), |filter, ex| {
			match ex {
				ChainRouteType::Enacted =>
					Ok(self.client.logs(filter).unwrap_or_default().into_iter().map(Into::into).collect()),
				ChainRouteType::Retracted =>
					Ok(self.client.logs(filter).unwrap_or_default().into_iter().map(Into::into).map(|mut log: Log| {
						log.log_type = "removed".into();
						log.removed = true;
						log
					}).collect()),
			}
		});
	}
}

impl<C: Send + Sync + 'static> EthPubSub for EthPubSubClient<C> {
	type Metadata = Metadata;

	fn subscribe(
		&self,
		_meta: Metadata,
		subscriber: Subscriber<pubsub::Result>,
		kind: pubsub::Kind,
		params: Option<pubsub::Params>,
	) {
		let error = match (kind, params) {
			(pubsub::Kind::NewHeads, None) => {
				self.heads_subscribers.write().push(subscriber);
				return;
			},
			(pubsub::Kind::NewHeads, _) => {
				errors::invalid_params("newHeads", "Expected no parameters.")
			},
			(pubsub::Kind::Logs, Some(pubsub::Params::Logs(filter))) => {
				match filter.try_into() {
					Ok(filter) => {
						self.logs_subscribers.write().push(subscriber, filter);
						return;
					},
					Err(err) => err,
				}
			},
			(pubsub::Kind::Logs, _) => {
				errors::invalid_params("logs", "Expected a filter object.")
			},
			(pubsub::Kind::NewPendingTransactions, None) => {
				self.transactions_subscribers.write().push(subscriber);
				return;
			},
			(pubsub::Kind::NewPendingTransactions, _) => {
				errors::invalid_params("newPendingTransactions", "Expected no parameters.")
			},
			_ => {
				errors::unimplemented(None)
			},
		};

		let _ = subscriber.reject(error);
	}

	fn unsubscribe(&self, _: Option<Self::Metadata>, id: SubscriptionId) -> Result<bool> {
		let res = self.heads_subscribers.write().remove(&id).is_some();
		let res2 = self.logs_subscribers.write().remove(&id).is_some();
		let res3 = self.transactions_subscribers.write().remove(&id).is_some();

		Ok(res || res2 || res3)
	}
}
