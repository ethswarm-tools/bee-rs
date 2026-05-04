//! `PrivateKey` / `PublicKey` and the Ethereum signed-message signer.
//!
//! Mirrors bee-go's `pkg/swarm/typed_bytes.go` crypto block. Uses
//! pure-Rust [`k256`] (RustCrypto secp256k1) so we don't pull in a
//! libsecp C dependency.
//!
//! The scheme matches bee-js:
//!
//! ```text
//! digest = keccak256("\x19Ethereum Signed Message:\n32" || keccak256(data))
//! ```
//!
//! and signatures are stored with `V ∈ {27, 28}` on the wire (v0
//! / v1 are normalized on the way out, denormalized on the way in).

use std::fmt;
use std::str::FromStr;

use k256::ecdsa::{
    RecoveryId, Signature as K256Signature, SigningKey, VerifyingKey,
    signature::hazmat::PrehashSigner,
};
use sha3::{Digest, Keccak256};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::swarm::bytes::{decode_hex, encode_hex};
use crate::swarm::errors::Error;
use crate::swarm::typed_bytes::{
    ETH_ADDRESS_LENGTH, EthAddress, PRIVATE_KEY_LENGTH, PUBLIC_KEY_LENGTH, SIGNATURE_LENGTH,
    Signature,
};

// ---- PrivateKey --------------------------------------------------------

/// secp256k1 private key (32 bytes).
///
/// Not [`Copy`] — copies of secret material should be intentional.
/// Implements [`ZeroizeOnDrop`] so the bytes are scrubbed from memory
/// when the value is dropped, and [`PartialEq`] is constant-time.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct PrivateKey([u8; PRIVATE_KEY_LENGTH]);

impl PartialEq for PrivateKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.ct_eq(&other.0).into()
    }
}

impl Eq for PrivateKey {}

impl PrivateKey {
    /// Length in bytes.
    pub const LENGTH: usize = PRIVATE_KEY_LENGTH;

    /// Construct from raw bytes.
    pub fn new(b: &[u8]) -> Result<Self, Error> {
        if b.len() != PRIVATE_KEY_LENGTH {
            return Err(Error::LengthMismatch {
                kind: "PrivateKey",
                expected: &[PRIVATE_KEY_LENGTH],
                got: b.len(),
            });
        }
        let mut a = [0u8; PRIVATE_KEY_LENGTH];
        a.copy_from_slice(b);
        Ok(Self(a))
    }

    /// Parse from hex (with or without `0x` prefix).
    pub fn from_hex(s: &str) -> Result<Self, Error> {
        Self::new(&decode_hex(s)?)
    }

    /// Borrow the raw bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Lowercase hex, no `0x` prefix.
    pub fn to_hex(&self) -> String {
        encode_hex(&self.0)
    }

    fn signing_key(&self) -> Result<SigningKey, Error> {
        SigningKey::from_slice(&self.0).map_err(Error::crypto)
    }

    /// Derive the uncompressed (64-byte `X || Y`) public key.
    pub fn public_key(&self) -> Result<PublicKey, Error> {
        let sk = self.signing_key()?;
        let vk = VerifyingKey::from(&sk);
        let point = vk.to_encoded_point(false);
        // SEC1 uncompressed: 0x04 || X(32) || Y(32). Strip the prefix.
        let bytes = point.as_bytes();
        debug_assert_eq!(bytes.len(), 65);
        let mut a = [0u8; PUBLIC_KEY_LENGTH];
        a.copy_from_slice(&bytes[1..]);
        Ok(PublicKey(a))
    }

    /// Sign `data` using the Ethereum signed-message scheme and return
    /// a 65-byte `R || S || V` signature with `V ∈ {27, 28}`.
    pub fn sign(&self, data: &[u8]) -> Result<Signature, Error> {
        let digest = eth_signed_message_digest(data);
        let sk = self.signing_key()?;
        let (sig, recovery_id): (K256Signature, RecoveryId) =
            sk.sign_prehash(&digest).map_err(Error::crypto)?;
        let normalized = sig.normalize_s().unwrap_or(sig);
        // k256 returns the recovery id corresponding to the original sig;
        // when normalize_s flips s, the recovery id also flips parity.
        let recovery_id = if normalized != sig {
            RecoveryId::from_byte(recovery_id.to_byte() ^ 1).expect("flipped recovery id valid")
        } else {
            recovery_id
        };

        let mut out = [0u8; SIGNATURE_LENGTH];
        let bytes = normalized.to_bytes();
        out[..64].copy_from_slice(&bytes);
        out[64] = recovery_id.to_byte() + 27;
        Signature::new(&out)
    }
}

