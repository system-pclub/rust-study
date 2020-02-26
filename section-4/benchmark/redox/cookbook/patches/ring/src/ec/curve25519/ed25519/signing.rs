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

//! EdDSA Signatures.

use super::{super::ops::*, PUBLIC_KEY_LEN};
use core;
use crate::{der, digest, error, pkcs8, rand, signature, signature_impl};
use untrusted;

use super::digest::*;

/// An Ed25519 key pair, for signing.
pub struct KeyPair {
    // RFC 8032 Section 5.1.6 calls this *s*.
    private_scalar: Scalar,

    // RFC 8032 Section 5.1.6 calls this *prefix*.
    private_prefix: Prefix,

    // RFC 8032 Section 5.1.5 calls this *A*.
    public_key: PublicKey,
}

impl<'a> KeyPair {
    /// Generates a new key pair and returns the key pair serialized as a
    /// PKCS#8 document.
    ///
    /// The PKCS#8 document will be a v2 `OneAsymmetricKey` with the public key,
    /// as described in [RFC 5958 Section 2]. See also
    /// https://tools.ietf.org/html/draft-ietf-curdle-pkix-04.
    ///
    /// [RFC 5958 Section 2]: https://tools.ietf.org/html/rfc5958#section-2
    pub fn generate_pkcs8(
        rng: &rand::SecureRandom,
    ) -> Result<[u8; PKCS8_V2_LEN], error::Unspecified> {
        let mut seed = [0u8; SEED_LEN];
        rng.fill(&mut seed)?;
        let key_pair = Self::from_seed_(&seed);
        // TODO: Replace this with `wrap_key()` and return a `pkcs8::Document`.
        let mut bytes = [0; PKCS8_V2_LEN];
        pkcs8::wrap_key_(
            &PKCS8_TEMPLATE,
            &seed[..],
            key_pair.public_key_bytes(),
            &mut bytes[..],
        );
        Ok(bytes)
    }

    /// Constructs an Ed25519 key pair by parsing an unencrypted PKCS#8 v2
    /// Ed25519 private key.
    ///
    /// The input must be in PKCS#8 v2 format, and in particular it must contain
    /// the public key in addition to the private key. `from_pkcs8()` will
    /// verify that the public key and the private key are consistent with each
    /// other.
    ///
    /// If you need to parse PKCS#8 v1 files (without the public key) then use
    /// `Ed25519KeyPair::from_pkcs8_maybe_unchecked()` instead.
    pub fn from_pkcs8(input: untrusted::Input) -> Result<Self, error::Unspecified> {
        let (seed, public_key) = unwrap_pkcs8(pkcs8::Version::V2Only, input)?;
        Self::from_seed_and_public_key(seed, public_key.unwrap())
    }

    /// Constructs an Ed25519 key pair by parsing an unencrypted PKCS#8 v1 or v2
    /// Ed25519 private key.
    ///
    /// It is recommended to use `Ed25519KeyPair::from_pkcs8()`, which accepts
    /// only PKCS#8 v2 files that contain the public key.
    /// `from_pkcs8_maybe_unchecked()` parses PKCS#2 files exactly like
    /// `from_pkcs8()`. It also accepts v1 files. PKCS#8 v1 files do not contain
    /// the public key, so when a v1 file is parsed the public key will be
    /// computed from the private key, and there will be no consistency check
    /// between the public key and the private key.
    ///
    /// PKCS#8 v2 files are parsed exactly like `Ed25519KeyPair::from_pkcs8()`.
    pub fn from_pkcs8_maybe_unchecked(input: untrusted::Input) -> Result<Self, error::Unspecified> {
        let (seed, public_key) = unwrap_pkcs8(pkcs8::Version::V1OrV2, input)?;
        if let Some(public_key) = public_key {
            Self::from_seed_and_public_key(seed, public_key)
        } else {
            Self::from_seed_unchecked(seed)
        }
    }

    /// Constructs an Ed25519 key pair from the private key seed `seed` and its
    /// public key `public_key`.
    ///
    /// It is recommended to use `Ed25519KeyPair::from_pkcs8()` instead.
    ///
    /// The private and public keys will be verified to be consistent with each
    /// other. This helps avoid misuse of the key (e.g. accidentally swapping
    /// the private key and public key, or using the wrong private key for the
    /// public key). This also detects any corruption of the public or private
    /// key.
    pub fn from_seed_and_public_key(
        seed: untrusted::Input, public_key: untrusted::Input,
    ) -> Result<Self, error::Unspecified> {
        let pair = Self::from_seed_unchecked(seed)?;

        // This implicitly verifies that `public_key` is the right length.
        // XXX: This rejects ~18 keys when they are partially reduced, though
        // those keys are virtually impossible to find.
        if public_key != pair.public_key_bytes() {
            return Err(error::Unspecified);
        }

        Ok(pair)
    }

    /// Constructs a Ed25519 key pair from the private key seed `seed`.
    ///
    /// It is recommended to use `Ed25519KeyPair::from_pkcs8()` instead. When
    /// that is not practical, it is recommended to use
    /// `Ed25519KeyPair::from_seed_and_public_key()` instead.
    ///
    /// Since the public key is not given, the public key will be computed from
    /// the private key. It is not possible to detect misuse or corruption of
    /// the private key since the public key isn't given as input.
    pub fn from_seed_unchecked(seed: untrusted::Input) -> Result<Self, error::Unspecified> {
        let seed = slice_as_array_ref!(seed.as_slice_less_safe(), SEED_LEN)?;
        Ok(Self::from_seed_(seed))
    }

    fn from_seed_(seed: &Seed) -> Self {
        let h = digest::digest(&digest::SHA512, seed);
        let (scalar_encoded, prefix_encoded) = h.as_ref().split_at(SCALAR_LEN);

        let mut scalar = [0u8; SCALAR_LEN];
        scalar.copy_from_slice(&scalar_encoded);
        unsafe { GFp_x25519_sc_mask(&mut scalar) };

        let mut prefix = [0u8; PREFIX_LEN];
        prefix.copy_from_slice(prefix_encoded);

        let mut a = ExtPoint::new_at_infinity();
        unsafe {
            GFp_x25519_ge_scalarmult_base(&mut a, &scalar);
        }

        Self {
            private_scalar: scalar,
            private_prefix: prefix,
            public_key: a.into_encoded_point(),
        }
    }

    /// Returns a reference to the little-endian-encoded public key bytes.
    pub fn public_key_bytes(&'a self) -> &'a [u8] { &self.public_key }

    /// Returns the signature of the message `msg`.
    pub fn sign(&self, msg: &[u8]) -> signature::Signature {
        let mut signature_bytes = [0u8; SIGNATURE_LEN];
        {
            // Borrow `signature_bytes`.
            let (signature_r, signature_s) = signature_bytes.split_at_mut(ELEM_LEN);
            let signature_r = slice_as_array_ref_mut!(signature_r, ELEM_LEN).unwrap();
            let signature_s = slice_as_array_ref_mut!(signature_s, SCALAR_LEN).unwrap();

            let nonce = {
                let mut ctx = digest::Context::new(&digest::SHA512);
                ctx.update(&self.private_prefix);
                ctx.update(msg);
                ctx.finish()
            };
            let nonce = digest_scalar(nonce);

            let mut r = ExtPoint::new_at_infinity();
            unsafe {
                GFp_x25519_ge_scalarmult_base(&mut r, &nonce);
            }
            *signature_r = r.into_encoded_point();
            let hram_digest = eddsa_digest(signature_r, &self.public_key, msg);
            let hram = digest_scalar(hram_digest);
            unsafe {
                GFp_x25519_sc_muladd(signature_s, &hram, &self.private_scalar, &nonce);
            }
        }
        signature_impl::signature_from_bytes(&signature_bytes)
    }
}

