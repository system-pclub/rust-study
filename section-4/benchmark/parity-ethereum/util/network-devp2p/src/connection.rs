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

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::time::Duration;
use hash::{keccak, write_keccak};
use mio::{Token, Ready, PollOpt};
use mio::deprecated::{Handler, EventLoop, TryRead, TryWrite};
use mio::tcp::*;
use ethereum_types::{H128, H256, H512};
use parity_bytes::*;
use rlp::{Rlp, RlpStream};
use std::io::{self, Cursor, Read, Write};
use io::{IoContext, StreamToken};
use handshake::Handshake;
use rcrypto::blockmodes::*;
use rcrypto::aessafe::*;
use rcrypto::symmetriccipher::*;
use rcrypto::buffer::*;
use tiny_keccak::Keccak;
use bytes::{Buf, BufMut};
use ethkey::crypto;
use network::{Error, ErrorKind};

const ENCRYPTED_HEADER_LEN: usize = 32;
const RECEIVE_PAYLOAD: Duration = Duration::from_secs(30);
pub const MAX_PAYLOAD_SIZE: usize = (1 << 24) - 1;

/// Network responses should try not to go over this limit.
/// This should be lower than MAX_PAYLOAD_SIZE
pub const PAYLOAD_SOFT_LIMIT: usize = (1 << 22) - 1;

pub trait GenericSocket : Read + Write {
}

impl GenericSocket for TcpStream {
}

pub struct GenericConnection<Socket: GenericSocket> {
	/// Connection id (token)
	pub token: StreamToken,
	/// Network socket
	pub socket: Socket,
	/// Receive buffer
	rec_buf: Bytes,
	/// Expected size
	rec_size: usize,
	/// Send out packets FIFO
	send_queue: VecDeque<Cursor<Bytes>>,
	/// Event flags this connection expects
	interest: Ready,
	/// Registered flag
	registered: AtomicBool,
}

impl<Socket: GenericSocket> GenericConnection<Socket> {
	pub fn expect(&mut self, size: usize) {
		trace!(target:"network", "Expect to read {} bytes", size);
		if self.rec_size != self.rec_buf.len() {
			warn!(target:"network", "Unexpected connection read start");
		}
		self.rec_size = size;
	}

	/// Readable IO handler. Called when there is some data to be read.
	pub fn readable(&mut self) -> io::Result<Option<Bytes>> {
		if self.rec_size == 0 || self.rec_buf.len() >= self.rec_size {
			return Ok(None);
		}
		let sock_ref = <Socket as Read>::by_ref(&mut self.socket);
		loop {
			let max = self.rec_size - self.rec_buf.len();
			match sock_ref.take(max as u64).try_read(unsafe { self.rec_buf.bytes_mut() }) {
				Ok(Some(size)) if size != 0  => {
					unsafe { self.rec_buf.advance_mut(size); }
					trace!(target:"network", "{}: Read {} of {} bytes", self.token, self.rec_buf.len(), self.rec_size);
					if self.rec_size != 0 && self.rec_buf.len() == self.rec_size {
						self.rec_size = 0;
						return Ok(Some(::std::mem::replace(&mut self.rec_buf, Bytes::new())))
					}
					else if self.rec_buf.len() > self.rec_size {
						warn!(target:"network", "Read past buffer {} bytes", self.rec_buf.len() - self.rec_size);
						return Ok(Some(::std::mem::replace(&mut self.rec_buf, Bytes::new())))
                    }
				},
				Ok(_) => return Ok(None),
				Err(e) => {
					debug!(target:"network", "Read error {} ({})", self.token, e);
					return Err(e)
				}
			}
        }
	}