impl fmt::Debug for PrivateKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Don't leak private bytes in Debug output.
        f.write_str("PrivateKey(<redacted>)")
    }
}

impl FromStr for PrivateKey {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_hex(s)
    }
}

// ---- PublicKey ---------------------------------------------------------

/// Uncompressed secp256k1 public key (`X || Y`, 64 bytes).
///
/// Constructors accept the 33-byte SEC1-compressed encoding too.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PublicKey([u8; PUBLIC_KEY_LENGTH]);

impl PublicKey {
    /// Length in bytes when stored uncompressed.
    pub const LENGTH: usize = PUBLIC_KEY_LENGTH;

    /// Construct from bytes. Accepts 64-byte uncompressed (`X || Y`)
    /// or 33-byte compressed (SEC1).
    pub fn new(b: &[u8]) -> Result<Self, Error> {
        match b.len() {
            PUBLIC_KEY_LENGTH => {
                let mut a = [0u8; PUBLIC_KEY_LENGTH];
                a.copy_from_slice(b);
                Ok(Self(a))
            }
            33 => {
                let vk = VerifyingKey::from_sec1_bytes(b).map_err(Error::crypto)?;
                let point = vk.to_encoded_point(false);
                let bytes = point.as_bytes();
                let mut a = [0u8; PUBLIC_KEY_LENGTH];
                a.copy_from_slice(&bytes[1..]);
                Ok(Self(a))
            }
            n => Err(Error::LengthMismatch {
                kind: "PublicKey",
                expected: &[33, PUBLIC_KEY_LENGTH],
                got: n,
            }),
        }
    }

    /// Parse from hex (with or without `0x` prefix). Length must be 33
    /// (compressed) or 64 (uncompressed).
    pub fn from_hex(s: &str) -> Result<Self, Error> {
        Self::new(&decode_hex(s)?)
    }

    /// Borrow the raw 64-byte uncompressed encoding.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Lowercase hex of the uncompressed encoding, no `0x` prefix.
    pub fn to_hex(&self) -> String {
        encode_hex(&self.0)
    }

    /// Ethereum address: last 20 bytes of `keccak256(X || Y)`.
    pub fn address(&self) -> EthAddress {
        let mut h = Keccak256::new();
        h.update(self.0);
        let out = h.finalize();
        let mut a = [0u8; ETH_ADDRESS_LENGTH];
        a.copy_from_slice(&out[12..]);
        EthAddress::new(&a).expect("hash slice has fixed length")
    }

    /// 33-byte SEC1-compressed encoding.
    pub fn compressed_bytes(&self) -> Result<[u8; 33], Error> {
        let mut full = [0u8; 65];
        full[0] = 0x04;
        full[1..].copy_from_slice(&self.0);
        let vk = VerifyingKey::from_sec1_bytes(&full).map_err(Error::crypto)?;
        let point = vk.to_encoded_point(true);
        let bytes = point.as_bytes();
        if bytes.len() != 33 {
            return Err(Error::crypto("compressed point not 33 bytes"));
        }
        let mut a = [0u8; 33];
        a.copy_from_slice(bytes);
        Ok(a)
    }

    /// Lowercase hex of the compressed encoding, no `0x` prefix.
    pub fn compressed_hex(&self) -> Result<String, Error> {
        Ok(encode_hex(&self.compressed_bytes()?))
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PublicKey({})", self.to_hex())
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl FromStr for PublicKey {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_hex(s)
    }
}

// ---- Signature recovery ------------------------------------------------

impl Signature {
    /// Recover the public key that produced this signature for `data`,
    /// using the Ethereum signed-message scheme.
    pub fn recover_public_key(&self, data: &[u8]) -> Result<PublicKey, Error> {
        let digest = eth_signed_message_digest(data);
        let bytes = self.as_bytes();
        let v = bytes[64];
        let recovery_byte = if v >= 27 { v - 27 } else { v };
        let recovery_id =
            RecoveryId::from_byte(recovery_byte).ok_or_else(|| Error::crypto("invalid V byte"))?;
        let sig = K256Signature::from_slice(&bytes[..64]).map_err(Error::crypto)?;
        let vk = VerifyingKey::recover_from_prehash(&digest, &sig, recovery_id)
            .map_err(Error::crypto)?;
        let point = vk.to_encoded_point(false);
        let raw = point.as_bytes();
        let mut a = [0u8; PUBLIC_KEY_LENGTH];
        a.copy_from_slice(&raw[1..]);
        Ok(PublicKey(a))
    }

