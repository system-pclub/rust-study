// Copyright 2015-2016 Brian Smith.
//
// Permission to use, copy, modify, and/or distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHORS DISCLAIM ALL WARRANTIES
// WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR ANY
// SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION
// OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN
// CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

#![forbid(
    anonymous_parameters,
    box_pointers,
    legacy_directory_ownership,
    missing_copy_implementations,
    missing_debug_implementations,
    missing_docs,
    trivial_casts,
    trivial_numeric_casts,
    unsafe_code,
    unstable_features,
    unused_extern_crates,
    unused_import_braces,
    unused_qualifications,
    unused_results,
    variant_size_differences,
    warnings,
)]

extern crate ring;

use ring::{aead, error, test};
use std::vec::Vec;

#[test]
fn aead_aes_gcm_128() {
    test_aead(&aead::AES_128_GCM, "tests/aead_aes_128_gcm_tests.txt");
}

#[test]
fn aead_aes_gcm_256() {
    test_aead(&aead::AES_256_GCM, "tests/aead_aes_256_gcm_tests.txt");
}

#[test]
fn aead_chacha20_poly1305() {
    test_aead(&aead::CHACHA20_POLY1305,
              "tests/aead_chacha20_poly1305_tests.txt");
}


fn test_aead(aead_alg: &'static aead::Algorithm, file_path: &str) {
    test_aead_key_sizes(aead_alg);
    test_aead_nonce_sizes(aead_alg).unwrap();

    test::from_file(file_path, |section, test_case| {
        assert_eq!(section, "");
        let key_bytes = test_case.consume_bytes("KEY");
        let nonce = test_case.consume_bytes("NONCE");
        let plaintext = test_case.consume_bytes("IN");
        let ad = test_case.consume_bytes("AD");
        let mut ct = test_case.consume_bytes("CT");
        let tag = test_case.consume_bytes("TAG");
        let error = test_case.consume_optional_string("FAILS");

        let tag_len = aead_alg.tag_len();
        let mut s_in_out = plaintext.clone();
        for _ in 0..tag_len {
            s_in_out.push(0);
        }
        let s_key = aead::SealingKey::new(aead_alg, &key_bytes[..])?;
        let s_result = aead::seal_in_place(&s_key, &nonce[..], &ad,
                                           &mut s_in_out[..], tag_len);
        let o_key = aead::OpeningKey::new(aead_alg, &key_bytes[..])?;

        ct.extend(tag);

        // In release builds, test all prefix lengths from 0 to 4096 bytes.
        // Debug builds are too slow for this, so for those builds, only
        // test a smaller subset.

        // TLS record headers are 5 bytes long.
        // TLS explicit nonces for AES-GCM are 8 bytes long.
        static MINIMAL_IN_PREFIX_LENS: [usize; 36] = [
            // No input prefix to overwrite; i.e. the opening is exactly
            // "in place."
            0,

            1,
            2,

            // Proposed TLS 1.3 header (no explicit nonce).
            5,

            8,

            // Probably the most common use of a non-zero `in_prefix_len`
            // would be to write a decrypted TLS record over the top of the
            // TLS header and nonce.
            5 /* record header */ + 8 /* explicit nonce */,

            // The stitched AES-GCM x86-64 code works on 6-block (96 byte)
            // units. Some of the ChaCha20 code is even weirder.

            15, // The maximum partial AES block.
            16, // One AES block.
            17, // One byte more than a full AES block.

            31, // 2 AES blocks or 1 ChaCha20 block, minus 1.
            32, // Two AES blocks, one ChaCha20 block.
            33, // 2 AES blocks or 1 ChaCha20 block, plus 1.

            47, // Three AES blocks - 1.
            48, // Three AES blocks.
            49, // Three AES blocks + 1.

            63, // Four AES blocks or two ChaCha20 blocks, minus 1.
            64, // Four AES blocks or two ChaCha20 blocks.
            65, // Four AES blocks or two ChaCha20 blocks, plus 1.

            79, // Five AES blocks, minus 1.
            80, // Five AES blocks.
            81, // Five AES blocks, plus 1.

            95, // Six AES blocks or three ChaCha20 blocks, minus 1.
            96, // Six AES blocks or three ChaCha20 blocks.
            97, // Six AES blocks or three ChaCha20 blocks, plus 1.

            111, // Seven AES blocks, minus 1.
            112, // Seven AES blocks.
            113, // Seven AES blocks, plus 1.

            127, // Eight AES blocks or four ChaCha20 blocks, minus 1.
            128, // Eight AES blocks or four ChaCha20 blocks.
            129, // Eight AES blocks or four ChaCha20 blocks, plus 1.

            143, // Nine AES blocks, minus 1.
            144, // Nine AES blocks.
            145, // Nine AES blocks, plus 1.

            255, // 16 AES blocks or 8 ChaCha20 blocks, minus 1.
            256, // 16 AES blocks or 8 ChaCha20 blocks.
            257, // 16 AES blocks or 8 ChaCha20 blocks, plus 1.
        ];

        let mut more_comprehensive_in_prefix_lengths = [0; 4096];
        let in_prefix_lengths;
        if cfg!(debug_assertions) {
            in_prefix_lengths = &MINIMAL_IN_PREFIX_LENS[..];
        } else {
            for b in 0..more_comprehensive_in_prefix_lengths.len() {
                more_comprehensive_in_prefix_lengths[b] = b;
            }
            in_prefix_lengths = &more_comprehensive_in_prefix_lengths[..];
        }
        let mut o_in_out = vec![123u8; 4096];

        for in_prefix_len in in_prefix_lengths.iter() {
            o_in_out.truncate(0);
            for _ in 0..*in_prefix_len {
                o_in_out.push(123);
            }
            o_in_out.extend_from_slice(&ct[..]);
            let o_result = aead::open_in_place(&o_key, &nonce[..], &ad,
                                               *in_prefix_len,
                                               &mut o_in_out[..]);
            match error {
                None => {
                    assert_eq!(Ok(ct.len()), s_result);
                    assert_eq!(&ct[..], &s_in_out[..ct.len()]);
                    assert_eq!(&plaintext[..], o_result.unwrap());
                },
                Some(ref error) if error == "WRONG_NONCE_LENGTH" => {
                    assert_eq!(Err(error::Unspecified), s_result);
                    assert_eq!(Err(error::Unspecified), o_result);
                },
                Some(error) => {
                    unreachable!("Unexpected error test case: {}", error);
                },
            };
        }

        Ok(())
    });
}