	/// Add a packet to send queue.
	pub fn send<Message>(&mut self, io: &IoContext<Message>, data: Bytes) where Message: Send + Clone + Sync + 'static {
		if !data.is_empty() {
			trace!(target:"network", "{}: Sending {} bytes", self.token, data.len());
			self.send_queue.push_back(Cursor::new(data));
			if !self.interest.is_writable() {
				self.interest.insert(Ready::writable());
			}
			io.update_registration(self.token).ok();
		}
	}

	/// Check if this connection has data to be sent.
	pub fn is_sending(&self) -> bool {
		self.interest.is_writable()
	}

	/// Writable IO handler. Called when the socket is ready to send.
	pub fn writable<Message>(&mut self, io: &IoContext<Message>) -> Result<WriteStatus, Error> where Message: Send + Clone + Sync + 'static {
		{
			let buf = match self.send_queue.front_mut() {
				Some(buf) => buf,
				None => return Ok(WriteStatus::Complete),
			};
			let send_size = buf.get_ref().len();
			let pos = buf.position() as usize;
			if (pos as usize) >= send_size {
				warn!(target:"net", "Unexpected connection data");
				return Ok(WriteStatus::Complete)
			}

			match self.socket.try_write(Buf::bytes(&buf)) {
				Ok(Some(size)) if (pos + size) < send_size => {
					buf.advance(size);
					Ok(WriteStatus::Ongoing)
				},
				Ok(Some(size)) if (pos + size) == send_size => {
					trace!(target:"network", "{}: Wrote {} bytes", self.token, send_size);
					Ok(WriteStatus::Complete)
				},
				Ok(Some(_)) => { panic!("Wrote past buffer");},
				Ok(None) => Ok(WriteStatus::Ongoing),
				Err(e) => Err(e)?
			}
		}.and_then(|r| {
			if r == WriteStatus::Complete {
				self.send_queue.pop_front();
			}
			if self.send_queue.is_empty() {
				self.interest.remove(Ready::writable());
			}
			io.update_registration(self.token)?;
			Ok(r)
		})
	}
}

/// Low level tcp connection
pub type Connection = GenericConnection<TcpStream>;

impl Connection {
	/// Create a new connection with given id and socket.
	pub fn new(token: StreamToken, socket: TcpStream) -> Connection {
		Connection {
			token,
			socket,
			send_queue: VecDeque::new(),
			rec_buf: Bytes::new(),
			rec_size: 0,
			interest: Ready::hup() | Ready::readable(),
			registered: AtomicBool::new(false),
		}
	}

	/// Get socket token
	pub fn token(&self) -> StreamToken {
		self.token
	}

	/// Get remote peer address
	pub fn remote_addr(&self) -> io::Result<SocketAddr> {
		self.socket.peer_addr()
	}

	/// Get remote peer address string
	pub fn remote_addr_str(&self) -> String {
		self.socket.peer_addr().map(|a| a.to_string()).unwrap_or_else(|_| "Unknown".to_owned())
	}

	/// Get local peer address string
	pub fn local_addr_str(&self) -> String {
		self.socket.local_addr().map(|a| a.to_string()).unwrap_or_else(|_| "Unknown".to_owned())
	}

	/// Clone this connection. Clears the receiving buffer of the returned connection.
	pub fn try_clone(&self) -> io::Result<Self> {
		Ok(Connection {
			token: self.token,
			socket: self.socket.try_clone()?,
			rec_buf: Vec::new(),
			rec_size: 0,
			send_queue: self.send_queue.clone(),
			interest: Ready::hup(),
			registered: AtomicBool::new(false),
		})
	}

	/// Register this connection with the IO event loop.
	pub fn register_socket<Host: Handler>(&self, reg: Token, event_loop: &mut EventLoop<Host>) -> io::Result<()> {
		if self.registered.load(AtomicOrdering::SeqCst) {
			return Ok(());
        }
		trace!(target: "network", "connection register; token={:?}", reg);
		if let Err(e) = event_loop.register(&self.socket, reg, self.interest, PollOpt::edge() /* | PollOpt::oneshot() */) { // TODO: oneshot is broken on windows
			trace!(target: "network", "Failed to register {:?}, {:?}", reg, e);
		}
		self.registered.store(true, AtomicOrdering::SeqCst);
		Ok(())
	}

