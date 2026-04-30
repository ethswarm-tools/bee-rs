//! Typed byte newtypes for the Swarm protocol.
//!
//! Mirrors bee-go's `pkg/swarm/typed_bytes.go` (which itself mirrors
//! bee-js's `src/utils/typed-bytes.ts`). Each type is a length-validated
//! wrapper over a fixed-size byte array; on the wire each is encoded as
//! lowercase hex without `0x` prefix.

use std::fmt;
use std::str::FromStr;

use sha3::{Digest, Keccak256};

use crate::swarm::bytes::{decode_hex, encode_hex};
use crate::swarm::errors::Error;

// ---- length constants --------------------------------------------------

/// Length of a plain content reference.
pub const REFERENCE_LENGTH: usize = 32;
/// Length of an encrypted content reference (key suffix appended).
pub const ENCRYPTED_REFERENCE_LENGTH: usize = 64;
/// Length of a postage batch identifier.
pub const BATCH_ID_LENGTH: usize = 32;
/// Length of a transaction hash.
pub const TRANSACTION_ID_LENGTH: usize = 32;
/// Length of a peer overlay address.
pub const PEER_ADDRESS_LENGTH: usize = 32;
/// Length of an arbitrary identifier (SOC / GSOC).
pub const IDENTIFIER_LENGTH: usize = 32;
/// Length of a feed / PSS topic.
pub const TOPIC_LENGTH: usize = 32;
/// Length of an Ethereum address.
pub const ETH_ADDRESS_LENGTH: usize = 20;
/// Length of a secp256k1 private key.
pub const PRIVATE_KEY_LENGTH: usize = 32;
/// Length of a secp256k1 public key (uncompressed `X || Y`).
pub const PUBLIC_KEY_LENGTH: usize = 64;
/// Length of an Ethereum signed-message signature (`R || S || V`).
pub const SIGNATURE_LENGTH: usize = 65;
/// Length of a chunk span (little-endian `u64`).
pub const SPAN_LENGTH: usize = 8;
/// Length of a feed index (big-endian `u64`).
pub const FEED_INDEX_LENGTH: usize = 8;

// ---- macro for fixed-length types --------------------------------------

/// Define a fixed-length typed-byte newtype with hex parsing,
/// `Display` / `Debug` / `LowerHex` / `FromStr` and serde impls.
///
/// Used internally to build the 12 typed wrappers; not part of the
/// public API.
macro_rules! define_typed_bytes {
    (
        $(#[$meta:meta])*
        $name:ident, $len:expr, $kind:literal
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name([u8; $len]);

        impl $name {
            #[doc = concat!("Length in bytes (", stringify!($len), ").")]
            pub const LENGTH: usize = $len;

            #[doc = concat!("Construct a [`", stringify!($name), "`] from bytes. Returns ")]
            #[doc = "[`Error::LengthMismatch`] if `b.len()` does not match."]
            pub fn new(b: &[u8]) -> Result<Self, Error> {
                if b.len() != $len {
                    return Err(Error::LengthMismatch {
                        kind: $kind,
                        expected: &[$len],
                        got: b.len(),
                    });
                }
                let mut a = [0u8; $len];
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

            /// Owned copy of the raw bytes.
            pub fn to_vec(&self) -> Vec<u8> {
                self.0.to_vec()
            }

            /// Lowercase hex, no `0x` prefix.
            pub fn to_hex(&self) -> String {
                encode_hex(&self.0)
            }

            /// Consume into the inner array.
            pub fn into_array(self) -> [u8; $len] {
                self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.to_hex())
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.to_hex())
            }
        }

        impl fmt::LowerHex for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.to_hex())
            }
        }

        impl FromStr for $name {
            type Err = Error;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Self::from_hex(s)
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&self.to_hex())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = String::deserialize(d)?;
                Self::from_hex(&s).map_err(serde::de::Error::custom)
            }
        }
    };
}

// ---- Reference (32 OR 64 bytes) ----------------------------------------

