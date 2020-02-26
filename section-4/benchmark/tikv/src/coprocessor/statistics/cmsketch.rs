// Copyright 2017 PingCAP, Inc.
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

use byteorder::{ByteOrder, LittleEndian};
use murmur3::murmur3_x64_128;
use protobuf::RepeatedField;
use tipb::analyze;

/// `CMSketch` is used to estimate point queries.
/// Refer:[Count-Min Sketch](https://en.wikipedia.org/wiki/Count-min_sketch)
#[derive(Clone)]
pub struct CMSketch {
    depth: usize,
    width: usize,
    count: u32,
    table: Vec<Vec<u32>>,
}

impl CMSketch {
    pub fn new(d: usize, w: usize) -> Option<CMSketch> {
        if d == 0 || w == 0 {
            None
        } else {
            Some(CMSketch {
                depth: d,
                width: w,
                count: 0,
                table: vec![vec![0; w]; d],
            })
        }
    }

    // `hash` hashes the data into two u64 using murmur hash.
    fn hash(mut bytes: &[u8]) -> (u64, u64) {
        let mut out: [u8; 16] = [0; 16];
        murmur3_x64_128(&mut bytes, 0, &mut out);
        (
            LittleEndian::read_u64(&out[0..8]),
            LittleEndian::read_u64(&out[8..16]),
        )
    }

    // `insert` inserts the data into cm sketch. For each row i, the position at
    // (h1 + h2*i) % width will be incremented by one, where the (h1, h2) is the hash value
    // of data.
    pub fn insert(&mut self, bytes: &[u8]) {
        self.count = self.count.wrapping_add(1);
        let (h1, h2) = CMSketch::hash(bytes);
        for (i, row) in self.table.iter_mut().enumerate() {
            let j = (h1.wrapping_add(h2.wrapping_mul(i as u64)) % self.width as u64) as usize;
            row[j] = row[j].saturating_add(1);
        }
    }

    pub fn into_proto(self) -> analyze::CMSketch {
        let mut proto = analyze::CMSketch::new();
        let mut rows = vec![analyze::CMSketchRow::default(); self.depth];
        for (i, row) in self.table.iter().enumerate() {
            rows[i].set_counters(row.to_vec());
        }
        proto.set_rows(RepeatedField::from_vec(rows));
        proto
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coprocessor::codec::datum;
    use crate::coprocessor::codec::datum::Datum;
    use crate::util::as_slice;
    use crate::util::collections::HashMap;
    use rand::{Rng, SeedableRng, StdRng};
    use std::cmp::min;
    use zipf::ZipfDistribution;

    impl CMSketch {
        fn query(&self, bytes: &[u8]) -> u32 {
            let (h1, h2) = CMSketch::hash(bytes);
            let mut vals = vec![0u32; self.depth];
            let mut min_counter = u32::max_value();
            for (i, row) in self.table.iter().enumerate() {
                let j = (h1.wrapping_add(h2.wrapping_mul(i as u64)) % self.width as u64) as usize;
                let noise = (self.count - row[j]) / (self.width as u32 - 1);
                vals[i] = row[j].saturating_sub(noise);
                min_counter = min(min_counter, row[j])
            }
            vals.sort();
            min(
                min_counter,
                vals[(self.depth - 1) / 2]
                    + (vals[self.depth / 2] - vals[(self.depth - 1) / 2]) / 2,
            )
        }

        pub fn count(&self) -> u32 {
            self.count
        }
    }

    fn average_error(depth: usize, width: usize, total: u32, max_value: usize, s: f64) -> u64 {
        let mut c = CMSketch::new(depth, width).unwrap();
        let mut map: HashMap<u64, u32> = HashMap::default();
        let seed: &[_] = &[1, 2, 3, 4];
        let mut gen: ZipfDistribution<StdRng> =
            ZipfDistribution::new(SeedableRng::from_seed(seed), max_value, s).unwrap();
        for _ in 0..total {
            let val = gen.next_u64();
            let bytes = datum::encode_value(as_slice(&Datum::U64(val))).unwrap();
            c.insert(&bytes);
            let counter = map.entry(val).or_insert(0);
            *counter += 1;
        }
        let mut total = 0u64;
        for (val, num) in &map {
            let bytes = datum::encode_value(as_slice(&Datum::U64(*val))).unwrap();
            let estimate = c.query(&bytes);
            let err = if *num > estimate {
                *num - estimate
            } else {
                estimate - *num
            };
            total += u64::from(err)
        }
        total / map.len() as u64
    }

    #[test]
    fn test_hash() {
        let hash_result = CMSketch::hash("€".as_bytes());
        assert_eq!(hash_result.0, 0x59E3303A2FDD9555);
        assert_eq!(hash_result.1, 0x4F9D8BB3E4BC3164);

        let hash_result = CMSketch::hash("€€€€€€€€€€".as_bytes());
        assert_eq!(hash_result.0, 0xCECFEB77375EEF6F);
        assert_eq!(hash_result.1, 0xE9830BC26869E2C6);
    }

    #[test]
    fn test_cm_sketch() {
        let (depth, width) = (8, 2048);
        let (total, max_value) = (10000, 10000000);
        assert_eq!(average_error(depth, width, total, max_value, 1.1), 1);
        assert_eq!(average_error(depth, width, total, max_value, 2.0), 2);
        assert_eq!(average_error(depth, width, total, max_value, 3.0), 2);
    }
}