	/// Update connection registration. Should be called at the end of the IO handler.
	pub fn update_socket<Host: Handler>(&self, reg: Token, event_loop: &mut EventLoop<Host>) -> io::Result<()> {
		trace!(target: "network", "connection reregister; token={:?}", reg);
		if !self.registered.load(AtomicOrdering::SeqCst) {
			self.register_socket(reg, event_loop)
        } else {
			event_loop.reregister(&self.socket, reg, self.interest, PollOpt::edge() /* | PollOpt::oneshot() */ ).unwrap_or_else(|e| {  // TODO: oneshot is broken on windows
				trace!(target: "network", "Failed to reregister {:?}, {:?}", reg, e);
			});
			Ok(())
		}
	}

	/// Delete connection registration. Should be called at the end of the IO handler.
	pub fn deregister_socket<Host: Handler>(&self, event_loop: &mut EventLoop<Host>) -> io::Result<()> {
		trace!(target: "network", "connection deregister; token={:?}", self.token);
		event_loop.deregister(&self.socket).ok(); // ignore errors here
		Ok(())
	}
}

/// Connection write status.
#[derive(PartialEq, Eq)]
pub enum WriteStatus {
	/// Some data is still pending for current packet
	Ongoing,
	/// All data sent.
	Complete
}

/// `RLPx` packet
pub struct Packet {
	pub protocol: u16,
	pub data: Bytes,
}

/// Encrypted connection receiving state.
enum EncryptedConnectionState {
	/// Reading a header.
	Header,
	/// Reading the rest of the packet.
	Payload,
}

/// Connection implementing `RLPx` framing
/// https://github.com/ethereum/devp2p/blob/master/rlpx.md#framing
pub struct EncryptedConnection {
	/// Underlying tcp connection
	pub connection: Connection,
	/// Egress data encryptor
	encoder: CtrMode<AesSafe256Encryptor>,
	/// Ingress data decryptor
	decoder: CtrMode<AesSafe256Encryptor>,
	/// Ingress data decryptor
	mac_encoder: EcbEncryptor<AesSafe256Encryptor, EncPadding<NoPadding>>,
	/// MAC for egress data
	egress_mac: Keccak,
	/// MAC for ingress data
	ingress_mac: Keccak,
	/// Read state
	read_state: EncryptedConnectionState,
	/// Protocol id for the last received packet
	protocol_id: u16,
	/// Payload expected to be received for the last header.
	payload_len: usize,
}

impl EncryptedConnection {
	/// Create an encrypted connection out of the handshake.
	pub fn new(handshake: &mut Handshake) -> Result<EncryptedConnection, Error> {
		let shared = crypto::ecdh::agree(handshake.ecdhe.secret(), &handshake.remote_ephemeral)?;
		let mut nonce_material = H512::new();
		if handshake.originated {
			handshake.remote_nonce.copy_to(&mut nonce_material[0..32]);
			handshake.nonce.copy_to(&mut nonce_material[32..64]);
		}
		else {
			handshake.nonce.copy_to(&mut nonce_material[0..32]);
			handshake.remote_nonce.copy_to(&mut nonce_material[32..64]);
		}
		let mut key_material = H512::new();
		shared.copy_to(&mut key_material[0..32]);
		write_keccak(&nonce_material, &mut key_material[32..64]);
		keccak(&key_material).copy_to(&mut key_material[32..64]);
		keccak(&key_material).copy_to(&mut key_material[32..64]);

		let iv = vec![0u8; 16];
		let encoder = CtrMode::new(AesSafe256Encryptor::new(&key_material[32..64]), iv);
		let iv = vec![0u8; 16];
		let decoder = CtrMode::new(AesSafe256Encryptor::new(&key_material[32..64]), iv);

		keccak(&key_material).copy_to(&mut key_material[32..64]);
		let mac_encoder = EcbEncryptor::new(AesSafe256Encryptor::new(&key_material[32..64]), NoPadding);

		let mut egress_mac = Keccak::new_keccak256();
		let mut mac_material = H256::from_slice(&key_material[32..64]) ^ handshake.remote_nonce;
		egress_mac.update(&mac_material);
		egress_mac.update(if handshake.originated { &handshake.auth_cipher } else { &handshake.ack_cipher });

		let mut ingress_mac = Keccak::new_keccak256();
		mac_material = H256::from_slice(&key_material[32..64]) ^ handshake.nonce;
		ingress_mac.update(&mac_material);
		ingress_mac.update(if handshake.originated { &handshake.ack_cipher } else { &handshake.auth_cipher });

		let old_connection = handshake.connection.try_clone()?;
		let connection = ::std::mem::replace(&mut handshake.connection, old_connection);
		let mut enc = EncryptedConnection {
			connection,
			encoder,
			decoder,
			mac_encoder,
			egress_mac,
			ingress_mac,
			read_state: EncryptedConnectionState::Header,
			protocol_id: 0,
			payload_len: 0,
		};
		enc.connection.expect(ENCRYPTED_HEADER_LEN);
		Ok(enc)
	}

