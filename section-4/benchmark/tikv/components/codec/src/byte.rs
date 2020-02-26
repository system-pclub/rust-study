// Copyright 2018 PingCAP, Inc.
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

use super::{Error, Result};

/// Memory-comparable encoding and decoding utility for bytes.
pub struct MemComparableByteCodec;

const MEMCMP_GROUP_SIZE: usize = 8;
const MEMCMP_PAD_BYTE: u8 = 0;

impl MemComparableByteCodec {
    /// Calculates the length of the value after encoding.
    #[inline]
    pub fn encoded_len(src_len: usize) -> usize {
        (src_len / MEMCMP_GROUP_SIZE + 1) * (MEMCMP_GROUP_SIZE + 1)
    }

    /// Encodes all bytes in the `src` into `dest` in ascending memory-comparable format.
    ///
    /// Returns the number of bytes encoded.
    ///
    /// `dest` must not overlaps `src`, otherwise encoded results will be incorrect.
    ///
    /// # Panics
    ///
    /// Panics if there is not enough space in `dest` for writing encoded bytes.
    ///
    /// You can calculate required space size via `encoded_len`.
    pub fn encode_all(src: &[u8], dest: &mut [u8]) -> usize {
        // Refer: https://github.com/facebook/mysql-5.6/wiki/MyRocks-record-format#memcomparable-format
        unsafe {
            let src_len = src.len();
            let dest_len = dest.len();
            assert!(dest_len >= Self::encoded_len(src_len));

            let mut src_ptr = src.as_ptr();
            let mut dest_ptr = dest.as_mut_ptr();
            let src_ptr_end = src_ptr.add(src_len);

            // There must be 0 or more zero padding groups and 1 non-zero padding groups
            // in the output.
            let zero_padding_groups = src.len() / MEMCMP_GROUP_SIZE;

            // Let's first write these zero padding groups.
            for _ in 0..zero_padding_groups {
                std::ptr::copy_nonoverlapping(src_ptr, dest_ptr, MEMCMP_GROUP_SIZE);
                src_ptr = src_ptr.add(MEMCMP_GROUP_SIZE);
                dest_ptr = dest_ptr.add(MEMCMP_GROUP_SIZE);

                dest_ptr.write(!0);
                dest_ptr = dest_ptr.add(1);
            }

            // Then, write the last group, which should never be zero padding.
            let remaining_size = src_ptr_end.offset_from(src_ptr) as usize;
            let padding_size = MEMCMP_GROUP_SIZE - remaining_size;
            let padding_marker = !(padding_size as u8);
            std::ptr::copy_nonoverlapping(src_ptr, dest_ptr, remaining_size);
            std::ptr::write_bytes(dest_ptr.add(remaining_size), MEMCMP_PAD_BYTE, padding_size);
            dest_ptr = dest_ptr.add(MEMCMP_GROUP_SIZE);
            dest_ptr.write(padding_marker);
            (dest_ptr.offset_from(dest.as_mut_ptr()) + 1) as usize
        }
    }

    /// Performs in place bitwise NOT for specified memory region.
    ///
    /// # Panics
    ///
    /// Panics if `len` exceeds `src.len()`.
    #[inline]
    fn flip_bytes_in_place(src: &mut [u8], len: usize) {
        // This is already super efficient after compiling.
        // It is even faster than a manually written "flip by 64bit".
        for k in &mut src[0..len] {
            *k = !*k;
        }
    }

    /// Encodes all bytes in the `src` into `dest` in descending memory-comparable format.
    ///
    /// Returns the number of bytes encoded.
    ///
    /// `dest` must not overlaps `src`, otherwise encoded results will be incorrect.
    ///
    /// # Panics
    ///
    /// Panics if there is not enough space in `dest` for writing encoded bytes.
    ///
    /// You can calculate required space size via `encoded_len`.
    pub fn encode_all_desc(src: &[u8], dest: &mut [u8]) -> usize {
        let encoded_len = Self::encode_all(src, dest);
        Self::flip_bytes_in_place(dest, encoded_len);
        encoded_len
    }

    /// Decodes bytes in ascending memory-comparable format in the `src` into `dest`.
    ///
    /// If there are multiple encoded byte slices in `src`, only the first one will be decoded.
    ///
    /// Returns `(read_bytes, written_bytes)` where `read_bytes` is the number of bytes read in
    /// `src` and `written_bytes` is the number of bytes written in `dest`.
    ///
    /// Note that actual written data may be larger than `written_bytes`. Bytes more than
    /// `written_bytes` are junk and should be ignored.
    ///
    /// If `src == dest`, please use `try_decode_first_in_place`.
    ///
    /// # Panics
    ///
    /// Panics if `dest.len() < src.len()`, although actual written data may be less.
    ///
    /// When there is a panic, `dest` may contain partially written data.
    ///
    /// # Errors
    ///
    /// Returns `Error::UnexpectedEOF` if `src` is drained while expecting more data.
    ///
    /// Returns `Error::BadPadding` if padding in `src` is incorrect.
    ///
    /// When there is an error, `dest` may contain partially written data.
    pub fn try_decode_first(src: &[u8], dest: &mut [u8]) -> Result<(usize, usize)> {
        Self::try_decode_first_internal(
            src.as_ptr(),
            src.len(),
            dest.as_mut_ptr(),
            dest.len(),
            AscendingMemComparableCodecHelper,
        )
    }

