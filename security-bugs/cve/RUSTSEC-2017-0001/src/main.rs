/**
 * Buggy application: sodiumoxide
 */
extern crate libsodium_sys;

use rand::Rng;


/// Number of bytes in a `GroupElement`.
pub const GROUPELEMENTBYTES: usize = libsodium_sys::crypto_scalarmult_curve25519_BYTES as usize;

/// Number of bytes in a `Scalar`.
pub const SCALARBYTES: usize = libsodium_sys::crypto_scalarmult_curve25519_SCALARBYTES as usize;


struct Scalar([u8; SCALARBYTES]);

#[derive(Debug)]
struct GroupElement([u8; GROUPELEMENTBYTES]);

fn scalarmult_bug(n: &Scalar, p: &GroupElement) -> GroupElement {
    let mut q = [0; GROUPELEMENTBYTES];
    unsafe {
        libsodium_sys::crypto_scalarmult_curve25519(q.as_mut_ptr(), n.0.as_ptr(), p.0.as_ptr()) != 0;
    }
    GroupElement(q)
}

fn scalarmult_patch(n: &Scalar, p: &GroupElement) -> Result<GroupElement, ()> {
    let mut q = [0; GROUPELEMENTBYTES];
    unsafe {
        if libsodium_sys::crypto_scalarmult_curve25519(q.as_mut_ptr(), n.0.as_ptr(), p.0.as_ptr()) != 0 {
            Err(())
        } else {
            Ok(GroupElement(q))
        }
    }
}

fn main() {
    let mut sk = rand::thread_rng().gen::<[u8; SCALARBYTES]>();
    let sk = Scalar(sk);
    let pk = GroupElement([0; GROUPELEMENTBYTES]);
    let buggy_result = scalarmult_bug(&sk, &pk);
    let patched_result = scalarmult_patch(&sk, &pk);
    println!("Buggy result: {:?}, Fixed result: {:?}", buggy_result, patched_result);
}
