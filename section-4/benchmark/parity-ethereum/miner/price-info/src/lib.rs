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

#![warn(missing_docs)]

//! A simple client to get the current ETH price using an external API.

extern crate futures;
extern crate serde_json;
extern crate parity_runtime;

#[macro_use]
extern crate log;

#[cfg(test)]
extern crate fake_fetch;

pub extern crate fetch;

use std::cmp;
use std::fmt;
use std::io;
use std::str;

use fetch::{Client as FetchClient, Fetch};
use futures::{Future, Stream};
use futures::future::{self, Either};
use serde_json::Value;
use parity_runtime::Executor;

/// Current ETH price information.
#[derive(Debug)]
pub struct PriceInfo {
	/// Current ETH price in USD.
	pub ethusd: f32,
}

/// Price info error.
#[derive(Debug)]
pub enum Error {
	/// The API returned an unexpected status code.
	StatusCode(&'static str),
	/// The API returned an unexpected status content.
	UnexpectedResponse(Option<String>),
	/// There was an error when trying to reach the API.
	Fetch(fetch::Error),
	/// IO error when reading API response.
	Io(io::Error),
}

impl From<io::Error> for Error {
	fn from(err: io::Error) -> Self { Error::Io(err) }
}

impl From<fetch::Error> for Error {
	fn from(err: fetch::Error) -> Self { Error::Fetch(err) }
}

/// A client to get the current ETH price using an external API.
pub struct Client<F = FetchClient> {
	pool: Executor,
	api_endpoint: String,
	fetch: F,
}

impl<F> fmt::Debug for Client<F> {
	fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
		fmt.debug_struct("price_info::Client")
		   .field("api_endpoint", &self.api_endpoint)
		   .finish()
	}
}

impl<F> cmp::PartialEq for Client<F> {
	fn eq(&self, other: &Client<F>) -> bool {
		self.api_endpoint == other.api_endpoint
	}
}

impl<F: Fetch> Client<F> {
	/// Creates a new instance of the `Client` given a `fetch::Client`.
	pub fn new(fetch: F, pool: Executor) -> Client<F> {
		let api_endpoint = "https://api.etherscan.io/api?module=stats&action=ethprice".to_owned();
		Client { pool, api_endpoint, fetch }
	}

	/// Gets the current ETH price and calls `set_price` with the result.
	pub fn get<G: FnOnce(PriceInfo) + Sync + Send + 'static>(&self, set_price: G) {
		let future = self.fetch.get(&self.api_endpoint, fetch::Abort::default())
			.from_err()
			.and_then(|response| {
				if !response.is_success() {
					let s = Error::StatusCode(response.status().canonical_reason().unwrap_or("unknown"));
					return Either::A(future::err(s));
				}
				Either::B(response.concat2().from_err())
			})
			.and_then(move |body| {
				let body_str = str::from_utf8(&body).ok();
				let value: Option<Value> = body_str.and_then(|s| serde_json::from_str(s).ok());

				let ethusd = value
					.as_ref()
					.and_then(|value| value.pointer("/result/ethusd"))
					.and_then(|obj| obj.as_str())
					.and_then(|s| s.parse().ok());

				match ethusd {
					Some(ethusd) => {
						set_price(PriceInfo { ethusd });
						Ok(())
					},
					None => Err(Error::UnexpectedResponse(body_str.map(From::from))),
				}
			})
			.map_err(|err| {
				warn!("Failed to auto-update latest ETH price: {:?}", err);
			});
		self.pool.spawn(future)
	}
}

#[cfg(test)]
mod test {
	use std::sync::Arc;
	use parity_runtime::{Runtime, Executor};
	use Client;
	use std::sync::atomic::{AtomicBool, Ordering};
	use fake_fetch::FakeFetch;

	fn price_info_ok(response: &str, executor: Executor) -> Client<FakeFetch<String>> {
		Client::new(FakeFetch::new(Some(response.to_owned())), executor)
	}

	fn price_info_not_found(executor: Executor) -> Client<FakeFetch<String>> {
		Client::new(FakeFetch::new(None::<String>), executor)
	}

	#[test]
	fn should_get_price_info() {
		let runtime = Runtime::with_thread_count(1);

		// given
		let response = r#"{
			"status": "1",
			"message": "OK",
			"result": {
				"ethbtc": "0.0891",
				"ethbtc_timestamp": "1499894236",
				"ethusd": "209.55",
				"ethusd_timestamp": "1499894229"
			}
		}"#;

		let price_info = price_info_ok(response, runtime.executor());

		// when
		price_info.get(|price| {

			// then
			assert_eq!(price.ethusd, 209.55);
		});
	}

	#[test]
	fn should_not_call_set_price_if_response_is_malformed() {
		let runtime = Runtime::with_thread_count(1);

		// given
		let response = "{}";

		let price_info = price_info_ok(response, runtime.executor());
		let b = Arc::new(AtomicBool::new(false));

		// when
		let bb = b.clone();
		price_info.get(move |_| {
			bb.store(true, Ordering::Relaxed);
		});

		// then
		assert_eq!(b.load(Ordering::Relaxed), false);
	}

	#[test]
	fn should_not_call_set_price_if_response_is_invalid() {
		let runtime = Runtime::with_thread_count(1);

		// given
		let price_info = price_info_not_found(runtime.executor());
		let b = Arc::new(AtomicBool::new(false));

		// when
		let bb = b.clone();
		price_info.get(move |_| {
			bb.store(true, Ordering::Relaxed);
		});

		// then
		assert_eq!(b.load(Ordering::Relaxed), false);
	}
}