fn test_aead_key_sizes(aead_alg: &'static aead::Algorithm) {
    let key_len = aead_alg.key_len();
    let key_data = vec![0u8; key_len * 2];

    // Key is the right size.
    assert!(aead::OpeningKey::new(aead_alg, &key_data[..key_len]).is_ok());
    assert!(aead::SealingKey::new(aead_alg, &key_data[..key_len]).is_ok());

    // Key is one byte too small.
    assert!(aead::OpeningKey::new(aead_alg, &key_data[..(key_len - 1)])
                .is_err());
    assert!(aead::SealingKey::new(aead_alg, &key_data[..(key_len - 1)])
                .is_err());

    // Key is one byte too large.
    assert!(aead::OpeningKey::new(aead_alg, &key_data[..(key_len + 1)])
                .is_err());
    assert!(aead::SealingKey::new(aead_alg, &key_data[..(key_len + 1)])
                .is_err());

    // Key is half the required size.
    assert!(aead::OpeningKey::new(aead_alg, &key_data[..(key_len / 2)])
                .is_err());
    assert!(aead::SealingKey::new(aead_alg, &key_data[..(key_len / 2)])
                .is_err());

    // Key is twice the required size.
    assert!(aead::OpeningKey::new(aead_alg, &key_data[..(key_len * 2)])
                .is_err());
    assert!(aead::SealingKey::new(aead_alg, &key_data[..(key_len * 2)])
                .is_err());

    // Key is empty.
    assert!(aead::OpeningKey::new(aead_alg, &[]).is_err());
    assert!(aead::SealingKey::new(aead_alg, &[]).is_err());

    // Key is one byte.
    assert!(aead::OpeningKey::new(aead_alg, &[0]).is_err());
    assert!(aead::SealingKey::new(aead_alg, &[0]).is_err());
}