	/// Send a packet
	pub fn send_packet<Message>(&mut self, io: &IoContext<Message>, payload: &[u8]) -> Result<(), Error> where Message: Send + Clone + Sync + 'static {
		let mut header = RlpStream::new();
		let len = payload.len();
		if len > MAX_PAYLOAD_SIZE {
			bail!(ErrorKind::OversizedPacket);
		}
		header.append_raw(&[(len >> 16) as u8, (len >> 8) as u8, len as u8], 1);
		header.append_raw(&[0xc2u8, 0x80u8, 0x80u8], 1);
		//TODO: get rid of vectors here
		let mut header = header.out();
		let padding = (16 - (payload.len() % 16)) % 16;
		header.resize(16, 0u8);

		let mut packet = vec![0u8; 32 + payload.len() + padding + 16];
		self.encoder.encrypt(&mut RefReadBuffer::new(&header), &mut RefWriteBuffer::new(&mut packet), false).expect("Invalid length or padding");
		EncryptedConnection::update_mac(&mut self.egress_mac, &mut self.mac_encoder,  &packet[0..16]);
		self.egress_mac.clone().finalize(&mut packet[16..32]);
		self.encoder.encrypt(&mut RefReadBuffer::new(payload), &mut RefWriteBuffer::new(&mut packet[32..(32 + len)]), padding == 0).expect("Invalid length or padding");
		if padding != 0 {
			let pad = [0u8; 16];
			self.encoder.encrypt(&mut RefReadBuffer::new(&pad[0..padding]), &mut RefWriteBuffer::new(&mut packet[(32 + len)..(32 + len + padding)]), true).expect("Invalid length or padding");
		}
		self.egress_mac.update(&packet[32..(32 + len + padding)]);
		EncryptedConnection::update_mac(&mut self.egress_mac, &mut self.mac_encoder, &[0u8; 0]);
		self.egress_mac.clone().finalize(&mut packet[(32 + len + padding)..]);
		self.connection.send(io, packet);
		Ok(())
	}

	/// Decrypt and authenticate an incoming packet header. Prepare for receiving payload.
	fn read_header(&mut self, header: &[u8]) -> Result<(), Error> {
		if header.len() != ENCRYPTED_HEADER_LEN {
			return Err(ErrorKind::Auth.into());
		}
		EncryptedConnection::update_mac(&mut self.ingress_mac, &mut self.mac_encoder, &header[0..16]);
		let mac = &header[16..];
		let mut expected = H256::new();
		self.ingress_mac.clone().finalize(&mut expected);
		if mac != &expected[0..16] {
			return Err(ErrorKind::Auth.into());
		}

		let mut hdec = H128::new();
		self.decoder.decrypt(&mut RefReadBuffer::new(&header[0..16]), &mut RefWriteBuffer::new(&mut hdec), false).expect("Invalid length or padding");

		let length = ((((hdec[0] as u32) << 8) + (hdec[1] as u32)) << 8) + (hdec[2] as u32);
		let header_rlp = Rlp::new(&hdec[3..6]);
		let protocol_id = header_rlp.val_at::<u16>(0)?;

		self.payload_len = length as usize;
		self.protocol_id = protocol_id;
		self.read_state = EncryptedConnectionState::Payload;

		let padding = (16 - (length % 16)) % 16;
		let full_length = length + padding + 16;
		self.connection.expect(full_length as usize);
		Ok(())
	}

	/// Decrypt and authenticate packet payload.
	fn read_payload(&mut self, payload: &[u8]) -> Result<Packet, Error> {
		let padding = (16 - (self.payload_len  % 16)) % 16;
		let full_length = self.payload_len + padding + 16;
		if payload.len() != full_length {
			return Err(ErrorKind::Auth.into());
		}
		self.ingress_mac.update(&payload[0..payload.len() - 16]);
		EncryptedConnection::update_mac(&mut self.ingress_mac, &mut self.mac_encoder, &[0u8; 0]);
		let mac = &payload[(payload.len() - 16)..];
		let mut expected = H128::new();
		self.ingress_mac.clone().finalize(&mut expected);
		if mac != &expected[..] {
			return Err(ErrorKind::Auth.into());
		}

		let mut packet = vec![0u8; self.payload_len];
		self.decoder.decrypt(&mut RefReadBuffer::new(&payload[0..self.payload_len]), &mut RefWriteBuffer::new(&mut packet), false).expect("Invalid length or padding");
		let mut pad_buf = [0u8; 16];
		self.decoder.decrypt(&mut RefReadBuffer::new(&payload[self.payload_len..(payload.len() - 16)]), &mut RefWriteBuffer::new(&mut pad_buf), false).expect("Invalid length or padding");
		Ok(Packet {
			protocol: self.protocol_id,
			data: packet
		})
	}

	/// Update MAC after reading or writing any data.
	fn update_mac(mac: &mut Keccak, mac_encoder: &mut EcbEncryptor<AesSafe256Encryptor, EncPadding<NoPadding>>, seed: &[u8]) {
		let mut prev = H128::new();
		mac.clone().finalize(&mut prev);
		let mut enc = H128::new();
		mac_encoder.encrypt(&mut RefReadBuffer::new(&prev), &mut RefWriteBuffer::new(&mut enc), true).expect("Error updating MAC");
		mac_encoder.reset();

		enc = enc ^ if seed.is_empty() { prev } else { H128::from_slice(seed) };
		mac.update(&enc);
	}

	/// Readable IO handler. Tracker receive status and returns decoded packet if available.
	pub fn readable<Message>(&mut self, io: &IoContext<Message>) -> Result<Option<Packet>, Error> where Message: Send + Clone + Sync + 'static {
		io.clear_timer(self.connection.token)?;
		if let EncryptedConnectionState::Header = self.read_state {
			if let Some(data) = self.connection.readable()? {
				self.read_header(&data)?;
				io.register_timer(self.connection.token, RECEIVE_PAYLOAD)?;
			}
		};
		if let EncryptedConnectionState::Payload = self.read_state {
			match self.connection.readable()? {
				Some(data) => {
					self.read_state = EncryptedConnectionState::Header;
					self.connection.expect(ENCRYPTED_HEADER_LEN);
					Ok(Some(self.read_payload(&data)?))
				},
				None => Ok(None)
			}
		} else {
			Ok(None)
		}
	}

	/// Writable IO handler. Processes send queue.
	pub fn writable<Message>(&mut self, io: &IoContext<Message>) -> Result<(), Error> where Message: Send + Clone + Sync + 'static {
		self.connection.writable(io)?;
		Ok(())
	}
}