    /// Decodes bytes in descending memory-comparable format in the `src` into `dest`.
    ///
    /// If there are multiple encoded byte slices in `src`, only the first one will be decoded.
    ///
    /// Returns `(read_bytes, written_bytes)` where `read_bytes` is the number of bytes read in
    /// `src` and `written_bytes` is the number of bytes written in `dest`.
    ///
    /// Note that actual written data may be larger than `written_bytes`. Bytes more than
    /// `written_bytes` are junk and should be ignored.
    ///
    /// If `src == dest`, please use `try_decode_first_in_place_desc`.
    ///
    /// # Panics
    ///
    /// Panics if `dest.len() < src.len()`, although actual written data may be less.
    ///
    /// When there is a panic, `dest` may contain partially written data.
    ///
    /// # Errors
    ///
    /// Returns `Error::UnexpectedEOF` if `src` is drained while expecting more data.
    ///
    /// Returns `Error::BadPadding` if padding in `src` is incorrect.
    ///
    /// When there is an error, `dest` may contain partially written data.
    pub fn try_decode_first_desc(src: &[u8], dest: &mut [u8]) -> Result<(usize, usize)> {
        let (read_bytes, written_bytes) = Self::try_decode_first_internal(
            src.as_ptr(),
            src.len(),
            dest.as_mut_ptr(),
            dest.len(),
            DescendingMemComparableCodecHelper,
        )?;
        Self::flip_bytes_in_place(dest, written_bytes);
        Ok((read_bytes, written_bytes))
    }

    /// Decodes bytes in ascending memory-comparable format in place, i.e. decoded data will
    /// overwrite the encoded data.
    ///
    /// If there are multiple encoded byte slices in `buffer`, only the first one will be decoded.
    ///
    /// Returns `(read_bytes, written_bytes)` where `read_bytes` is the number of bytes read
    /// and `written_bytes` is the number of bytes written.
    ///
    /// Note that actual written data may be larger than `written_bytes`. Bytes more than
    /// `written_bytes` are junk and should be ignored.
    ///
    /// # Errors
    ///
    /// Returns `Error::UnexpectedEOF` if `buffer` is drained while expecting more data.
    ///
    /// Returns `Error::BadPadding` if padding in `buffer` is incorrect.
    ///
    /// When there is an error, `dest` may contain partially written data.
    pub fn try_decode_first_in_place(buffer: &mut [u8]) -> Result<(usize, usize)> {
        Self::try_decode_first_internal(
            buffer.as_ptr(),
            buffer.len(),
            buffer.as_mut_ptr(),
            buffer.len(),
            AscendingMemComparableCodecHelper,
        )
    }

    /// Decodes bytes in descending memory-comparable format in place, i.e. decoded data will
    /// overwrite the encoded data.
    ///
    /// If there are multiple encoded byte slices in `buffer`, only the first one will be decoded.
    ///
    /// Returns `(read_bytes, written_bytes)` where `read_bytes` is the number of bytes read
    /// and `written_bytes` is the number of bytes written.
    ///
    /// Note that actual written data may be larger than `written_bytes`. Bytes more than
    /// `written_bytes` are junk and should be ignored.
    ///
    /// # Errors
    ///
    /// Returns `Error::UnexpectedEOF` if `buffer` is drained while expecting more data.
    ///
    /// Returns `Error::BadPadding` if padding in `buffer` is incorrect.
    ///
    /// When there is an error, `dest` may contain partially written data.
    pub fn try_decode_first_in_place_desc(buffer: &mut [u8]) -> Result<(usize, usize)> {
        let (read_bytes, written_bytes) = Self::try_decode_first_internal(
            buffer.as_ptr(),
            buffer.len(),
            buffer.as_mut_ptr(),
            buffer.len(),
            DescendingMemComparableCodecHelper,
        )?;
        Self::flip_bytes_in_place(buffer, written_bytes);
        Ok((read_bytes, written_bytes))
    }

    /// The internal implementation for:
    /// - `try_decode_first`
    /// - `try_decode_first_desc`
    /// - `try_decode_first_in_place`
    /// - `try_decode_first_in_place_desc`
    ///
    /// This function uses pointers to accept the scenario that `src == dest`.
    ///
    /// This function also uses generics to specialize different code path for ascending and
    /// descending decoding, which performs better than inlining a flag.
    ///
    /// Please refer to `try_decode_first` for the meaning of return values, panics and errors.
    #[inline(always)]
    fn try_decode_first_internal<T: MemComparableCodecHelper>(
        mut src_ptr: *const u8,
        src_len: usize,
        mut dest_ptr: *mut u8,
        dest_len: usize,
        _helper: T,
    ) -> Result<(usize, usize)> {
        assert!(dest_len >= src_len);

        // Make copies for the original pointer for calculating filled / read bytes.
        let src_ptr_untouched = src_ptr;
        let dest_ptr_untouched = dest_ptr;

        unsafe {
            let src_ptr_end = src_ptr.add(src_len);

            loop {
                let src_ptr_next = src_ptr.add(MEMCMP_GROUP_SIZE + 1);
                if std::intrinsics::unlikely(src_ptr_next > src_ptr_end) {
                    return Err(Error::UnexpectedEOF);
                }

                // Copy `MEMCMP_GROUP_SIZE` bytes any way. However we will truncate the returned
                // length according to padding size if it is the last block.
                std::ptr::copy(src_ptr, dest_ptr, MEMCMP_GROUP_SIZE);

                let padding_size = T::parse_padding_size(*src_ptr.add(MEMCMP_GROUP_SIZE));
                src_ptr = src_ptr_next;
                dest_ptr = dest_ptr.add(MEMCMP_GROUP_SIZE);

                // If there is a padding, check whether or not it is correct.
                if std::intrinsics::unlikely(padding_size > 0) {
                    // First check padding size.
                    if std::intrinsics::unlikely(padding_size > MEMCMP_GROUP_SIZE) {
                        return Err(Error::BadPadding);
                    }

                    // Then check padding content. Use `libc::memcmp` to compare two memory blocks
                    // is faster than checking pad bytes one by one, since it will compare multiple
                    // bytes at once.
                    let base_padding_ptr = dest_ptr.sub(padding_size);
                    let expected_padding_ptr = T::get_raw_padding_ptr();
                    let cmp_result = libc::memcmp(
                        base_padding_ptr as *const libc::c_void,
                        expected_padding_ptr as *const libc::c_void,
                        padding_size,
                    );
                    if std::intrinsics::unlikely(cmp_result != 0) {
                        return Err(Error::BadPadding);
                    }

                    let read_bytes = src_ptr.offset_from(src_ptr_untouched) as usize;
                    let written_bytes =
                        dest_ptr.offset_from(dest_ptr_untouched) as usize - padding_size;

                    return Ok((read_bytes, written_bytes));
                }
            }
        }
    }
}

