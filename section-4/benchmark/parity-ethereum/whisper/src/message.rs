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

//! Whisper message parsing, handlers, and construction.

use std::fmt;
use std::time::{self, SystemTime, Duration, Instant};

use ethereum_types::{H256, H512};
use rlp::{self, DecoderError, RlpStream, Rlp};
use smallvec::SmallVec;
use tiny_keccak::{keccak256, Keccak};

#[cfg(not(time_checked_add))]
use time_utils::CheckedSystemTime;

/// Work-factor proved. Takes 3 parameters: size of message, time to live,
/// and hash.
///
/// Panics if size or TTL is zero.
pub fn work_factor_proved(size: u64, ttl: u64, hash: H256) -> f64 {
	assert!(size != 0 && ttl != 0);

	let leading_zeros = {
		let leading_bytes = hash.iter().take_while(|&&x| x == 0).count();
		let remaining_leading_bits = hash.get(leading_bytes).map_or(0, |byte| byte.leading_zeros() as usize);
		(leading_bytes * 8) + remaining_leading_bits
	};
	let spacetime = size as f64 * ttl as f64;

	2.0_f64.powi(leading_zeros as i32) / spacetime
}

/// A topic of a message.
#[derive(Debug, Default, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Topic(pub [u8; 4]);

impl From<[u8; 4]> for Topic {
	fn from(x: [u8; 4]) -> Self {
		Topic(x)
	}
}

impl Topic {
	/// set up to three bits in the 64-byte bloom passed.
	///
	/// this takes 3 sets of 9 bits, treating each as an index in the range
	/// 0..512 into the bloom and setting the corresponding bit in the bloom to 1.
	pub fn bloom_into(&self, bloom: &mut H512) {

		let data = &self.0;
		for i in 0..3 {
			let mut idx = data[i] as usize;

			if data[3] & (1 << i) != 0 {
				idx += 256;
			}

			debug_assert!(idx <= 511);
			bloom[idx / 8] |= 1 << (7 - idx % 8);
		}
	}

	/// Get bloom for single topic.
	pub fn bloom(&self) -> H512 {
		let mut bloom = Default::default();
		self.bloom_into(&mut bloom);
		bloom
	}
}

impl rlp::Encodable for Topic {
	fn rlp_append(&self, s: &mut RlpStream) {
		s.encoder().encode_value(&self.0);
	}
}

impl rlp::Decodable for Topic {
	fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
		use std::cmp;

		rlp.decoder().decode_value(|bytes| match bytes.len().cmp(&4) {
			cmp::Ordering::Less => Err(DecoderError::RlpIsTooShort),
			cmp::Ordering::Greater => Err(DecoderError::RlpIsTooBig),
			cmp::Ordering::Equal => {
				let mut t = [0u8; 4];
				t.copy_from_slice(bytes);
				Ok(Topic(t))
			}
		})
	}
}

/// Calculate union of blooms for given topics.
pub fn bloom_topics(topics: &[Topic]) -> H512 {
	let mut bloom = H512::default();
	for topic in topics {
		topic.bloom_into(&mut bloom);
	}
	bloom
}

/// Message errors.
#[derive(Debug)]
pub enum Error {
	Decoder(DecoderError),
	EmptyTopics,
	LivesTooLong,
	IssuedInFuture,
	TimestampOverflow,
	ZeroTTL,
}

impl From<DecoderError> for Error {
	fn from(err: DecoderError) -> Self {
		Error::Decoder(err)
	}
}

impl fmt::Display for Error {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match *self {
			Error::Decoder(ref err) => write!(f, "Failed to decode message: {}", err),
			Error::LivesTooLong => write!(f, "Message claims to be issued before the unix epoch."),
			Error::IssuedInFuture => write!(f, "Message issued in future."),
			Error::ZeroTTL => write!(f, "Message live for zero time."),
			Error::TimestampOverflow => write!(f, "Timestamp overflow"),
			Error::EmptyTopics => write!(f, "Message has no topics."),
		}
	}
}