#[test]
pub fn test_encryption() {
	use ethereum_types::{H256, H128};
	use std::str::FromStr;
	let key = H256::from_str("2212767d793a7a3d66f869ae324dd11bd17044b82c9f463b8a541a4d089efec5").unwrap();
	let before = H128::from_str("12532abaec065082a3cf1da7d0136f15").unwrap();
	let before2 = H128::from_str("7e99f682356fdfbc6b67a9562787b18a").unwrap();
	let after = H128::from_str("89464c6b04e7c99e555c81d3f7266a05").unwrap();
	let after2 = H128::from_str("85c070030589ef9c7a2879b3a8489316").unwrap();

	let mut got = H128::new();

	let mut encoder = EcbEncryptor::new(AesSafe256Encryptor::new(&key), NoPadding);
	encoder.encrypt(&mut RefReadBuffer::new(&before), &mut RefWriteBuffer::new(&mut got), true).unwrap();
	encoder.reset();
	assert_eq!(got, after);
	got = H128::new();
	encoder.encrypt(&mut RefReadBuffer::new(&before2), &mut RefWriteBuffer::new(&mut got), true).unwrap();
	encoder.reset();
	assert_eq!(got, after2);
}

#[cfg(test)]
mod tests {
	use std::cmp;
	use std::collections::VecDeque;
	use std::io::{Read, Write, Cursor, ErrorKind, Result, Error};
	use std::sync::atomic::AtomicBool;