/// Swarm content reference. 32 bytes for a plain CAC reference, 64
/// bytes for an encrypted reference (CAC address || encryption key).
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum Reference {
    /// Plain 32-byte reference (BMT root over the chunk).
    Plain([u8; REFERENCE_LENGTH]),
    /// Encrypted 64-byte reference (32-byte CAC addr || 32-byte key).
    Encrypted([u8; ENCRYPTED_REFERENCE_LENGTH]),
}

impl Reference {
    /// All accepted lengths.
    pub const ALLOWED_LENGTHS: &'static [usize] = &[REFERENCE_LENGTH, ENCRYPTED_REFERENCE_LENGTH];

    /// Construct from raw bytes. Length must be 32 or 64.
    pub fn new(b: &[u8]) -> Result<Self, Error> {
        match b.len() {
            REFERENCE_LENGTH => {
                let mut a = [0u8; REFERENCE_LENGTH];
                a.copy_from_slice(b);
                Ok(Reference::Plain(a))
            }
            ENCRYPTED_REFERENCE_LENGTH => {
                let mut a = [0u8; ENCRYPTED_REFERENCE_LENGTH];
                a.copy_from_slice(b);
                Ok(Reference::Encrypted(a))
            }
            n => Err(Error::LengthMismatch {
                kind: "Reference",
                expected: Self::ALLOWED_LENGTHS,
                got: n,
            }),
        }
    }

    /// Parse from hex (with or without `0x` prefix).
    pub fn from_hex(s: &str) -> Result<Self, Error> {
        Self::new(&decode_hex(s)?)
    }

    /// Borrow the raw bytes (32 or 64).
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Reference::Plain(a) => a,
            Reference::Encrypted(a) => a,
        }
    }

    /// Owned copy of the raw bytes (32 or 64).
    pub fn to_vec(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }

    /// Lowercase hex, no `0x` prefix.
    pub fn to_hex(&self) -> String {
        encode_hex(self.as_bytes())
    }

    /// True for the 64-byte encrypted variant.
    pub fn is_encrypted(&self) -> bool {
        matches!(self, Reference::Encrypted(_))
    }

    /// Length in bytes (32 or 64).
    pub fn len(&self) -> usize {
        self.as_bytes().len()
    }

    /// Whether this reference holds zero bytes — never true for a
    /// constructed `Reference` but useful in default-value paths.
    pub fn is_empty(&self) -> bool {
        false
    }
}

impl fmt::Debug for Reference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Reference({})", self.to_hex())
    }
}

impl fmt::Display for Reference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::LowerHex for Reference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl FromStr for Reference {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_hex(s)
    }
}

impl serde::Serialize for Reference {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> serde::Deserialize<'de> for Reference {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::from_hex(&s).map_err(serde::de::Error::custom)
    }
}

// ---- 32-byte identifiers (no extra behavior) ---------------------------

define_typed_bytes!(
    /// Postage batch identifier (32 bytes).
    BatchId, BATCH_ID_LENGTH, "BatchId"
);

define_typed_bytes!(
    /// Ethereum transaction hash (32 bytes).
    TransactionId, TRANSACTION_ID_LENGTH, "TransactionId"
);

define_typed_bytes!(
    /// Peer overlay address (32 bytes).
    PeerAddress, PEER_ADDRESS_LENGTH, "PeerAddress"
);

// ---- Identifier / Topic with from_string keccak helpers ----------------

define_typed_bytes!(
    /// Arbitrary 32-byte identifier (SOC / GSOC). Use
    /// [`Identifier::from_string`] for the keccak256-of-utf8 variant.
    Identifier, IDENTIFIER_LENGTH, "Identifier"
);

impl Identifier {
    /// `keccak256(utf8(s))` as an identifier. Mirrors bee-js
    /// `Identifier.fromString`.
    pub fn from_string(s: &str) -> Self {
        let mut h = Keccak256::new();
        h.update(s.as_bytes());
        let out = h.finalize();
        let mut a = [0u8; IDENTIFIER_LENGTH];
        a.copy_from_slice(&out);
        Self(a)
    }
}