fn append_topics<'a>(s: &'a mut RlpStream, topics: &[Topic]) -> &'a mut RlpStream {
	if topics.len() == 1 {
		s.append(&topics[0])
	} else {
		s.append_list(&topics)
	}
}

fn decode_topics(rlp: Rlp) -> Result<SmallVec<[Topic; 4]>, DecoderError> {
	if rlp.is_list() {
		rlp.iter().map(|r| r.as_val::<Topic>()).collect()
	} else {
		rlp.as_val().map(|t| SmallVec::from_slice(&[t]))
	}
}

// Raw envelope struct.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Envelope {
	/// Expiry timestamp
	pub expiry: u64,
	/// Time-to-live in seconds
	pub ttl: u64,
	/// series of 4-byte topics.
	pub topics: SmallVec<[Topic; 4]>,
	/// The message contained within.
	pub data: Vec<u8>,
	/// Arbitrary value used to target lower PoW hash.
	pub nonce: u64,
}

impl Envelope {
	/// Whether the message is multi-topic. Only relay these to Parity peers.
	pub fn is_multitopic(&self) -> bool {
		self.topics.len() != 1
	}

	fn proving_hash(&self) -> H256 {
		use byteorder::{BigEndian, ByteOrder};

		let mut buf = [0; 32];

		let mut stream = RlpStream::new_list(4);
		stream.append(&self.expiry).append(&self.ttl);

		append_topics(&mut stream, &self.topics)
			.append(&self.data);

		let mut digest = Keccak::new_keccak256();
		digest.update(&*stream.drain());
		digest.update(&{
			let mut nonce_bytes = [0u8; 8];
			BigEndian::write_u64(&mut nonce_bytes, self.nonce);

			nonce_bytes
		});

		digest.finalize(&mut buf);
		H256(buf)
	}
}

impl rlp::Encodable for Envelope {
	fn rlp_append(&self, s: &mut RlpStream) {
		s.begin_list(5)
			.append(&self.expiry)
			.append(&self.ttl);

		append_topics(s, &self.topics)
			.append(&self.data)
			.append(&self.nonce);
	}
}

impl rlp::Decodable for Envelope {
	fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
		if rlp.item_count()? != 5 { return Err(DecoderError::RlpIncorrectListLen) }

		Ok(Envelope {
			expiry: rlp.val_at(0)?,
			ttl: rlp.val_at(1)?,
			topics: decode_topics(rlp.at(2)?)?,
			data: rlp.val_at(3)?,
			nonce: rlp.val_at(4)?,
		})
	}
}

/// Message creation parameters.
/// Pass this to `Message::create` to make a message.
pub struct CreateParams {
	/// time-to-live in seconds.
	pub ttl: u64,
	/// payload data.
	pub payload: Vec<u8>,
	/// Topics. May not be empty.
	pub topics: Vec<Topic>,
	/// How many milliseconds to spend proving work.
	pub work: u64,
}

/// A whisper message. This is a checked message carrying around metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
	envelope: Envelope,
	bloom: H512,
	hash: H256,
	encoded_size: usize,
}