    /// True iff the signature is valid against `data` and the recovered
    /// signer matches `expected`.
    pub fn is_valid(&self, data: &[u8], expected: EthAddress) -> bool {
        match self.recover_public_key(data) {
            Ok(pk) => pk.address() == expected,
            Err(_) => false,
        }
    }
}

// ---- digest helper -----------------------------------------------------

/// `keccak256("\x19Ethereum Signed Message:\n32" || keccak256(data))`.
/// The exact digest Bee verifies SOC and feed signatures against.
pub fn eth_signed_message_digest(data: &[u8]) -> [u8; 32] {
    let mut h = Keccak256::new();
    h.update(data);
    let inner = h.finalize();

    let mut h = Keccak256::new();
    h.update(b"\x19Ethereum Signed Message:\n32");
    h.update(inner);
    let out = h.finalize();
    let mut a = [0u8; 32];
    a.copy_from_slice(&out);
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    fn priv_repeat(byte: u8) -> PrivateKey {
        PrivateKey::new(&[byte; PRIVATE_KEY_LENGTH]).unwrap()
    }

    #[test]
    fn private_to_public_to_address_is_consistent() {
        let pk = priv_repeat(0x11);
        let pub_a = pk.public_key().unwrap();
        let pub_b = pk.public_key().unwrap();
        assert_eq!(pub_a.as_bytes(), pub_b.as_bytes());
        assert_eq!(pub_a.address(), pub_b.address());
    }

    #[test]
    fn public_key_compressed_round_trip() {
        let pk = priv_repeat(0x22);
        let pub_a = pk.public_key().unwrap();
        let compressed = pub_a.compressed_bytes().unwrap();
        assert_eq!(compressed.len(), 33);
        let pub_b = PublicKey::new(&compressed).unwrap();
        assert_eq!(pub_a.as_bytes(), pub_b.as_bytes());
    }

    #[test]
    fn sign_recover_round_trip() {
        let pk = priv_repeat(0x33);
        let data = b"hello swarm";
        let sig = pk.sign(data).unwrap();
        // V normalized to {27, 28} per bee-js wire format.
        let v = sig.as_bytes()[64];
        assert!(v == 27 || v == 28, "V was {v}");
        let recovered = sig.recover_public_key(data).unwrap();
        assert_eq!(recovered.as_bytes(), pk.public_key().unwrap().as_bytes());
        assert!(sig.is_valid(data, pk.public_key().unwrap().address()));
        assert!(!sig.is_valid(b"tampered", pk.public_key().unwrap().address()));
    }

    #[test]
    fn debug_does_not_leak_private_bytes() {
        let pk = priv_repeat(0x44);
        let s = format!("{pk:?}");
        assert!(!s.contains("44"));
        assert!(s.contains("redacted"));
    }

    #[test]
    fn zeroize_clears_private_bytes() {
        let mut pk = priv_repeat(0x55);
        assert_eq!(pk.as_bytes(), &[0x55; PRIVATE_KEY_LENGTH]);
        pk.zeroize();
        assert_eq!(pk.as_bytes(), &[0u8; PRIVATE_KEY_LENGTH]);
    }

    #[test]
    fn private_key_is_zeroize_on_drop() {
        // Compile-time bound: ZeroizeOnDrop is what we promise in the
        // type's docs; this assertion fails to compile if the trait is
        // ever removed.
        fn assert_zod<T: zeroize::ZeroizeOnDrop>() {}
        assert_zod::<PrivateKey>();
    }

    #[test]
    fn private_key_eq_is_correct() {
        let a = priv_repeat(0x66);
        let b = priv_repeat(0x66);
        let c = priv_repeat(0x77);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