define_typed_bytes!(
    /// Feed / PSS topic (32 bytes). Use [`Topic::from_string`] for the
    /// keccak256-of-utf8 variant.
    Topic, TOPIC_LENGTH, "Topic"
);

impl Topic {
    /// `keccak256(utf8(s))` as a topic. Mirrors bee-js
    /// `Topic.fromString`.
    pub fn from_string(s: &str) -> Self {
        let mut h = Keccak256::new();
        h.update(s.as_bytes());
        let out = h.finalize();
        let mut a = [0u8; TOPIC_LENGTH];
        a.copy_from_slice(&out);
        Self(a)
    }
}

// ---- EthAddress with EIP-55 checksum -----------------------------------

define_typed_bytes!(
    /// Ethereum address (20 bytes). [`EthAddress::to_checksum`] returns
    /// the EIP-55 mixed-case representation with `0x` prefix.
    EthAddress, ETH_ADDRESS_LENGTH, "EthAddress"
);

impl EthAddress {
    /// EIP-55 checksum representation, `0x`-prefixed.
    pub fn to_checksum(&self) -> String {
        let lower = self.to_hex();
        let mut h = Keccak256::new();
        h.update(lower.as_bytes());
        let hash = h.finalize();
        let mut out = String::with_capacity(2 + lower.len());
        out.push_str("0x");
        for (i, c) in lower.chars().enumerate() {
            if c.is_ascii_digit() {
                out.push(c);
            } else {
                // Each hex char maps to 4 bits; nibble of hash[i/2].
                let nibble = if i % 2 == 0 {
                    hash[i / 2] >> 4
                } else {
                    hash[i / 2] & 0x0f
                };
                if nibble >= 8 {
                    out.push(c.to_ascii_uppercase());
                } else {
                    out.push(c);
                }
            }
        }
        out
    }
}

// ---- Span (8 bytes, little-endian u64) ---------------------------------

define_typed_bytes!(
    /// Chunk span: 8 bytes, little-endian `u64`.
    Span, SPAN_LENGTH, "Span"
);

impl Span {
    /// Encode `n` as a little-endian span.
    pub fn from_u64(n: u64) -> Self {
        Self(n.to_le_bytes())
    }

    /// Decode the little-endian `u64`.
    pub fn to_u64(&self) -> u64 {
        u64::from_le_bytes(self.0)
    }
}

// ---- FeedIndex (8 bytes, big-endian u64) -------------------------------

define_typed_bytes!(
    /// Feed index: 8 bytes, big-endian `u64`. The all-`0xff` value is a
    /// "before first" sentinel matching bee-js `FeedIndex.MINUS_ONE`.
    FeedIndex, FEED_INDEX_LENGTH, "FeedIndex"
);

impl FeedIndex {
    /// All-`0xff` "before first" / wraparound sentinel.
    pub const MINUS_ONE: FeedIndex = FeedIndex([0xff; FEED_INDEX_LENGTH]);

    /// Encode `n` as a big-endian feed index.
    pub fn from_u64(n: u64) -> Self {
        Self(n.to_be_bytes())
    }

    /// Decode the big-endian `u64`. Returns `u64::MAX` for `MINUS_ONE`.
    pub fn to_u64(&self) -> u64 {
        u64::from_be_bytes(self.0)
    }

    /// Successor index. The `MINUS_ONE` sentinel wraps to `0`.
    pub fn next(&self) -> Self {
        if self == &Self::MINUS_ONE {
            Self::from_u64(0)
        } else {
            Self::from_u64(self.to_u64() + 1)
        }
    }
}

// ---- Signature (65 bytes, R || S || V) ---------------------------------

define_typed_bytes!(
    /// Ethereum signed-message signature: `R || S || V` with V
    /// normalized to `{27, 28}` on the wire, matching bee-js.
    Signature, SIGNATURE_LENGTH, "Signature"
);