fn unwrap_pkcs8(
    version: pkcs8::Version, input: untrusted::Input,
) -> Result<(untrusted::Input, Option<untrusted::Input>), error::Unspecified> {
    let (private_key, public_key) = pkcs8::unwrap_key(&PKCS8_TEMPLATE, version, input)?;
    let private_key = private_key.read_all(error::Unspecified, |input| {
        der::expect_tag_and_get_value(input, der::Tag::OctetString)
    })?;
    Ok((private_key, public_key))
}

extern "C" {
    fn GFp_x25519_ge_scalarmult_base(h: &mut ExtPoint, a: &Seed);
    fn GFp_x25519_sc_mask(a: &mut Scalar);
    fn GFp_x25519_sc_muladd(s: &mut Scalar, a: &Scalar, b: &Scalar, c: &Scalar);
}

type PublicKey = [u8; PUBLIC_KEY_LEN];

type Prefix = [u8; PREFIX_LEN];
const PREFIX_LEN: usize = digest::SHA512_OUTPUT_LEN - SCALAR_LEN;

const SIGNATURE_LEN: usize = ELEM_LEN + SCALAR_LEN;

type Seed = [u8; SEED_LEN];
const SEED_LEN: usize = 32;

static PKCS8_TEMPLATE: pkcs8::Template = pkcs8::Template {
    bytes: include_bytes!("ed25519_pkcs8_v2_template.der"),
    alg_id_range: core::ops::Range { start: 7, end: 12 },
    curve_id_index: 0,
    private_key_index: 0x10,
};

/// The length of a Ed25519 PKCS#8 (v2) private key generated by
/// `Ed25519KeyPair::generate_pkcs8()`. Ed25519 PKCS#8 files generated by other
/// software may have different lengths, and `Ed25519KeyPair::generate_pkcs8()`
/// may generate files of a different length in the future.
pub const PKCS8_V2_LEN: usize = 0x55;
