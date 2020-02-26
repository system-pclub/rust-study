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

use super::{bigint, parse_public_key, RSAParameters, N, PUBLIC_KEY_PUBLIC_MODULUS_MAX_LEN};
/// RSA PKCS#1 1.5 signatures.
use core;
use crate::{bits, digest, error, private, signature};
use untrusted;

#[derive(Debug)]
pub struct Key {
    pub n: bigint::Modulus<N>,
    pub e: bigint::PublicExponent,
    pub n_bits: bits::BitLength,
}

impl Key {
    pub fn from_modulus_and_exponent(
        n: untrusted::Input, e: untrusted::Input, n_min_bits: bits::BitLength,
        n_max_bits: bits::BitLength, e_min_value: u64,
    ) -> Result<Self, error::Unspecified> {
        // This is an incomplete implementation of NIST SP800-56Br1 Section
        // 6.4.2.2, "Partial Public-Key Validation for RSA." That spec defers
        // to NIST SP800-89 Section 5.3.3, "(Explicit) Partial Public Key
        // Validation for RSA," "with the caveat that the length of the modulus
        // shall be a length that is specified in this Recommendation." In
        // SP800-89, two different sets of steps are given, one set numbered,
        // and one set lettered. TODO: Document this in the end-user
        // documentation for RSA keys.

        // Step 3 / Step c for `n` (out of order).
        let (n, n_bits) = bigint::Modulus::from_be_bytes_with_bit_length(n)?;

        // `pkcs1_encode` depends on this not being small. Otherwise,
        // `pkcs1_encode` would generate padding that is invalid (too few 0xFF
        // bytes) for very small keys.
        const N_MIN_BITS: bits::BitLength = bits::BitLength(2048);

        // Step 1 / Step a. XXX: SP800-56Br1 and SP800-89 require the length of
        // the public modulus to be exactly 2048 or 3072 bits, but we are more
        // flexible to be compatible with other commonly-used crypto libraries.
        assert!(n_min_bits >= N_MIN_BITS);
        let n_bits_rounded_up =
            bits::BitLength::from_usize_bytes(n_bits.as_usize_bytes_rounded_up())?;
        if n_bits_rounded_up < n_min_bits {
            return Err(error::Unspecified);
        }
        if n_bits > n_max_bits {
            return Err(error::Unspecified);
        }

        // Step 2 / Step b.
        // Step 3 / Step c for `e`.
        let e = bigint::PublicExponent::from_be_bytes(e, e_min_value)?;

        // If `n` is less than `e` then somebody has probably accidentally swapped
        // them. The largest acceptable `e` is smaller than the smallest acceptable
        // `n`, so no additional checks need to be done.

        // XXX: Steps 4 & 5 / Steps d, e, & f are not implemented. This is also the
        // case in most other commonly-used crypto libraries.

        Ok(Self { n, e, n_bits })
    }

    /// Returns the length in bytes of the modulus.
    ///
    /// A signature has the same length as the public modulus.
    #[cfg(feature = "rsa_signing")]
    pub fn modulus_len(&self) -> usize { self.n_bits.as_usize_bytes_rounded_up() }
}

impl signature::VerificationAlgorithm for RSAParameters {
    fn verify(
        &self, public_key: untrusted::Input, msg: untrusted::Input, signature: untrusted::Input,
    ) -> Result<(), error::Unspecified> {
        let public_key = parse_public_key(public_key)?;
        verify_rsa(self, public_key, msg, signature)
    }
}

impl private::Sealed for RSAParameters {}

impl core::fmt::Debug for RSAParameters {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> Result<(), core::fmt::Error> {
        use super::RSAParametersID::*;
        // XXX: This doesn't include the padding algorithm nor the size range.
        write!(
            f,
            "ring::signature::{}",
            match self.id {
                RSA_PKCS1_2048_8192_SHA1 => "RSA_PKCS1_2048_8192_SHA1",
                RSA_PKCS1_2048_8192_SHA256 => "RSA_PKCS1_2048_8192_SHA256",
                RSA_PKCS1_2048_8192_SHA384 => "RSA_PKCS1_2048_8192_SHA384",
                RSA_PKCS1_2048_8192_SHA512 => "RSA_PKCS1_2048_8192_SHA512",
                RSA_PKCS1_3072_8192_SHA384 => "RSA_PKCS1_3072_8192_SHA384",
                RSA_PSS_2048_8192_SHA256 => "RSA_PSS_2048_8192_SHA256",
                RSA_PSS_2048_8192_SHA384 => "RSA_PSS_2048_8192_SHA384",
                RSA_PSS_2048_8192_SHA512 => "RSA_PSS_2048_8192_SHA512",
            }
        )
    }
}

macro_rules! rsa_params {
    ( $VERIFY_ALGORITHM:ident, $min_bits:expr, $PADDING_ALGORITHM:expr,
      $doc_str:expr ) => {
        #[doc=$doc_str]
        ///
        /// Only available in `use_heap` mode.
        pub static $VERIFY_ALGORITHM: RSAParameters = RSAParameters {
            padding_alg: $PADDING_ALGORITHM,
            min_bits: bits::BitLength($min_bits),
            id: super::RSAParametersID::$VERIFY_ALGORITHM,
        };
    };
}

