// Copyright 2015-2017 Brian Smith.
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

//! Key Agreement: ECDH, including X25519.
//!
//! # Example
//!
//! Note that this example uses X25519, but ECDH using NIST P-256/P-384 is done
//! exactly the same way, just substituting
//! `agreement::ECDH_P256`/`agreement::ECDH_P384` for `agreement::X25519`.
//!
//! ```
//! # extern crate untrusted;
//! # extern crate ring;
//! #
//! # fn x25519_agreement_example() -> Result<(), ring::error::Unspecified> {
//! use ring::{agreement, rand};
//! use untrusted;
//!
//! let rng = rand::SystemRandom::new();
//!
//! let my_private_key =
//!     agreement::EphemeralPrivateKey::generate(&agreement::X25519, &rng)?;
//!
//! // Make `my_public_key` a byte slice containing my public key. In a real
//! // application, this would be sent to the peer in an encoded protocol
//! // message.
//! let mut my_public_key = [0u8; agreement::PUBLIC_KEY_MAX_LEN];
//! let my_public_key =
//!     &mut my_public_key[..my_private_key.public_key_len()];
//! my_private_key.compute_public_key(my_public_key)?;
//!
//! // In a real application, the peer public key would be parsed out of a
//! // protocol message. Here we just generate one.
//! let mut peer_public_key_buf = [0u8; agreement::PUBLIC_KEY_MAX_LEN];
//! let peer_public_key;
//! {
//!     let peer_private_key =
//!        agreement::EphemeralPrivateKey::generate(&agreement::X25519, &rng)?;
//!     peer_public_key =
//!         &mut peer_public_key_buf[..peer_private_key.public_key_len()];
//!     peer_private_key.compute_public_key(peer_public_key)?;
//! }
//! let peer_public_key = untrusted::Input::from(peer_public_key);
//!
//! // In a real application, the protocol specifies how to determine what
//! // algorithm was used to generate the peer's private key. Here, we know it
//! // is X25519 since we just generated it.
//! let peer_public_key_alg = &agreement::X25519;
//!
//! agreement::agree_ephemeral(my_private_key, peer_public_key_alg,
//!                            peer_public_key, ring::error::Unspecified,
//!                            |_key_material| {
//!     // In a real application, we'd apply a KDF to the key material and the
//!     // public keys (as recommended in RFC 7748) and then derive session
//!     // keys from the result. We omit all that here.
//!     Ok(())
//! })
//! # }
//! # fn main() { x25519_agreement_example().unwrap() }
//! ```

// The "NSA Guide" steps here are from from section 3.1, "Ephemeral Unified
// Model."

use crate::{ec, error, rand};
use untrusted;

pub use ec::PUBLIC_KEY_MAX_LEN;

pub use ec::suite_b::ecdh::{ECDH_P256, ECDH_P384};

pub use ec::curve25519::x25519::X25519;

/// A key agreement algorithm.
#[derive(Eq, PartialEq)]
pub struct Algorithm {
    pub(crate) i: ec::AgreementAlgorithmImpl,
}

derive_debug_via_self!(Algorithm, self.i);

/// An ephemeral private key for use (only) with `agree_ephemeral`. The
/// signature of `agree_ephemeral` ensures that an `EphemeralPrivateKey` can be
/// used for at most one key agreement.
pub struct EphemeralPrivateKey {
    private_key: ec::PrivateKey,
    alg: &'static Algorithm,
}

