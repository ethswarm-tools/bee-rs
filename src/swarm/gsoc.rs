//! GSOC (Graffiti SOC) primitives. Mirrors bee-go's `pkg/swarm/gsoc.go`.
//!
//! GSOC mining searches a small key space until it finds a private key
//! whose derived `(identifier, owner)` SOC address has the desired
//! proximity to a target overlay. The space is `[0xb33, 0xb33 + 0xffff]`
//! to match bee-js's mining loop exactly.

use crate::swarm::bmt::keccak256;
use crate::swarm::errors::Error;
use crate::swarm::keys::PrivateKey;
use crate::swarm::typed_bytes::Identifier;

/// Bee-js mining start offset (`0xb33` = 2867).
pub const GSOC_MINE_START: u64 = 0xb33;
/// Default proximity target (matches bee-js).
pub const GSOC_DEFAULT_PROXIMITY: u32 = 12;

/// Number of common most-significant bits between two equal-length
/// byte sequences. Mirrors bee-go's `swarm.Proximity`. Returns `0`
/// when lengths differ. Caller is responsible for passing same-length
/// inputs.
pub fn proximity(a: &[u8], b: &[u8]) -> u32 {
    if a.len() != b.len() {
        return 0;
    }
    let mut po: u32 = 0;
    for i in 0..a.len() {
        let x = a[i] ^ b[i];
        if x == 0 {
            po += 8;
            continue;
        }
        for j in 0..8 {
            if (x & (0x80 >> j)) == 0 {
                po += 1;
            } else {
                return po;
            }
        }
        return po;
    }
    po
}

/// Search for a private key whose `(identifier, owner_address)` SOC
/// address has at least `target_proximity` matching most-significant
/// bits with `target` (a 32-byte overlay address). Returns
/// [`Error::Argument`] if no key is found within the bee-js search
/// window.
///
/// Mirrors bee-js `gsocMine` and bee-go `GSOCMine`.
pub fn gsoc_mine(
    target: &[u8],
    identifier: &Identifier,
    target_proximity: u32,
) -> Result<PrivateKey, Error> {
    if target.len() != 32 {
        return Err(Error::argument(format!(
            "target overlay must be 32 bytes, got {}",
            target.len()
        )));
    }

    for i in 0..0xffffu64 {
        let val = GSOC_MINE_START + i;

        let mut key_bytes = [0u8; 32];
        key_bytes[24..].copy_from_slice(&val.to_be_bytes());

        // Skip values that don't form a valid secp256k1 scalar.
        let priv_key = match PrivateKey::new(&key_bytes) {
            Ok(k) => k,
            Err(_) => continue,
        };
        let pub_key = match priv_key.public_key() {
            Ok(p) => p,
            Err(_) => continue,
        };
        let owner = pub_key.address();

        // SOC address = keccak256(identifier || owner_bytes).
        let mut input = Vec::with_capacity(identifier.as_bytes().len() + owner.as_bytes().len());
        input.extend_from_slice(identifier.as_bytes());
        input.extend_from_slice(owner.as_bytes());
        let soc_addr = keccak256(&input);

        if proximity(&soc_addr, target) >= target_proximity {
            return Ok(priv_key);
        }
    }
    Err(Error::argument("could not mine a valid GSOC signer"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proximity_zero_for_max_diff() {
        let a = [0x00u8; 32];
        let mut b = [0x00u8; 32];
        b[0] = 0xff;
        assert_eq!(proximity(&a, &b), 0);
    }

    #[test]
    fn proximity_full_for_equal() {
        let a = [0x42u8; 32];
        assert_eq!(proximity(&a, &a), 8 * 32);
    }

    #[test]
    fn proximity_counts_leading_common_bits() {
        let a = [0xffu8; 32]; // 1111_1111 ...
        let mut b = a;
        b[0] = 0xfe; // 1111_1110 — first 7 bits common, then differ
        assert_eq!(proximity(&a, &b), 7);
    }

    #[test]
    fn proximity_returns_zero_for_length_mismatch() {
        assert_eq!(proximity(&[0u8; 16], &[0u8; 32]), 0);
    }

    #[test]
    fn mining_finds_low_proximity_key_quickly() {
        // Proximity 0 always matches on the first key.
        let target = [0xaau8; 32];
        let id = Identifier::from_string("test");
        let key = gsoc_mine(&target, &id, 0).unwrap();
        // Sanity: derived key must produce a public key.
        assert!(key.public_key().is_ok());
    }

    #[test]
    fn mining_rejects_wrong_target_length() {
        let id = Identifier::from_string("x");
        assert!(gsoc_mine(&[0u8; 31], &id, 1).is_err());
    }
}