rsa_params!(
    RSA_PKCS1_2048_8192_SHA1,
    2048,
    &super::padding::RSA_PKCS1_SHA1,
    "Verification of signatures using RSA keys of 2048-8192 bits,
             PKCS#1.5 padding, and SHA-1.\n\nSee \"`RSA_PKCS1_*` Details\" in
             `ring::signature`'s module-level documentation for more details."
);
rsa_params!(
    RSA_PKCS1_2048_8192_SHA256,
    2048,
    &super::RSA_PKCS1_SHA256,
    "Verification of signatures using RSA keys of 2048-8192 bits,
             PKCS#1.5 padding, and SHA-256.\n\nSee \"`RSA_PKCS1_*` Details\" in
             `ring::signature`'s module-level documentation for more details."
);
rsa_params!(
    RSA_PKCS1_2048_8192_SHA384,
    2048,
    &super::RSA_PKCS1_SHA384,
    "Verification of signatures using RSA keys of 2048-8192 bits,
             PKCS#1.5 padding, and SHA-384.\n\nSee \"`RSA_PKCS1_*` Details\" in
             `ring::signature`'s module-level documentation for more details."
);
rsa_params!(
    RSA_PKCS1_2048_8192_SHA512,
    2048,
    &super::RSA_PKCS1_SHA512,
    "Verification of signatures using RSA keys of 2048-8192 bits,
             PKCS#1.5 padding, and SHA-512.\n\nSee \"`RSA_PKCS1_*` Details\" in
             `ring::signature`'s module-level documentation for more details."
);
rsa_params!(
    RSA_PKCS1_3072_8192_SHA384,
    3072,
    &super::RSA_PKCS1_SHA384,
    "Verification of signatures using RSA keys of 3072-8192 bits,
             PKCS#1.5 padding, and SHA-384.\n\nSee \"`RSA_PKCS1_*` Details\" in
             `ring::signature`'s module-level documentation for more details."
);

rsa_params!(
    RSA_PSS_2048_8192_SHA256,
    2048,
    &super::RSA_PSS_SHA256,
    "Verification of signatures using RSA keys of 2048-8192 bits,
             PSS padding, and SHA-256.\n\nSee \"`RSA_PSS_*` Details\" in
             `ring::signature`'s module-level documentation for more details."
);
rsa_params!(
    RSA_PSS_2048_8192_SHA384,
    2048,
    &super::RSA_PSS_SHA384,
    "Verification of signatures using RSA keys of 2048-8192 bits,
             PSS padding, and SHA-384.\n\nSee \"`RSA_PSS_*` Details\" in
             `ring::signature`'s module-level documentation for more details."
);
rsa_params!(
    RSA_PSS_2048_8192_SHA512,
    2048,
    &super::RSA_PSS_SHA512,
    "Verification of signatures using RSA keys of 2048-8192 bits,
             PSS padding, and SHA-512.\n\nSee \"`RSA_PSS_*` Details\" in
             `ring::signature`'s module-level documentation for more details."
);

/// Lower-level API for the verification of RSA signatures.
///
/// When the public key is in DER-encoded PKCS#1 ASN.1 format, it is
/// recommended to use `ring::signature::verify()` with
/// `ring::signature::RSA_PKCS1_*`, because `ring::signature::verify()`
/// will handle the parsing in that case. Otherwise, this function can be used
/// to pass in the raw bytes for the public key components as
/// `untrusted::Input` arguments.
///
/// `params` determine what algorithm parameters (padding, digest algorithm,
/// key length range, etc.) are used in the verification. `msg` is the message
/// and `signature` is the signature.
///
/// `n` is the public key modulus and `e` is the public key exponent. Both are
/// interpreted as unsigned big-endian encoded values. Both must be positive
/// and neither may have any leading zeros.
//
// There are a small number of tests that test `verify_rsa` directly, but the
// test coverage for this function mostly depends on the test coverage for the
// `signature::VerificationAlgorithm` implementation for `RSAParameters`. If we
// change that, test coverage for `verify_rsa()` will need to be reconsidered.
// (The NIST test vectors were originally in a form that was optimized for
// testing `verify_rsa` directly, but the testing work for RSA PKCS#1
// verification was done during the implementation of
// `signature::VerificationAlgorithm`, before `verify_rsa` was factored out).
pub fn verify_rsa(
    params: &RSAParameters, (n, e): (untrusted::Input, untrusted::Input), msg: untrusted::Input,
    signature: untrusted::Input,
) -> Result<(), error::Unspecified> {
    let max_bits = bits::BitLength::from_usize_bytes(PUBLIC_KEY_PUBLIC_MODULUS_MAX_LEN)?;

    // XXX: FIPS 186-4 seems to indicate that the minimum
    // exponent value is 2**16 + 1, but it isn't clear if this is just for
    // signing or also for verification. We support exponents of 3 and larger
    // for compatibility with other commonly-used crypto libraries.
    let Key { n, e, n_bits } = Key::from_modulus_and_exponent(n, e, params.min_bits, max_bits, 3)?;

    // The signature must be the same length as the modulus, in bytes.
    if signature.len() != n_bits.as_usize_bytes_rounded_up() {
        return Err(error::Unspecified);
    }

    // RFC 8017 Section 5.2.2: RSAVP1.

    // Step 1.
    let s = bigint::Elem::from_be_bytes_padded(signature, &n)?;
    if s.is_zero() {
        return Err(error::Unspecified);
    }

    // Step 2.
    let m = bigint::elem_exp_vartime(s, e, &n);
    let m = m.into_unencoded(&n);

    // Step 3.
    let mut decoded = [0u8; PUBLIC_KEY_PUBLIC_MODULUS_MAX_LEN];
    let decoded = &mut decoded[..n_bits.as_usize_bytes_rounded_up()];
    m.fill_be_bytes(decoded);

    // Verify the padded message is correct.
    let m_hash = digest::digest(params.padding_alg.digest_alg(), msg.as_slice_less_safe());
    untrusted::Input::from(decoded).read_all(error::Unspecified, |m| {
        params.padding_alg.verify(&m_hash, m, n_bits)
    })
}