	use mio::{Ready};
	use parity_bytes::Bytes;
	use io::*;
	use super::*;

	pub struct TestSocket {
		pub read_buffer: Vec<u8>,
		pub write_buffer: Vec<u8>,
		pub cursor: usize,
		pub buf_size: usize,
	}

	impl Default for TestSocket {
		fn default() -> Self {
			TestSocket::new()
		}
	}

	impl TestSocket {
		pub fn new() -> Self {
			TestSocket {
				read_buffer: vec![],
				write_buffer: vec![],
				cursor: 0,
				buf_size: 0,
			}
		}

		pub fn new_buf(buf_size: usize) -> TestSocket {
			TestSocket {
				read_buffer: vec![],
				write_buffer: vec![],
				cursor: 0,
				buf_size,
			}
		}
	}

	impl Read for TestSocket {
		fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
			let end_position = cmp::min(self.read_buffer.len(), self.cursor+buf.len());
			if self.cursor > end_position { return Ok(0) }
			let len = cmp::max(end_position - self.cursor, 0);
			match len {
				0 => Ok(0),
				_ => {
					for i in self.cursor..end_position {
						buf[i-self.cursor] = self.read_buffer[i];
					}
					self.cursor = end_position;
					Ok(len)
				}
			}
		}
	}

	impl Write for TestSocket {
		fn write(&mut self, buf: &[u8]) -> Result<usize> {
			if self.buf_size == 0 || buf.len() < self.buf_size {
				self.write_buffer.extend(buf.iter().cloned());
				Ok(buf.len())
			}
			else {
				self.write_buffer.extend(buf.iter().take(self.buf_size).cloned());
				Ok(self.buf_size)
			}
		}

		fn flush(&mut self) -> Result<()> {
			unimplemented!();
		}
	}

	impl GenericSocket for TestSocket {}

	struct TestBrokenSocket {
		error: String
	}

	impl Read for TestBrokenSocket {
		fn read(&mut self, _: &mut [u8]) -> Result<usize> {
			Err(Error::new(ErrorKind::Other, self.error.clone()))
		}
	}

	impl Write for TestBrokenSocket {
		fn write(&mut self, _: &[u8]) -> Result<usize> {
			Err(Error::new(ErrorKind::Other, self.error.clone()))
		}

		fn flush(&mut self) -> Result<()> {
			unimplemented!();
		}
	}

	impl GenericSocket for TestBrokenSocket {}

	type TestConnection = GenericConnection<TestSocket>;

	impl Default for TestConnection {
		fn default() -> Self {
			TestConnection::new()
		}
	}

	impl TestConnection {
		pub fn new() -> Self {
			TestConnection {
				token: 999998888usize,
				socket: TestSocket::new(),
				send_queue: VecDeque::new(),
				rec_buf: Bytes::new(),
				rec_size: 0,
				interest: Ready::hup() | Ready::readable(),
				registered: AtomicBool::new(false),
			}
		}
	}

	type TestBrokenConnection = GenericConnection<TestBrokenSocket>;

	impl Default for TestBrokenConnection {
		fn default() -> Self {
			TestBrokenConnection::new()
		}
	}

	impl TestBrokenConnection {
		pub fn new() -> Self {
			TestBrokenConnection {
				token: 999998888usize,
				socket: TestBrokenSocket { error: "test broken socket".to_owned() },
				send_queue: VecDeque::new(),
				rec_buf: Bytes::new(),
				rec_size: 0,
				interest: Ready::hup() | Ready::readable(),
				registered: AtomicBool::new(false),
			}
		}
	}

	fn test_io() -> IoContext<i32> {
		IoContext::new(IoChannel::disconnected(), 0)
	}

	#[test]
	fn connection_expect() {
		let mut connection = TestConnection::new();
		connection.expect(1024);
		assert_eq!(1024, connection.rec_size);
	}

	#[test]
	fn connection_write_empty() {
		let mut connection = TestConnection::new();
		let status = connection.writable(&test_io());
		assert!(status.is_ok());
		assert!(WriteStatus::Complete == status.unwrap());
	}

	#[test]
	fn connection_write() {
		let mut connection = TestConnection::new();
		let data = Cursor::new(vec![0; 10240]);
		connection.send_queue.push_back(data);

		let status = connection.writable(&test_io());
		assert!(status.is_ok());
		assert!(WriteStatus::Complete == status.unwrap());
		assert_eq!(10240, connection.socket.write_buffer.len());
	}

	#[test]
	fn connection_write_is_buffered() {
		let mut connection = TestConnection::new();
		connection.socket = TestSocket::new_buf(1024);
		let data = Cursor::new(vec![0; 10240]);
		connection.send_queue.push_back(data);

		let status = connection.writable(&test_io());

		assert!(status.is_ok());
		assert!(WriteStatus::Ongoing == status.unwrap());
		assert_eq!(1024, connection.socket.write_buffer.len());
	}

	#[test]
	fn connection_write_to_broken() {
		let mut connection = TestBrokenConnection::new();
		let data = Cursor::new(vec![0; 10240]);
		connection.send_queue.push_back(data);

		let status = connection.writable(&test_io());

		assert!(!status.is_ok());
		assert_eq!(1, connection.send_queue.len());
	}

	#[test]
	fn connection_read() {
		let mut connection = TestConnection::new();
		connection.rec_size = 2048;
		connection.rec_buf = vec![10; 1024];
		connection.socket.read_buffer = vec![99; 2048];

		let status = connection.readable();

		assert!(status.is_ok());
		assert_eq!(1024, connection.socket.cursor);
	}

	#[test]
	fn connection_read_from_broken() {
		let mut connection = TestBrokenConnection::new();
		connection.rec_size = 2048;

		let status = connection.readable();
		assert!(!status.is_ok());
		assert_eq!(0, connection.rec_buf.len());
	}

	#[test]
	fn connection_read_nothing() {
		let mut connection = TestConnection::new();
		connection.rec_size = 2048;

		let status = connection.readable();

		assert!(status.is_ok());
		assert_eq!(0, connection.rec_buf.len());
	}

	#[test]
	fn connection_read_full() {
		let mut connection = TestConnection::new();
		connection.rec_size = 1024;
		connection.rec_buf = vec![76;1024];

		let status = connection.readable();

		assert!(status.is_ok());
		assert_eq!(0, connection.socket.cursor);
	}
}