// Test that we reject non-standard nonce sizes.
//
// XXX: This test isn't that great in terms of how it tests
// `open_in_place`. It should be constructing a valid ciphertext using the
// unsupported nonce size using a different implementation that supports
// non-standard nonce sizes. So, when `open_in_place` returns
// `Err(error::Unspecified)`, we don't know if it is because it rejected
// the non-standard nonce size or because it tried to process the input
// with the wrong nonce. But at least we're verifying that `open_in_place`
// won't crash or access out-of-bounds memory (when run under valgrind or
// similar). The AES-128-GCM tests have some WRONG_NONCE_LENGTH test cases
// that tests this more correctly.
fn test_aead_nonce_sizes(aead_alg: &'static aead::Algorithm)
                         -> Result<(), error::Unspecified> {
    let key_len = aead_alg.key_len();
    let key_data = vec![0u8; key_len];
    let s_key = aead::SealingKey::new(aead_alg, &key_data[..key_len])?;
    let o_key = aead::OpeningKey::new(aead_alg, &key_data[..key_len])?;

    let nonce_len = aead_alg.nonce_len();

    let nonce = vec![0u8; nonce_len * 2];

    let prefix_len = 0;
    let tag_len = aead_alg.tag_len();
    let ad: [u8; 0] = [];

    // Construct a template input for `seal_in_place`.
    let mut to_seal = b"hello, world".to_vec();
    // Reserve space for tag.
    for _ in 0..tag_len {
        to_seal.push(0);
    }
    let to_seal = &to_seal[..]; // to_seal is no longer mutable.

    // Construct a template input for `open_in_place`.
    let mut to_open = Vec::from(to_seal);
    let ciphertext_len =
        aead::seal_in_place(&s_key, &nonce[..nonce_len], &ad, &mut to_open,
                            tag_len)?;
    let to_open = &to_open[..ciphertext_len];

    // Nonce is the correct length.
    {
        let mut in_out = Vec::from(to_seal);
        assert!(aead::seal_in_place(&s_key, &nonce[..nonce_len], &ad,
                                    &mut in_out, tag_len).is_ok());
    }
    {
        let mut in_out = Vec::from(to_open);
        assert!(aead::open_in_place(&o_key, &nonce[..nonce_len], &ad,
                                    prefix_len, &mut in_out).is_ok());
    }

    // Nonce is one byte too small.
    {
        let mut in_out = Vec::from(to_seal);
        assert!(aead::seal_in_place(&s_key, &nonce[..(nonce_len - 1)], &ad,
                                    &mut in_out, tag_len).is_err());
    }
    {
        let mut in_out = Vec::from(to_open);
        assert!(aead::open_in_place(&o_key, &nonce[..(nonce_len - 1)], &ad,
                                    prefix_len, &mut in_out).is_err());
    }

    // Nonce is one byte too large.
    {
        let mut in_out = Vec::from(to_seal);
        assert!(aead::seal_in_place(&s_key, &nonce[..(nonce_len + 1)], &ad,
                                    &mut in_out, tag_len).is_err());
    }
    {
        let mut in_out = Vec::from(to_open);
        assert!(aead::open_in_place(&o_key, &nonce[..(nonce_len + 1)], &ad,
                                    prefix_len, &mut in_out).is_err());
    }

    // Nonce is half the required size.
    {
        let mut in_out = Vec::from(to_seal);
        assert!(aead::seal_in_place(&s_key, &nonce[..(nonce_len / 2)], &ad,
                                    &mut in_out, tag_len).is_err());
    }
    {
        let mut in_out = Vec::from(to_open);
        assert!(aead::open_in_place(&o_key, &nonce[..(nonce_len / 2)], &ad,
                                    prefix_len, &mut in_out).is_err());
    }

    // Nonce is twice the required size.
    {
        let mut in_out = Vec::from(to_seal);
        assert!(aead::seal_in_place(&s_key, &nonce[..(nonce_len * 2)], &ad,
                                    &mut in_out, tag_len).is_err());
    }
    {
        let mut in_out = Vec::from(to_open);
        assert!(aead::open_in_place(&o_key, &nonce[..(nonce_len * 2)], &ad,
                                    prefix_len, &mut in_out).is_err());
    }

    // Nonce is empty.
    {
        let mut in_out = Vec::from(to_seal);
        assert!(aead::seal_in_place(&s_key, &[], &ad, &mut in_out, tag_len)
                    .is_err());
    }
    {
        let mut in_out = Vec::from(to_open);
        assert!(aead::open_in_place(&o_key, &[], &ad, prefix_len,
                                    &mut in_out).is_err());
    }

    // Nonce is one byte.
    {
        let mut in_out = Vec::from(to_seal);
        assert!(aead::seal_in_place(&s_key, &nonce[..1], &ad, &mut in_out,
                                    tag_len).is_err());
    }
    {
        let mut in_out = Vec::from(to_open);
        assert!(aead::open_in_place(&o_key, &nonce[..1], &ad, prefix_len,
                                    &mut in_out).is_err());
    }

    // Nonce is 128 bits (16 bytes).
    {
        let mut in_out = Vec::from(to_seal);
        assert!(aead::seal_in_place(&s_key, &nonce[..16], &ad, &mut in_out,
                                    tag_len).is_err());
    }
    {
        let mut in_out = Vec::from(to_open);
        assert!(aead::open_in_place(&o_key, &nonce[..16], &ad, prefix_len,
                                    &mut in_out).is_err());
    }

    Ok(())
}
