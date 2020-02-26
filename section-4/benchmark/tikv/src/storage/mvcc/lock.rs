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

use super::super::types::Value;
use super::{Error, Result};
use crate::storage::{Mutation, SHORT_VALUE_MAX_LEN, SHORT_VALUE_PREFIX};
use crate::util::codec::bytes::{self, BytesEncoder};
use crate::util::codec::number::{self, NumberEncoder, MAX_VAR_U64_LEN};
use byteorder::ReadBytesExt;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LockType {
    Put,
    Delete,
    Lock,
}

const FLAG_PUT: u8 = b'P';
const FLAG_DELETE: u8 = b'D';
const FLAG_LOCK: u8 = b'L';

impl LockType {
    pub fn from_mutation(mutation: &Mutation) -> LockType {
        match *mutation {
            Mutation::Put(_) | Mutation::Insert(_) => LockType::Put,
            Mutation::Delete(_) => LockType::Delete,
            Mutation::Lock(_) => LockType::Lock,
        }
    }

    fn from_u8(b: u8) -> Option<LockType> {
        match b {
            FLAG_PUT => Some(LockType::Put),
            FLAG_DELETE => Some(LockType::Delete),
            FLAG_LOCK => Some(LockType::Lock),
            _ => None,
        }
    }

    fn to_u8(self) -> u8 {
        match self {
            LockType::Put => FLAG_PUT,
            LockType::Delete => FLAG_DELETE,
            LockType::Lock => FLAG_LOCK,
        }
    }
}

#[derive(PartialEq, Debug)]
pub struct Lock {
    pub lock_type: LockType,
    pub primary: Vec<u8>,
    pub ts: u64,
    pub ttl: u64,
    pub short_value: Option<Value>,
}

impl Lock {
    pub fn new(
        lock_type: LockType,
        primary: Vec<u8>,
        ts: u64,
        ttl: u64,
        short_value: Option<Value>,
    ) -> Lock {
        Lock {
            lock_type,
            primary,
            ts,
            ttl,
            short_value,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(
            1 + MAX_VAR_U64_LEN + self.primary.len() + MAX_VAR_U64_LEN + SHORT_VALUE_MAX_LEN + 2,
        );
        b.push(self.lock_type.to_u8());
        b.encode_compact_bytes(&self.primary).unwrap();
        b.encode_var_u64(self.ts).unwrap();
        b.encode_var_u64(self.ttl).unwrap();
        if let Some(ref v) = self.short_value {
            b.push(SHORT_VALUE_PREFIX);
            b.push(v.len() as u8);
            b.extend_from_slice(v);
        }
        b
    }

    pub fn parse(mut b: &[u8]) -> Result<Lock> {
        if b.is_empty() {
            return Err(Error::BadFormatLock);
        }
        let lock_type = LockType::from_u8(b.read_u8()?).ok_or(Error::BadFormatLock)?;
        let primary = bytes::decode_compact_bytes(&mut b)?;
        let ts = number::decode_var_u64(&mut b)?;
        let ttl = if b.is_empty() {
            0
        } else {
            number::decode_var_u64(&mut b)?
        };

        if b.is_empty() {
            return Ok(Lock::new(lock_type, primary, ts, ttl, None));
        }

        let flag = b.read_u8()?;
        assert_eq!(
            flag, SHORT_VALUE_PREFIX,
            "invalid flag [{:?}] in write",
            flag
        );

        let len = b.read_u8()?;
        if len as usize != b.len() {
            panic!(
                "short value len [{}] not equal to content len [{}]",
                len,
                b.len()
            );
        }

        Ok(Lock::new(lock_type, primary, ts, ttl, Some(b.to_vec())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{Key, Mutation};

    #[test]
    fn test_lock_type() {
        let (key, value) = (b"key", b"value");
        let mut tests = vec![
            (
                Mutation::Put((Key::from_raw(key), value.to_vec())),
                LockType::Put,
                FLAG_PUT,
            ),
            (
                Mutation::Delete(Key::from_raw(key)),
                LockType::Delete,
                FLAG_DELETE,
            ),
            (
                Mutation::Lock(Key::from_raw(key)),
                LockType::Lock,
                FLAG_LOCK,
            ),
        ];
        for (i, (mutation, lock_type, flag)) in tests.drain(..).enumerate() {
            let lt = LockType::from_mutation(&mutation);
            assert_eq!(
                lt, lock_type,
                "#{}, expect from_mutation({:?}) returns {:?}, but got {:?}",
                i, mutation, lock_type, lt
            );
            let f = lock_type.to_u8();
            assert_eq!(
                f, flag,
                "#{}, expect {:?}.to_u8() returns {:?}, but got {:?}",
                i, lock_type, flag, f
            );
            let lt = LockType::from_u8(flag).unwrap();
            assert_eq!(
                lt, lock_type,
                "#{}, expect from_u8({:?}) returns {:?}, but got {:?})",
                i, flag, lock_type, lt
            );
        }
    }

    #[test]
    fn test_lock() {
        // Test `Lock::to_bytes()` and `Lock::parse()` works as a pair.
        let mut locks = vec![
            Lock::new(LockType::Put, b"pk".to_vec(), 1, 10, None),
            Lock::new(
                LockType::Delete,
                b"pk".to_vec(),
                1,
                10,
                Some(b"short_value".to_vec()),
            ),
        ];
        for (i, lock) in locks.drain(..).enumerate() {
            let v = lock.to_bytes();
            let l = Lock::parse(&v[..]).unwrap_or_else(|e| panic!("#{} parse() err: {:?}", i, e));
            assert_eq!(l, lock, "#{} expect {:?}, but got {:?}", i, lock, l);
        }

        // Test `Lock::parse()` handles incorrect input.
        assert!(Lock::parse(b"").is_err());

        let lock = Lock::new(
            LockType::Lock,
            b"pk".to_vec(),
            1,
            10,
            Some(b"short_value".to_vec()),
        );
        let v = lock.to_bytes();
        assert!(Lock::parse(&v[..4]).is_err());
    }
}
