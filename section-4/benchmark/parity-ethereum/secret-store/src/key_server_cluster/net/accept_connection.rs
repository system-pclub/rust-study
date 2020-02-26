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

use std::io;
use std::sync::Arc;
use std::net::SocketAddr;
use std::time::Duration;
use futures::{Future, Poll};
use tokio::net::TcpStream;
use key_server_cluster::{Error, NodeKeyPair};
use key_server_cluster::io::{accept_handshake, Handshake, Deadline, deadline};
use key_server_cluster::net::Connection;

/// Create future for accepting incoming connection.
pub fn accept_connection(stream: TcpStream, self_key_pair: Arc<NodeKeyPair>) -> Deadline<AcceptConnection> {
	// TODO: This could fail so it would be better either to accept the
	// address as a separate argument or return a result.
	let address = stream.peer_addr().expect("Unable to determine tcp peer address");

	let accept = AcceptConnection {
		handshake: accept_handshake(stream, self_key_pair),
		address: address,
	};

	deadline(Duration::new(5, 0), accept).expect("Failed to create timeout")
}

/// Future for accepting incoming connection.
pub struct AcceptConnection {
	handshake: Handshake<TcpStream>,
	address: SocketAddr,
}

impl Future for AcceptConnection {
	type Item = Result<Connection, Error>;
	type Error = io::Error;

	fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
		let (stream, result) = try_ready!(self.handshake.poll());
		let result = match result {
			Ok(result) => result,
			Err(err) => return Ok(Err(err).into()),
		};
		let connection = Connection {
			stream: stream.into(),
			address: self.address,
			node_id: result.node_id,
			key: result.shared_key,
		};
		Ok(Ok(connection).into())
	}
}