trait MemComparableCodecHelper {
    const PADDING: [u8; MEMCMP_GROUP_SIZE];

    /// Given a raw padding size byte, interprets the padding size according to correct order.
    fn parse_padding_size(raw_marker: u8) -> usize;

    /// Returns a pointer to the raw padding bytes in 8 bytes for current ordering.
    fn get_raw_padding_ptr() -> *const u8;
}

struct AscendingMemComparableCodecHelper;

struct DescendingMemComparableCodecHelper;

impl MemComparableCodecHelper for AscendingMemComparableCodecHelper {
    const PADDING: [u8; MEMCMP_GROUP_SIZE] = [MEMCMP_PAD_BYTE; MEMCMP_GROUP_SIZE];

    #[inline]
    fn parse_padding_size(raw_marker: u8) -> usize {
        (!raw_marker) as usize
    }

    #[inline]
    fn get_raw_padding_ptr() -> *const u8 {
        Self::PADDING.as_ptr()
    }
}

impl MemComparableCodecHelper for DescendingMemComparableCodecHelper {
    const PADDING: [u8; MEMCMP_GROUP_SIZE] = [!MEMCMP_PAD_BYTE; MEMCMP_GROUP_SIZE];

    #[inline]
    fn parse_padding_size(raw_marker: u8) -> usize {
        raw_marker as usize
    }

    #[inline]
    fn get_raw_padding_ptr() -> *const u8 {
        Self::PADDING.as_ptr()
    }
}

#[cfg(test)]
mod tests {
    use rand;

    use super::MemComparableByteCodec;

    #[test]
    fn test_memcmp_flip_bytes() {
        for container_len in 0..50 {
            for payload_begin in 0..container_len {
                for payload_end in payload_begin..container_len {
                    let mut base_container: Vec<u8> = Vec::with_capacity(container_len);
                    for _ in 0..container_len {
                        base_container.push(rand::random());
                    }

                    let mut container = base_container.clone();
                    MemComparableByteCodec::flip_bytes_in_place(
                        &mut container.as_mut_slice()[payload_begin..],
                        payload_end - payload_begin,
                    );
                    // bytes before payload_begin should not flip
                    for i in 0..payload_begin {
                        assert_eq!(container[i], base_container[i]);
                    }
                    // bytes between payload_begin and payload_end should flip
                    for i in payload_begin..payload_end {
                        assert_eq!(container[i], !base_container[i]);
                    }
                    // bytes after payload_end should not flip
                    for i in payload_end..container_len {
                        assert_eq!(container[i], base_container[i]);
                    }
                }
            }
        }
    }

    #[test]
    fn test_memcmp_encoded_len() {
        use super::MEMCMP_GROUP_SIZE as N;

        let cases = vec![
            (0, N + 1),
            (N / 2, N + 1),
            (N - 1, N + 1),
            (N, 2 * (N + 1)),
            (N + 1, 2 * (N + 1)),
            (2 * N, 3 * (N + 1)),
            (2 * N + 1, 3 * (N + 1)),
        ];

        for (src_len, encoded_len) in cases {
            assert_eq!(MemComparableByteCodec::encoded_len(src_len), encoded_len);
        }
    }