impl<'a> EphemeralPrivateKey {
    /// Generate a new ephemeral private key for the given algorithm.
    ///
    /// C analog: `EC_KEY_new_by_curve_name` + `EC_KEY_generate_key`.
    pub fn generate(
        alg: &'static Algorithm, rng: &rand::SecureRandom,
    ) -> Result<EphemeralPrivateKey, error::Unspecified> {
        // NSA Guide Step 1.
        //
        // This only handles the key generation part of step 1. The rest of
        // step one is done by `compute_public_key()`.
        let private_key = ec::PrivateKey::generate(&alg.i.curve, rng)?;
        Ok(EphemeralPrivateKey { private_key, alg })
    }

    /// The key exchange algorithm.
    #[inline]
    pub fn algorithm(&self) -> &'static Algorithm { self.alg }

    /// The size in bytes of the encoded public key.
    #[inline(always)]
    pub fn public_key_len(&self) -> usize { self.alg.i.curve.public_key_len }

    /// Computes the public key from the private key's value and fills `out`
    /// with the public point encoded in the standard form for the algorithm.
    ///
    /// `out.len()` must be equal to the value returned by `public_key_len`.
    #[inline(always)]
    pub fn compute_public_key(&self, out: &mut [u8]) -> Result<(), error::Unspecified> {
        // NSA Guide Step 1.
        //
        // Obviously, this only handles the part of Step 1 between the private
        // key generation and the sending of the public key to the peer. `out`
        // is what should be sent to the peer.
        self.private_key.compute_public_key(&self.alg.i.curve, out)
    }

    #[cfg(test)]
    pub fn bytes(&'a self, curve: &ec::Curve) -> &'a [u8] { self.private_key.bytes(curve) }
}

/// Performs a key agreement with an ephemeral private key and the given public
/// key.
///
/// `my_private_key` is the ephemeral private key to use. Since it is moved, it
/// will not be usable after calling `agree_ephemeral`, thus guaranteeing that
/// the key is used for only one key agreement.
///
/// `peer_public_key_alg` is the algorithm/curve for the peer's public key
/// point; `agree_ephemeral` will return `Err(error_value)` if it does not
/// match `my_private_key's` algorithm/curve.
///
/// `peer_public_key` is the peer's public key. `agree_ephemeral` verifies that
/// it is encoded in the standard form for the algorithm and that the key is
/// *valid*; see the algorithm's documentation for details on how keys are to
/// be encoded and what constitutes a valid key for that algorithm.
///
/// `error_value` is the value to return if an error occurs before `kdf` is
/// called, e.g. when decoding of the peer's public key fails or when the public
/// key is otherwise invalid.
///
/// After the key agreement is done, `agree_ephemeral` calls `kdf` with the raw
/// key material from the key agreement operation and then returns what `kdf`
/// returns.
///
/// C analogs: `EC_POINT_oct2point` + `ECDH_compute_key`, `X25519`.
pub fn agree_ephemeral<F, R, E>(
    my_private_key: EphemeralPrivateKey, peer_public_key_alg: &Algorithm,
    peer_public_key: untrusted::Input, error_value: E, kdf: F,
) -> Result<R, E>
where
    F: FnOnce(&[u8]) -> Result<R, E>,
{
    // NSA Guide Prerequisite 1.
    //
    // The domain parameters are hard-coded. This check verifies that the
    // peer's public key's domain parameters match the domain parameters of
    // this private key.
    if peer_public_key_alg.i.curve.id != my_private_key.alg.i.curve.id {
        return Err(error_value);
    }

    let alg = &my_private_key.alg.i;

    // NSA Guide Prerequisite 2, regarding which KDFs are allowed, is delegated
    // to the caller.

    // NSA Guide Prerequisite 3, "Prior to or during the key-agreement process,
    // each party shall obtain the identifier associated with the other party
    // during the key-agreement scheme," is delegated to the caller.

    // NSA Guide Step 1 is handled by `EphemeralPrivateKey::generate()` and
    // `EphemeralPrivateKey::compute_public_key()`.

    let mut shared_key = [0u8; ec::ELEM_MAX_BYTES];
    let shared_key = &mut shared_key[..alg.curve.elem_and_scalar_len];

    // NSA Guide Steps 2, 3, and 4.
    //
    // We have a pretty liberal interpretation of the NIST's spec's "Destroy"
    // that doesn't meet the NSA requirement to "zeroize."
    (alg.ecdh)(shared_key, &my_private_key.private_key, peer_public_key)
        .map_err(|_| error_value)?;

    // NSA Guide Steps 5 and 6.
    //
    // Again, we have a pretty liberal interpretation of the NIST's spec's
    // "Destroy" that doesn't meet the NSA requirement to "zeroize."
    kdf(shared_key)
}
