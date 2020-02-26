// Copyright 2016 PingCAP, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// See the License for the specific language governing permissions and
// limitations under the License.

//! Coprocessor mainly handles some simple SQL query executors. Most TiDB read queries are processed
//! by Coprocessor instead of KV interface. By doing so, the CPU of TiKV nodes can be utilized for
//! computing and the amount of data to transfer can be reduced (i.e. filtered at TiKV side).
//!
//! Notice that Coprocessor handles more than simple SQL query executors (DAG request). It also
//! handles analyzing requests and checksum requests.
//!
//! The entry point of handling all coprocessor requests is `Endpoint`. Common steps are:
//! 1. Parse the request into a DAG request, Checksum request or Analyze request.
//! 2. Retrieve a snapshot from the underlying engine according to the given timestamp.
//! 3. Build corresponding request handlers from the snapshot and request detail.
//! 4. Run request handlers once (for unary requests) or multiple times (for streaming requests)
//!    on a future thread pool.
//! 5. Return handling result as a response.
//!
//! Please refer to `Endpoint` for more details.

mod checksum;
pub mod codec;
pub mod dag;
mod endpoint;
mod error;
pub mod local_metrics;
mod metrics;
mod readpool_context;
mod statistics;
mod tracker;
pub mod util;

pub use self::endpoint::Endpoint;
pub use self::error::{Error, Result};
pub use self::readpool_context::Context as ReadPoolContext;

use std::boxed::FnBox;

use kvproto::{coprocessor as coppb, kvrpcpb};

use crate::util::time::{Duration, Instant};

pub const REQ_TYPE_DAG: i64 = 103;
pub const REQ_TYPE_ANALYZE: i64 = 104;
pub const REQ_TYPE_CHECKSUM: i64 = 105;

const SINGLE_GROUP: &[u8] = b"SingleGroup";

type HandlerStreamStepResult = Result<(Option<coppb::Response>, bool)>;

/// An interface for all kind of Coprocessor request handlers.
pub trait RequestHandler: Send {
    /// Processes current request and produces a response.
    fn handle_request(&mut self) -> Result<coppb::Response> {
        panic!("unary request is not supported for this handler");
    }

    /// Processes current request and produces streaming responses.
    fn handle_streaming_request(&mut self) -> HandlerStreamStepResult {
        panic!("streaming request is not supported for this handler");
    }

    /// Collects metrics generated in this request handler so far.
    fn collect_metrics_into(&mut self, _metrics: &mut self::dag::executor::ExecutorMetrics) {
        // Do nothing by default
    }

    fn into_boxed(self) -> Box<dyn RequestHandler>
    where
        Self: 'static + Sized,
    {
        Box::new(self)
    }
}

/// Request process dead line.
///
/// When dead line exceeded, the request handling should be stopped.
// TODO: This struct can be removed.
#[derive(Debug, Clone, Copy)]
pub struct Deadline {
    /// Used to construct the Error when deadline exceeded
    tag: &'static str,

    start_time: Instant,
    deadline: Instant,
}

impl Deadline {
    /// Initializes a deadline that counting from current.
    pub fn from_now(tag: &'static str, after_duration: Duration) -> Self {
        let start_time = Instant::now_coarse();
        let deadline = start_time + after_duration;
        Self {
            tag,
            start_time,
            deadline,
        }
    }

    /// Returns error if the deadline is exceeded.
    pub fn check_if_exceeded(&self) -> Result<()> {
        fail_point!("coprocessor_deadline_check_exceeded", |_| Err(
            Error::Outdated(Duration::from_secs(60), self.tag)
        ));

        let now = Instant::now_coarse();
        if self.deadline <= now {
            let elapsed = now.duration_since(self.start_time);
            return Err(Error::Outdated(elapsed, self.tag));
        }
        Ok(())
    }
}

/// Denotes for a function that builds a `RequestHandler`.
/// Due to rust-lang#23856, we have to make it a type alias of `Box<..>`.
type RequestHandlerBuilder<Snap> =
    Box<dyn for<'a> FnBox(Snap, &'a ReqContext) -> Result<Box<dyn RequestHandler>> + Send>;

/// Encapsulate the `kvrpcpb::Context` to provide some extra properties.
#[derive(Debug, Clone)]
pub struct ReqContext {
    /// The tag of the request
    pub tag: &'static str,

    /// The rpc context carried in the request
    pub context: kvrpcpb::Context,

    /// The first range of the request
    pub first_range: Option<coppb::KeyRange>,

    /// The length of the range
    pub ranges_len: usize,

    /// The deadline of the request
    pub deadline: Deadline,

    /// The peer address of the request
    pub peer: Option<String>,

    /// Whether the request is a descending scan (only applicable to DAG)
    pub is_desc_scan: Option<bool>,

    /// The transaction start_ts of the request
    pub txn_start_ts: Option<u64>,
}

impl ReqContext {
    pub fn new(
        tag: &'static str,
        context: kvrpcpb::Context,
        ranges: &[coppb::KeyRange],
        max_handle_duration: Duration,
        peer: Option<String>,
        is_desc_scan: Option<bool>,
        txn_start_ts: Option<u64>,
    ) -> Self {
        let deadline = Deadline::from_now(tag, max_handle_duration);
        Self {
            tag,
            context,
            deadline,
            peer,
            is_desc_scan,
            txn_start_ts,
            first_range: ranges.first().cloned(),
            ranges_len: ranges.len(),
        }
    }

    #[cfg(test)]
    pub fn default_for_test() -> Self {
        Self::new(
            "test",
            kvrpcpb::Context::new(),
            &[],
            Duration::from_secs(100),
            None,
            None,
            None,
        )
    }
}