    #[test]
    fn test_memcmp_encode_all() {
        // Checks whether encoded result matches expectation.

        let cases = vec![
            (
                vec![],
                vec![0, 0, 0, 0, 0, 0, 0, 0, 247],
                vec![255, 255, 255, 255, 255, 255, 255, 255, 8],
            ),
            (
                vec![0],
                vec![0, 0, 0, 0, 0, 0, 0, 0, 248],
                vec![255, 255, 255, 255, 255, 255, 255, 255, 7],
            ),
            (
                vec![1, 2, 3],
                vec![1, 2, 3, 0, 0, 0, 0, 0, 250],
                vec![254, 253, 252, 255, 255, 255, 255, 255, 5],
            ),
            (
                vec![1, 2, 3, 0],
                vec![1, 2, 3, 0, 0, 0, 0, 0, 251],
                vec![254, 253, 252, 255, 255, 255, 255, 255, 4],
            ),
            (
                vec![1, 2, 3, 4, 5, 6, 7],
                vec![1, 2, 3, 4, 5, 6, 7, 0, 254],
                vec![254, 253, 252, 251, 250, 249, 248, 255, 1],
            ),
            (
                vec![0, 0, 0, 0, 0, 0, 0, 0],
                vec![0, 0, 0, 0, 0, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 247],
                vec![
                    255, 255, 255, 255, 255, 255, 255, 255, 0, 255, 255, 255, 255, 255, 255, 255,
                    255, 8,
                ],
            ),
            (
                vec![1, 2, 3, 4, 5, 6, 7, 8],
                vec![1, 2, 3, 4, 5, 6, 7, 8, 255, 0, 0, 0, 0, 0, 0, 0, 0, 247],
                vec![
                    254, 253, 252, 251, 250, 249, 248, 247, 0, 255, 255, 255, 255, 255, 255, 255,
                    255, 8,
                ],
            ),
            (
                vec![1, 2, 3, 4, 5, 6, 7, 8, 9],
                vec![1, 2, 3, 4, 5, 6, 7, 8, 255, 9, 0, 0, 0, 0, 0, 0, 0, 248],
                vec![
                    254, 253, 252, 251, 250, 249, 248, 247, 0, 246, 255, 255, 255, 255, 255, 255,
                    255, 7,
                ],
            ),
        ];

        for (src, expect_encoded_asc, expect_encoded_desc) in cases {
            let encoded_len = MemComparableByteCodec::encoded_len(src.len());
            let buffer_len = encoded_len + 50;
            let mut base_buffer: Vec<u8> = Vec::with_capacity(buffer_len);
            for _ in 0..buffer_len {
                base_buffer.push(rand::random());
            }
            for output_offset in 0..buffer_len {
                for output_slice_len in encoded_len..buffer_len - output_offset {
                    // Test encode ascending
                    let mut output_buffer = base_buffer.clone();
                    let output_len = MemComparableByteCodec::encode_all(
                        src.as_slice(),
                        &mut output_buffer.as_mut_slice()
                            [output_offset..output_offset + output_slice_len],
                    );
                    assert_eq!(output_len, encoded_len);
                    assert_eq!(output_len, expect_encoded_asc.len());
                    // output buffer before output_offset should remain unchanged
                    assert_eq!(
                        &output_buffer[0..output_offset],
                        &base_buffer[0..output_offset]
                    );
                    // output buffer between output_offset and ..+encoded_len should be encoded
                    assert_eq!(
                        &output_buffer[output_offset..output_offset + encoded_len],
                        expect_encoded_asc.as_slice()
                    );
                    // output buffer after output_offset+encoded_len should remain unchanged
                    assert_eq!(
                        &output_buffer[output_offset + encoded_len..],
                        &base_buffer[output_offset + encoded_len..]
                    );

                    // Test encode descending
                    let mut output_buffer = base_buffer.clone();
                    let output_len = MemComparableByteCodec::encode_all_desc(
                        src.as_slice(),
                        &mut output_buffer.as_mut_slice()
                            [output_offset..output_offset + output_slice_len],
                    );
                    assert_eq!(output_len, encoded_len);
                    assert_eq!(output_len, expect_encoded_desc.len());
                    assert_eq!(
                        &output_buffer[0..output_offset],
                        &base_buffer[0..output_offset]
                    );
                    assert_eq!(
                        &output_buffer[output_offset..output_offset + encoded_len],
                        expect_encoded_desc.as_slice()
                    );
                    assert_eq!(
                        &output_buffer[output_offset + encoded_len..],
                        &base_buffer[output_offset + encoded_len..]
                    );
                }
            }
        }
    }