impl Message {
	/// Create a message from creation parameters.
	/// Panics if TTL is 0.
	pub fn create(params: CreateParams) -> Result<Self, Error> {
		use byteorder::{BigEndian, ByteOrder};
		use rand::{Rng, SeedableRng, XorShiftRng};

		if params.topics.is_empty() { return Err(Error::EmptyTopics) }

		let mut rng = {
			let mut thread_rng = ::rand::thread_rng();

			XorShiftRng::from_seed(thread_rng.gen::<[u32; 4]>())
		};

		assert!(params.ttl > 0);

		let expiry = {
			let after_mining = SystemTime::now().checked_sub(Duration::from_millis(params.work))
				.ok_or(Error::TimestampOverflow)?;
			let since_epoch = after_mining.duration_since(time::UNIX_EPOCH)
				.expect("time after now is after unix epoch; qed");

			// round up the sub-second to next whole second.
			since_epoch.as_secs() + if since_epoch.subsec_nanos() == 0 { 0 } else { 1 }
		};

		let start_digest = {
			let mut stream = RlpStream::new_list(4);
			stream.append(&expiry).append(&params.ttl);
			append_topics(&mut stream, &params.topics).append(&params.payload);

			let mut digest = Keccak::new_keccak256();
			digest.update(&*stream.drain());
			digest
		};

		let mut buf = [0; 32];
		let mut try_nonce = move |nonce: &[u8; 8]| {
			let mut digest = start_digest.clone();
			digest.update(&nonce[..]);
			digest.finalize(&mut buf[..]);

			buf.clone()
		};

		let mut nonce: [u8; 8] = rng.gen();
		let mut best_found = try_nonce(&nonce);

		let start = Instant::now();

		while start.elapsed() <= Duration::from_millis(params.work) {
			let temp_nonce = rng.gen();
			let hash = try_nonce(&temp_nonce);

			if hash < best_found {
				nonce = temp_nonce;
				best_found = hash;
			}
		}

		let envelope = Envelope {
			expiry: expiry,
			ttl: params.ttl,
			topics: params.topics.into_iter().collect(),
			data: params.payload,
			nonce: BigEndian::read_u64(&nonce[..]),
		};

		debug_assert_eq!(H256(best_found.clone()), envelope.proving_hash());

		let encoded = ::rlp::encode(&envelope);

		Ok(Message::from_components(
			envelope,
			encoded.len(),
			H256(keccak256(&encoded)),
			SystemTime::now(),
		).expect("Message generated here known to be valid; qed"))
	}

	/// Decode message from RLP and check for validity against system time.
	pub fn decode(rlp: Rlp, now: SystemTime) -> Result<Self, Error> {
		let envelope: Envelope = rlp.as_val()?;
		let encoded_size = rlp.as_raw().len();
		let hash = H256(keccak256(rlp.as_raw()));

		Message::from_components(envelope, encoded_size, hash, now)
	}

	// create message from envelope, hash, and encoded size.
	// does checks for validity.
	fn from_components(envelope: Envelope, size: usize, hash: H256, now: SystemTime)
		-> Result<Self, Error>
	{
		const LEEWAY_SECONDS: u64 = 2;

		if envelope.expiry <= envelope.ttl { return Err(Error::LivesTooLong) }
		if envelope.ttl == 0 { return Err(Error::ZeroTTL) }

		if envelope.topics.is_empty() { return Err(Error::EmptyTopics) }

		let issue_time_adjusted = Duration::from_secs(
			(envelope.expiry - envelope.ttl).saturating_sub(LEEWAY_SECONDS)
		);

		let issue_time_adjusted = time::UNIX_EPOCH.checked_add(issue_time_adjusted)
			.ok_or(Error::TimestampOverflow)?;

		if issue_time_adjusted > now {
			return Err(Error::IssuedInFuture);
		}

		// other validity checks?
		let bloom = bloom_topics(&envelope.topics);

		Ok(Message {
			envelope: envelope,
			bloom: bloom,
			hash: hash,
			encoded_size: size,
		})
	}

	/// Get a reference to the envelope.
	pub fn envelope(&self) -> &Envelope {
		&self.envelope
	}

	/// Get the encoded size of the envelope.
	pub fn encoded_size(&self) -> usize {
		self.encoded_size
	}

	/// Get a uniquely identifying hash for the message.
	pub fn hash(&self) -> &H256 {
		&self.hash
	}

	/// Get the bloom filter of the topics
	pub fn bloom(&self) -> &H512 {
		&self.bloom
	}

	/// Get the work proved by the hash.
	pub fn work_proved(&self) -> f64 {
		let proving_hash = self.envelope.proving_hash();

		work_factor_proved(self.encoded_size as _, self.envelope.ttl, proving_hash)
	}