// PrivateKey, PublicKey: defined in [`crate::swarm::keys`] so the
// crypto-heavy parts don't bloat this file.

// ---- tests -------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip_with_and_without_0x_prefix() {
        let hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let a = BatchId::from_hex(hex).unwrap();
        let b = BatchId::from_hex(&format!("0x{hex}")).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.to_hex(), hex);
    }

    #[test]
    fn fixed_length_rejects_wrong_size() {
        assert!(BatchId::new(&[0u8; 31]).is_err());
        assert!(BatchId::new(&[0u8; 33]).is_err());
        assert!(BatchId::new(&[0u8; 32]).is_ok());
    }

    #[test]
    fn reference_accepts_32_or_64() {
        let r32 = "ab".repeat(32);
        let r64 = "cd".repeat(64);
        let a = Reference::from_hex(&r32).unwrap();
        let b = Reference::from_hex(&r64).unwrap();
        assert!(!a.is_encrypted());
        assert!(b.is_encrypted());
        assert_eq!(a.to_hex(), r32);
        assert_eq!(b.to_hex(), r64);
        assert!(Reference::from_hex(&"ab".repeat(31)).is_err());
        assert!(Reference::from_hex(&"ab".repeat(48)).is_err());
    }

    #[test]
    fn identifier_from_string_is_keccak256_utf8() {
        // keccak256("hello") (Ethereum keccak, not SHA3-256)
        let want = "1c8aff950685c2ed4bc3174f3472287b56d9517b9c948127319a09a7a36deac8";
        let id = Identifier::from_string("hello");
        assert_eq!(id.to_hex(), want);
    }

    #[test]
    fn topic_from_string_is_keccak256_utf8() {
        let want_a = Topic::from_string("my-topic");
        let want_b = Topic::from_string("my-topic");
        assert_eq!(want_a, want_b);
        assert_ne!(want_a, Topic::from_string("other"));
    }

    #[test]
    fn eth_address_eip55_checksum() {
        let raw = "fb6916095ca1df60bb79ce92ce3ea74c37c5d359";
        let addr = EthAddress::from_hex(raw).unwrap();
        assert_eq!(
            addr.to_checksum(),
            "0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359"
        );
    }

    #[test]
    fn span_round_trip_little_endian() {
        for n in [0u64, 1, 4096, 1 << 40] {
            let s = Span::from_u64(n);
            assert_eq!(s.to_u64(), n);
        }
        // 1 → 01 00 00 00 00 00 00 00
        assert_eq!(Span::from_u64(1).as_bytes(), &[1, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn feed_index_round_trip_big_endian_and_next() {
        for n in [0u64, 1, 100, (1u64 << 32) - 1] {
            assert_eq!(FeedIndex::from_u64(n).to_u64(), n);
        }
        // 1 → 00 00 00 00 00 00 00 01
        assert_eq!(FeedIndex::from_u64(1).as_bytes(), &[0, 0, 0, 0, 0, 0, 0, 1]);
        assert_eq!(FeedIndex::from_u64(5).next().to_u64(), 6);
        assert_eq!(FeedIndex::MINUS_ONE.next().to_u64(), 0);
    }

    #[test]
    fn serde_round_trip() {
        let r = Reference::from_hex(&"ab".repeat(32)).unwrap();
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, format!("\"{}\"", "ab".repeat(32)));
        let r2: Reference = serde_json::from_str(&json).unwrap();
        assert_eq!(r, r2);
    }

    #[test]
    fn from_str_works() {
        let id: BatchId = "00".repeat(32).parse().unwrap();
        assert_eq!(id.as_bytes(), &[0u8; 32]);
    }

    #[test]
    fn debug_format_includes_type_name() {
        let id = BatchId::new(&[0xab; 32]).unwrap();
        let s = format!("{id:?}");
        assert!(s.starts_with("BatchId("));
        assert!(s.contains(&"ab".repeat(32)));
    }
}