    #[test]
    fn test_memcmp_encode_all_panic() {
        let cases = vec![(0, 0), (0, 7), (0, 8), (7, 8), (8, 9), (8, 17)];
        for (src_len, dest_len) in cases {
            let src = vec![0; src_len];
            let mut dest = vec![0; dest_len];
            let result = panic_hook::recover_safe(move || {
                let _ = MemComparableByteCodec::encode_all(src.as_slice(), dest.as_mut_slice());
            });
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_memcmp_try_decode_first() {
        use super::MEMCMP_GROUP_SIZE as N;

        // We have ensured correctness in `test_memcmp_encode_all`, so we use `encode_all` to
        // generate fixtures in different length, used for decoding.

        fn do_test(
            is_desc: bool,
            payload_len: usize,
            prefix_len: usize,
            suffix_len: usize,
            encoded_prefix_len: usize,
            encoded_suffix_len: usize,
        ) {
            let mut payload_raw: Vec<u8> = Vec::with_capacity(payload_len);
            for _ in 0..payload_len {
                payload_raw.push(rand::random());
            }

            let encoded_len = MemComparableByteCodec::encoded_len(payload_len);
            let mut payload_encoded: Vec<u8> =
                vec![0; encoded_prefix_len + encoded_len + encoded_suffix_len];
            #[allow(clippy::needless_range_loop)]
            for i in 0..encoded_prefix_len {
                payload_encoded[i] = rand::random();
            }
            {
                let src = payload_raw.as_slice();
                let dest = &mut payload_encoded.as_mut_slice()[encoded_prefix_len..];
                if is_desc {
                    MemComparableByteCodec::encode_all_desc(src, dest);
                } else {
                    MemComparableByteCodec::encode_all(src, dest);
                }
            }
            #[allow(clippy::needless_range_loop)]
            for i in encoded_prefix_len + encoded_len..encoded_suffix_len {
                payload_encoded[i] = rand::random();
            }

            let mut base_buffer: Vec<u8> =
                Vec::with_capacity(prefix_len + encoded_len + suffix_len);
            for _ in 0..prefix_len + encoded_len + encoded_suffix_len + suffix_len {
                base_buffer.push(rand::random());
            }

            // Test `dest` doesn't overlap `src`
            let mut output_buffer = base_buffer.clone();
            let output_len = {
                let src = &payload_encoded.as_slice()[encoded_prefix_len..];
                let dest = &mut output_buffer.as_mut_slice()[prefix_len..];
                if is_desc {
                    MemComparableByteCodec::try_decode_first_desc(src, dest).unwrap()
                } else {
                    MemComparableByteCodec::try_decode_first(src, dest).unwrap()
                }
            };
            assert_eq!(output_len.0, encoded_len);
            assert_eq!(output_len.1, payload_len);
            assert_eq!(&output_buffer[0..prefix_len], &base_buffer[0..prefix_len]);
            assert_eq!(
                &output_buffer[prefix_len..prefix_len + payload_len],
                payload_raw.as_slice()
            );
            // Although required space for output is encoded_len + encoded_suffix_len,
            // only first `encoded_len` bytes may be changed, so we only skip `encoded_len`.
            assert_eq!(
                &output_buffer[prefix_len + encoded_len..],
                &base_buffer[prefix_len + encoded_len..]
            );

            // Test `dest` overlaps `src`
            let mut buffer = payload_encoded.clone();
            let output_len = unsafe {
                let src_ptr = buffer.as_mut_ptr().add(encoded_prefix_len);
                let slice_len = buffer.len() - encoded_prefix_len;
                let src = std::slice::from_raw_parts(src_ptr, slice_len);
                let dest = std::slice::from_raw_parts_mut(src_ptr, slice_len);
                if is_desc {
                    MemComparableByteCodec::try_decode_first_desc(src, dest).unwrap()
                } else {
                    MemComparableByteCodec::try_decode_first(src, dest).unwrap()
                }
            };
            assert_eq!(output_len.0, encoded_len);
            assert_eq!(output_len.1, payload_len);
            assert_eq!(
                &buffer[0..encoded_prefix_len],
                &payload_encoded[0..encoded_prefix_len]
            );
            assert_eq!(
                &buffer[encoded_prefix_len..encoded_prefix_len + payload_len],
                payload_raw.as_slice()
            );
            assert_eq!(
                &buffer[encoded_prefix_len + encoded_len..],
                &payload_encoded[encoded_prefix_len + encoded_len..]
            );
        }

        // Whether it is descending order
        for is_desc in &[false, true] {
            // How long is the raw value
            for payload_len in &[
                0,
                1,
                N - 1,
                N,
                N + 1,
                N * 2 - 1,
                N * 2,
                N * 2 + 1,
                N * 3 - 1,
                N * 3,
                N * 3 + 1,
            ] {
                // How long is the prefix prepended before the output slice
                for prefix_len in &[0, 1, N - 1, N, N + 1] {
                    // How long is the suffix appended after the output slice
                    for suffix_len in &[0, 1, N - 1, N, N + 1] {
                        // How long is the prefix prepended before the encoded slice.
                        // Used in overlap tests.
                        for encoded_prefix_len in &[0, 1, N - 1, N, N + 1] {
                            // How long is the suffix appended after the encoded slice to simulate
                            // extra data. Decoding should ignore these extra data in src.
                            for encoded_suffix_len in &[0, 1, N - 1, N, N + 1] {
                                do_test(
                                    *is_desc,
                                    *payload_len,
                                    *prefix_len,
                                    *suffix_len,
                                    *encoded_prefix_len,
                                    *encoded_suffix_len,
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_memcmp_try_decode_first_error() {
        let cases = vec![
            vec![1, 2, 3, 4],
            vec![0, 0, 0, 0, 0, 0, 0, 247],
            vec![0, 0, 0, 0, 0, 0, 0, 0, 246],
            vec![0, 0, 0, 0, 0, 0, 0, 1, 247],
            vec![1, 2, 3, 4, 5, 6, 7, 8, 0],
            vec![1, 2, 3, 4, 5, 6, 7, 8, 255, 1],
            vec![1, 2, 3, 4, 5, 6, 7, 8, 255, 1, 2, 3, 4, 5, 6, 7, 8],
            vec![1, 2, 3, 4, 5, 6, 7, 8, 255, 1, 2, 3, 4, 5, 6, 7, 8, 255],
            vec![1, 2, 3, 4, 5, 6, 7, 8, 255, 1, 2, 3, 4, 5, 6, 7, 8, 0],
            vec![1, 2, 3, 4, 5, 6, 7, 8, 255, 1, 0, 0, 0, 0, 0, 0, 0, 247],
        ];
        for invalid_src in cases {
            let mut dest = vec![0; invalid_src.len()];
            let result = MemComparableByteCodec::try_decode_first(
                invalid_src.as_slice(),
                dest.as_mut_slice(),
            );
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_memcmp_try_decode_first_panic() {
        let cases = vec![
            vec![0, 0, 0, 0, 0, 0, 0, 0, 247],
            vec![1, 2, 3, 4, 5, 6, 7, 0, 254],
            vec![0, 0, 0, 0, 0, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 247],
        ];
        for src in cases {
            {
                let src = src.clone();
                let mut dest = vec![0; src.len() - 1];
                let result = panic_hook::recover_safe(move || {
                    let _ = MemComparableByteCodec::try_decode_first(
                        src.as_slice(),
                        dest.as_mut_slice(),
                    );
                });
                assert!(result.is_err());
            }
            {
                let mut dest = vec![0; src.len()];
                MemComparableByteCodec::try_decode_first(src.as_slice(), dest.as_mut_slice())
                    .unwrap();
            }
        }
    }

    #[test]
    fn test_memcmp_compare() {
        use std::cmp::Ordering;

        let cases: Vec<(&[u8], &[u8], _)> = vec![
            (b"", b"\x00", Ordering::Less),
            (b"\x00", b"\x00", Ordering::Equal),
            (b"\xFF", b"\x00", Ordering::Greater),
            (b"\xFF", b"\xFF\x00", Ordering::Less),
            (b"a", b"b", Ordering::Less),
            (b"a", b"\x00", Ordering::Greater),
            (b"\x00", b"\x01", Ordering::Less),
            (b"\x00\x01", b"\x00\x00", Ordering::Greater),
            (b"\x00\x00\x00", b"\x00\x00", Ordering::Greater),
            (b"\x00\x00\x00", b"\x00\x00", Ordering::Greater),
            (
                b"\x00\x00\x00\x00\x00\x00\x00\x00",
                b"\x00\x00\x00\x00\x00\x00\x00\x00\x00",
                Ordering::Less,
            ),
            (b"\x01\x02\x03\x00", b"\x01\x02\x03", Ordering::Greater),
            (b"\x01\x03\x03\x04", b"\x01\x03\x03\x05", Ordering::Less),
            (
                b"\x01\x02\x03\x04\x05\x06\x07",
                b"\x01\x02\x03\x04\x05\x06\x07\x08",
                Ordering::Less,
            ),
            (
                b"\x01\x02\x03\x04\x05\x06\x07\x08\x09",
                b"\x01\x02\x03\x04\x05\x06\x07\x08",
                Ordering::Greater,
            ),
            (
                b"\x01\x02\x03\x04\x05\x06\x07\x08\x00",
                b"\x01\x02\x03\x04\x05\x06\x07\x08",
                Ordering::Greater,
            ),
        ];

        fn encode_asc(src: &[u8]) -> Vec<u8> {
            let mut buf = vec![0; MemComparableByteCodec::encoded_len(src.len())];
            let encoded = MemComparableByteCodec::encode_all(src, buf.as_mut_slice());
            assert_eq!(encoded, buf.len());
            buf
        }

        fn encode_desc(src: &[u8]) -> Vec<u8> {
            let mut buf = vec![0; MemComparableByteCodec::encoded_len(src.len())];
            let encoded = MemComparableByteCodec::encode_all_desc(src, buf.as_mut_slice());
            assert_eq!(encoded, buf.len());
            buf
        }

        for (x, y, ord) in cases {
            assert_eq!(x.cmp(y), ord);
            assert_eq!(encode_asc(x).cmp(&encode_asc(y)), ord);
            assert_eq!(encode_desc(x).cmp(&encode_desc(y)), ord.reverse());
        }
    }
}

#[cfg(test)]
mod benches {
    use crate::test;

    /// A naive implementation of encoding in mem-comparable format.
    /// It does not process non zero-padding groups separately.
    fn mem_comparable_encode_all_naive(src: &[u8], dest: &mut [u8]) -> usize {
        unsafe {
            let mut src_ptr = src.as_ptr();
            let mut dest_ptr = dest.as_mut_ptr();
            let src_ptr_end = src_ptr.add(src.len());
            let dest_ptr_end = dest_ptr.add(dest.len());

            while src_ptr <= src_ptr_end {
                // We needs to write GROUP_SIZE + 1 bytes then, so we assert a bound.
                assert!(
                    dest_ptr.add(super::MEMCMP_GROUP_SIZE + 1) <= dest_ptr_end,
                    "dest out of bound"
                );
                let remaining_size = src_ptr_end.offset_from(src_ptr) as usize;
                let padding_size;
                if remaining_size > super::MEMCMP_GROUP_SIZE {
                    padding_size = 0;
                    std::ptr::copy_nonoverlapping(src_ptr, dest_ptr, super::MEMCMP_GROUP_SIZE);
                } else {
                    padding_size = super::MEMCMP_GROUP_SIZE - remaining_size;
                    std::ptr::copy_nonoverlapping(src_ptr, dest_ptr, remaining_size);
                    std::ptr::write_bytes(
                        dest_ptr.add(remaining_size),
                        super::MEMCMP_PAD_BYTE,
                        padding_size,
                    );
                }
                src_ptr = src_ptr.add(super::MEMCMP_GROUP_SIZE);
                dest_ptr = dest_ptr.add(super::MEMCMP_GROUP_SIZE);

                let padding_marker = !(padding_size as u8);
                dest_ptr.write(padding_marker);
                dest_ptr = dest_ptr.add(1);
            }

            dest_ptr.offset_from(dest.as_mut_ptr()) as usize
        }
    }

    const ENC_GROUP_SIZE: usize = 8;
    const ENC_MARKER: u8 = b'\xff';
    const ENC_ASC_PADDING: [u8; ENC_GROUP_SIZE] = [0; ENC_GROUP_SIZE];
    const ENC_DESC_PADDING: [u8; ENC_GROUP_SIZE] = [!0; ENC_GROUP_SIZE];

    /// The original implementation of `encode_bytes` in TiKV.
    trait OldBytesEncoder: std::io::Write {
        fn encode_bytes(&mut self, key: &[u8], desc: bool) -> super::Result<()> {
            let len = key.len();
            let mut index = 0;
            let mut buf = [0; ENC_GROUP_SIZE];
            while index <= len {
                let remain = len - index;
                let mut pad: usize = 0;
                if remain > ENC_GROUP_SIZE {
                    self.write_all(adjust_bytes_order(
                        &key[index..index + ENC_GROUP_SIZE],
                        desc,
                        &mut buf,
                    ))
                    .map_err(|_| super::Error::BufferTooSmall)?;
                } else {
                    pad = ENC_GROUP_SIZE - remain;
                    self.write_all(adjust_bytes_order(&key[index..], desc, &mut buf))
                        .map_err(|_| super::Error::BufferTooSmall)?;
                    if desc {
                        self.write_all(&ENC_DESC_PADDING[..pad])
                            .map_err(|_| super::Error::BufferTooSmall)?;
                    } else {
                        self.write_all(&ENC_ASC_PADDING[..pad])
                            .map_err(|_| super::Error::BufferTooSmall)?;
                    }
                }
                self.write_all(adjust_bytes_order(
                    &[ENC_MARKER - (pad as u8)],
                    desc,
                    &mut buf,
                ))
                .map_err(|_| super::Error::BufferTooSmall)?;
                index += ENC_GROUP_SIZE;
            }
            Ok(())
        }
    }

    fn adjust_bytes_order<'a>(bs: &'a [u8], desc: bool, buf: &'a mut [u8]) -> &'a [u8] {
        if desc {
            let mut buf_idx = 0;
            for &b in bs {
                buf[buf_idx] = !b;
                buf_idx += 1;
            }
            &buf[..buf_idx]
        } else {
            bs
        }
    }

    impl<T: std::io::Write> OldBytesEncoder for T {}

    /// The original implementation of `decode_bytes` in TiKV.
    fn original_decode_bytes(data: &mut &[u8], desc: bool) -> super::Result<Vec<u8>> {
        use std::io::Write;

        let mut key = Vec::with_capacity(data.len() / (ENC_GROUP_SIZE + 1) * ENC_GROUP_SIZE);
        let mut offset = 0;
        let chunk_len = ENC_GROUP_SIZE + 1;
        loop {
            // everytime make ENC_GROUP_SIZE + 1 elements as a decode unit
            let next_offset = offset + chunk_len;
            let chunk = if next_offset <= data.len() {
                &data[offset..next_offset]
            } else {
                return Err(super::Error::UnexpectedEOF);
            };
            offset = next_offset;
            // the last byte in decode unit is for marker which indicates pad size
            let (&marker, bytes) = chunk.split_last().unwrap();
            let pad_size = if desc {
                marker as usize
            } else {
                (ENC_MARKER - marker) as usize
            };
            // no padding, just push 8 bytes
            if pad_size == 0 {
                key.write_all(bytes).unwrap();
                continue;
            }
            if pad_size > ENC_GROUP_SIZE {
                return Err(super::Error::BadPadding);
            }
            // if has padding, split the padding pattern and push rest bytes
            let (bytes, padding) = bytes.split_at(ENC_GROUP_SIZE - pad_size);
            key.write_all(bytes).unwrap();
            let pad_byte = if desc { !0 } else { 0 };
            // check the padding pattern whether validate or not
            if padding.iter().any(|x| *x != pad_byte) {
                return Err(super::Error::BadPadding);
            }

            if desc {
                for k in &mut key {
                    *k = !*k;
                }
            }
            // data will point to following unencoded bytes, maybe timestamp
            *data = &data[offset..];
            return Ok(key);
        }
    }

    /// The original implementation of `decode_bytes_in_place` in TiKV.
    fn original_decode_bytes_in_place(data: &mut Vec<u8>, desc: bool) -> super::Result<()> {
        let mut write_offset = 0;
        let mut read_offset = 0;
        loop {
            let marker_offset = read_offset + ENC_GROUP_SIZE;
            if marker_offset >= data.len() {
                return Err(super::Error::UnexpectedEOF);
            };

            unsafe {
                // it is semantically equivalent to C's memmove()
                // and the src and dest may overlap
                // if src == dest do nothing
                std::ptr::copy(
                    data.as_ptr().add(read_offset),
                    data.as_mut_ptr().add(write_offset),
                    ENC_GROUP_SIZE,
                );
            }
            write_offset += ENC_GROUP_SIZE;
            // everytime make ENC_GROUP_SIZE + 1 elements as a decode unit
            read_offset += ENC_GROUP_SIZE + 1;

            // the last byte in decode unit is for marker which indicates pad size
            let marker = data[marker_offset];
            let pad_size = if desc {
                marker as usize
            } else {
                (ENC_MARKER - marker) as usize
            };

            if pad_size > 0 {
                if pad_size > ENC_GROUP_SIZE {
                    return Err(super::Error::BadPadding);
                }

                // check the padding pattern whether validate or not
                let padding_slice = if desc {
                    &ENC_DESC_PADDING[..pad_size]
                } else {
                    &ENC_ASC_PADDING[..pad_size]
                };
                if &data[write_offset - pad_size..write_offset] != padding_slice {
                    return Err(super::Error::BadPadding);
                }
                unsafe {
                    data.set_len(write_offset - pad_size);
                }
                if desc {
                    for k in data {
                        *k = !*k;
                    }
                }
                return Ok(());
            }
        }
    }

    #[bench]
    fn bench_memcmp_encode_all_asc_small(b: &mut test::Bencher) {
        let src = [b'x'; 100];
        let mut dest = [0; 200];
        b.iter(|| {
            let encoded = super::MemComparableByteCodec::encode_all(
                test::black_box(&src),
                test::black_box(&mut dest),
            );
            test::black_box(encoded);
            test::black_box(&dest);
        });
    }

    #[bench]
    fn bench_memcmp_encode_all_desc_small(b: &mut test::Bencher) {
        let src = [b'x'; 100];
        let mut dest = [0; 200];
        b.iter(|| {
            let encoded = super::MemComparableByteCodec::encode_all_desc(
                test::black_box(&src),
                test::black_box(&mut dest),
            );
            test::black_box(encoded);
            test::black_box(&dest);
        });
    }

    #[bench]
    fn bench_memcmp_encode_all_asc_small_naive(b: &mut test::Bencher) {
        let src = [b'x'; 100];
        let mut dest = [0; 200];
        b.iter(|| {
            let encoded =
                mem_comparable_encode_all_naive(test::black_box(&src), test::black_box(&mut dest));
            test::black_box(encoded);
            test::black_box(&dest);
        });
    }

    #[bench]
    fn bench_memcmp_encode_all_asc_large(b: &mut test::Bencher) {
        let src = [b'x'; 1000];
        let mut dest = [0; 2000];
        b.iter(|| {
            let encoded = super::MemComparableByteCodec::encode_all(
                test::black_box(&src),
                test::black_box(&mut dest),
            );
            test::black_box(encoded);
            test::black_box(&dest);
        });
    }

    #[bench]
    fn bench_memcmp_encode_all_asc_large_naive(b: &mut test::Bencher) {
        let src = [b'x'; 1000];
        let mut dest = [0; 2000];
        b.iter(|| {
            let encoded =
                mem_comparable_encode_all_naive(test::black_box(&src), test::black_box(&mut dest));
            test::black_box(encoded);
            test::black_box(&dest);
        });
    }

    #[bench]
    fn bench_memcmp_encode_all_desc_large(b: &mut test::Bencher) {
        let src = [b'x'; 1000];
        let mut dest = [0; 2000];
        b.iter(|| {
            let encoded = super::MemComparableByteCodec::encode_all_desc(
                test::black_box(&src),
                test::black_box(&mut dest),
            );
            test::black_box(encoded);
            test::black_box(&dest);
        });
    }

    #[bench]
    fn bench_memcmp_encode_all_asc_large_original(b: &mut test::Bencher) {
        let src = [b'x'; 1000];
        let mut dest: Vec<u8> = Vec::with_capacity(2000);
        b.iter(|| {
            let dest = test::black_box(&mut dest);
            dest.encode_bytes(test::black_box(&src), test::black_box(false))
                .unwrap();
            test::black_box(&dest);
            unsafe { dest.set_len(0) };
        });
    }

    #[bench]
    fn bench_memcmp_encode_all_desc_large_original(b: &mut test::Bencher) {
        let src = [b'x'; 1000];
        let mut dest: Vec<u8> = Vec::with_capacity(2000);
        b.iter(|| {
            let dest = test::black_box(&mut dest);
            dest.encode_bytes(test::black_box(&src), test::black_box(true))
                .unwrap();
            test::black_box(&dest);
            unsafe { dest.set_len(0) };
        });
    }

    #[bench]
    fn bench_memcmp_decode_first_asc_large(b: &mut test::Bencher) {
        let raw = [b'x'; 1000];
        let mut encoded = vec![0; super::MemComparableByteCodec::encoded_len(1000)];
        super::MemComparableByteCodec::encode_all(&raw, encoded.as_mut_slice());
        let mut decoded = vec![0; encoded.len()];
        b.iter(|| {
            let src = test::black_box(&encoded).as_slice();
            let dest = test::black_box(&mut decoded).as_mut_slice();
            super::MemComparableByteCodec::try_decode_first(src, dest).unwrap();
            test::black_box(&dest);
        });
    }

    #[bench]
    fn bench_memcmp_decode_first_desc_large(b: &mut test::Bencher) {
        let raw = [b'x'; 1000];
        let mut encoded = vec![0; super::MemComparableByteCodec::encoded_len(1000)];
        super::MemComparableByteCodec::encode_all_desc(&raw, encoded.as_mut_slice());
        let mut decoded = vec![0; encoded.len()];
        b.iter(|| {
            let src = test::black_box(&encoded).as_slice();
            let dest = test::black_box(&mut decoded).as_mut_slice();
            super::MemComparableByteCodec::try_decode_first_desc(src, dest).unwrap();
            test::black_box(&dest);
        });
    }

    #[bench]
    fn bench_memcmp_decode_first_asc_large_original(b: &mut test::Bencher) {
        let raw = [b'x'; 1000];
        let mut encoded = vec![0; super::MemComparableByteCodec::encoded_len(1000)];
        super::MemComparableByteCodec::encode_all(&raw, encoded.as_mut_slice());
        b.iter(|| {
            let mut data = test::black_box(&encoded).as_slice();
            let decoded = original_decode_bytes(&mut data, test::black_box(false)).unwrap();
            test::black_box(&data);
            test::black_box(decoded);
        });
    }

    #[bench]
    fn bench_memcmp_decode_first_desc_large_original(b: &mut test::Bencher) {
        let raw = [b'x'; 1000];
        let mut encoded = vec![0; super::MemComparableByteCodec::encoded_len(1000)];
        super::MemComparableByteCodec::encode_all_desc(&raw, encoded.as_mut_slice());
        b.iter(|| {
            let mut data = test::black_box(&encoded).as_slice();
            let decoded = original_decode_bytes(&mut data, test::black_box(true)).unwrap();
            test::black_box(&data);
            test::black_box(decoded);
        });
    }

    #[bench]
    fn bench_memcmp_decode_first_in_place_asc_large(b: &mut test::Bencher) {
        let raw = [b'x'; 1000];
        let mut encoded = vec![0; super::MemComparableByteCodec::encoded_len(1000)];
        super::MemComparableByteCodec::encode_all(&raw, encoded.as_mut_slice());
        b.iter(|| {
            let mut encoded = test::black_box(encoded.clone());
            let src = encoded.as_mut_slice();
            super::MemComparableByteCodec::try_decode_first_in_place(src).unwrap();
            test::black_box(&src);
        });
    }

    #[bench]
    fn bench_memcmp_decode_first_in_place_desc_large(b: &mut test::Bencher) {
        let raw = [b'x'; 1000];
        let mut encoded = vec![0; super::MemComparableByteCodec::encoded_len(1000)];
        super::MemComparableByteCodec::encode_all_desc(&raw, encoded.as_mut_slice());
        b.iter(|| {
            let mut encoded = test::black_box(encoded.clone());
            let src = encoded.as_mut_slice();
            super::MemComparableByteCodec::try_decode_first_in_place_desc(src).unwrap();
            test::black_box(&src);
        });
    }

    #[bench]
    fn bench_memcmp_decode_first_in_place_asc_large_original(b: &mut test::Bencher) {
        let raw = [b'x'; 1000];
        let mut encoded = vec![0; super::MemComparableByteCodec::encoded_len(1000)];
        super::MemComparableByteCodec::encode_all(&raw, encoded.as_mut_slice());
        b.iter(|| {
            let mut encoded = test::black_box(encoded.clone());
            original_decode_bytes_in_place(&mut encoded, test::black_box(false)).unwrap();
            test::black_box(&encoded);
        });
    }

    #[bench]
    fn bench_memcmp_decode_first_in_place_desc_large_original(b: &mut test::Bencher) {
        let raw = [b'x'; 1000];
        let mut encoded = vec![0; super::MemComparableByteCodec::encoded_len(1000)];
        super::MemComparableByteCodec::encode_all_desc(&raw, encoded.as_mut_slice());
        b.iter(|| {
            let mut encoded = test::black_box(encoded.clone());
            original_decode_bytes_in_place(&mut encoded, test::black_box(true)).unwrap();
            test::black_box(&encoded);
        });
    }
}