	/// Get the expiry time.
	pub fn expiry(&self) -> Option<SystemTime> {
		time::UNIX_EPOCH.checked_add(Duration::from_secs(self.envelope.expiry))
	}

	/// Get the topics.
	pub fn topics(&self) -> &[Topic] {
		&self.envelope.topics
	}

	/// Get the message data.
	pub fn data(&self) -> &[u8] {
		&self.envelope.data
	}
}

#[cfg(test)]
mod tests {
	use ethereum_types::H256;
	use super::*;
	use std::time::{self, Duration, SystemTime};
	use rlp::Rlp;
	use smallvec::SmallVec;

	fn unix_time(x: u64) -> SystemTime {
		time::UNIX_EPOCH + Duration::from_secs(x)
	}

	#[test]
	fn create_message() {
		assert!(Message::create(CreateParams {
			ttl: 100,
			payload: vec![1, 2, 3, 4],
			topics: vec![Topic([1, 2, 1, 2])],
			work: 50,
		}).is_ok());
	}

	#[test]
	fn round_trip() {
		let envelope = Envelope {
			expiry: 100_000,
			ttl: 30,
			data: vec![9; 256],
			topics: SmallVec::from_slice(&[Default::default()]),
			nonce: 1010101,
		};

		let encoded = ::rlp::encode(&envelope);
		let decoded = ::rlp::decode(&encoded).expect("failure decoding Envelope");

		assert_eq!(envelope, decoded)
	}

	#[test]
	fn round_trip_multitopic() {
		let envelope = Envelope {
			expiry: 100_000,
			ttl: 30,
			data: vec![9; 256],
			topics: SmallVec::from_slice(&[Default::default(), Topic([1, 2, 3, 4])]),
			nonce: 1010101,
		};

		let encoded = ::rlp::encode(&envelope);
		let decoded = ::rlp::decode(&encoded).expect("failure decoding Envelope");

		assert_eq!(envelope, decoded)
	}

	#[test]
	fn passes_checks() {
		let envelope = Envelope {
			expiry: 100_000,
			ttl: 30,
			data: vec![9; 256],
			topics: SmallVec::from_slice(&[Default::default()]),
			nonce: 1010101,
		};

		let encoded = ::rlp::encode(&envelope);

		for i in 0..30 {
			let now = unix_time(100_000 - i);
			Message::decode(Rlp::new(&*encoded), now).unwrap();
		}
	}

	#[test]
	#[should_panic]
	fn future_message() {
		let envelope = Envelope {
			expiry: 100_000,
			ttl: 30,
			data: vec![9; 256],
			topics: SmallVec::from_slice(&[Default::default()]),
			nonce: 1010101,
		};

		let encoded = ::rlp::encode(&envelope);

		let now = unix_time(100_000 - 1_000);
		Message::decode(Rlp::new(&*encoded), now).unwrap();
	}

	#[test]
	#[should_panic]
	fn pre_epoch() {
		let envelope = Envelope {
			expiry: 100_000,
			ttl: 200_000,
			data: vec![9; 256],
			topics: SmallVec::from_slice(&[Default::default()]),
			nonce: 1010101,
		};

		let encoded = ::rlp::encode(&envelope);

		let now = unix_time(95_000);
		Message::decode(Rlp::new(&*encoded), now).unwrap();
	}

	#[test]
	fn work_factor() {
		// 256 leading zeros -> 2^256 / 1
		assert_eq!(work_factor_proved(1, 1, H256::from(0)), 115792089237316200000000000000000000000000000000000000000000000000000000000000.0);
		// 255 leading zeros -> 2^255 / 1
		assert_eq!(work_factor_proved(1, 1, H256::from(1)), 57896044618658100000000000000000000000000000000000000000000000000000000000000.0);
		// 0 leading zeros -> 2^0 / 1
		assert_eq!(work_factor_proved(1, 1, serde_json::from_str::<H256>("\"0xff00000000000000000000000000000000000000000000000000000000000000\"").unwrap()), 1.0);
	}
}
