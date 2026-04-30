//! Swarm reference ↔ CID conversion (manifest and feed codecs).
//!
//! Mirrors `pkg/swarm/cid.go` in bee-go and `src/utils/cid.ts` in
//! bee-js. The encoding is base32 (RFC4648, no padding), lowercased,
//! prefixed with `b` per the multibase spec.

use crate::swarm::errors::Error;
use crate::swarm::typed_bytes::Reference;

/// Multicodec for the Swarm manifest CID type.
pub const MANIFEST_CODEC: u8 = 0xfa;
/// Multicodec for the Swarm feed CID type.
pub const FEED_CODEC: u8 = 0xfb;

/// CID-encoded reference type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CidType {
    /// Manifest CID.
    Manifest,
    /// Feed CID.
    Feed,
}

impl CidType {
    fn codec(self) -> u8 {
        match self {
            CidType::Manifest => MANIFEST_CODEC,
            CidType::Feed => FEED_CODEC,
        }
    }

    fn from_codec(c: u8) -> Result<Self, Error> {
        match c {
            MANIFEST_CODEC => Ok(CidType::Manifest),
            FEED_CODEC => Ok(CidType::Feed),
            other => Err(Error::argument(format!("unknown CID codec: 0x{other:02x}"))),
        }
    }
}

/// Decoded CID payload returned by [`convert_cid_to_reference`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedCid {
    /// The CID type recovered from the multicodec byte.
    pub kind: CidType,
    /// The Swarm reference embedded in the CID.
    pub reference: Reference,
}

/// Encode a 32-byte Swarm reference as a CID string.
///
/// Output is `"b" + base32(header) + base32(reference)` lowercased,
/// where `header = [version=1, codec, _=1, sha256=0x1b, size=32]`.
pub fn convert_reference_to_cid(reference: &Reference, kind: CidType) -> Result<String, Error> {
    if reference.is_encrypted() {
        return Err(Error::argument(
            "encrypted references (64 bytes) cannot be encoded as a CID",
        ));
    }
    let header: [u8; 5] = [1, kind.codec(), 1, 0x1b, 32];
    let mut out = String::with_capacity(1 + 8 + 52);
    out.push('b');
    out.push_str(&b32_encode(&header).to_ascii_lowercase());
    out.push_str(&b32_encode(reference.as_bytes()).to_ascii_lowercase());
    Ok(out)
}

/// Decode a CID string into its codec type + Swarm reference.
pub fn convert_cid_to_reference(cid: &str) -> Result<DecodedCid, Error> {
    let body = cid
        .strip_prefix('b')
        .ok_or_else(|| Error::argument("CID must start with multibase prefix 'b'"))?;
    let bytes = b32_decode(&body.to_ascii_uppercase())?;
    if bytes.len() < 32 {
        return Err(Error::argument(format!(
            "decoded CID too short: {} bytes",
            bytes.len()
        )));
    }
    // bee-js / bee-go convention: codec is byte index 1; reference is
    // the last 32 bytes.
    let kind = CidType::from_codec(bytes[1])?;
    let ref_bytes = &bytes[bytes.len() - 32..];
    let reference = Reference::new(ref_bytes)?;
    Ok(DecodedCid { kind, reference })
}

// ---- minimal RFC4648 base32 (NoPadding) --------------------------------

const B32_ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn b32_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(5) * 8);
    let mut buffer: u64 = 0;
    let mut bits: u32 = 0;
    for &b in input {
        buffer = (buffer << 8) | b as u64;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            let idx = ((buffer >> bits) & 0x1f) as usize;
            out.push(B32_ALPHABET[idx] as char);
        }
    }
    if bits > 0 {
        let idx = ((buffer << (5 - bits)) & 0x1f) as usize;
        out.push(B32_ALPHABET[idx] as char);
    }
    out
}

fn b32_decode(input: &str) -> Result<Vec<u8>, Error> {
    let mut out = Vec::with_capacity(input.len() * 5 / 8);
    let mut buffer: u64 = 0;
    let mut bits: u32 = 0;
    for c in input.chars() {
        let v: u32 = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            '2'..='7' => 26 + (c as u32 - '2' as u32),
            other => {
                return Err(Error::argument(format!(
                    "invalid base32 character: {other:?}"
                )));
            }
        };
        buffer = (buffer << 5) | v as u64;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_feed() {
        // Same fixture as bee-go's TestCID.
        let hex = "ca6357a08e317d15ec560fef34e4c45f8f19f01c75d6f20a7021602e9575a617";
        let reference = Reference::from_hex(hex).unwrap();
        let cid = convert_reference_to_cid(&reference, CidType::Feed).unwrap();
        assert!(cid.starts_with('b'));
        let decoded = convert_cid_to_reference(&cid).unwrap();
        assert_eq!(decoded.kind, CidType::Feed);
        assert_eq!(decoded.reference.to_hex(), hex);
    }

    #[test]
    fn round_trip_manifest() {
        let hex = "f".repeat(64);
        let reference = Reference::from_hex(&hex).unwrap();
        let cid = convert_reference_to_cid(&reference, CidType::Manifest).unwrap();
        let decoded = convert_cid_to_reference(&cid).unwrap();
        assert_eq!(decoded.kind, CidType::Manifest);
        assert_eq!(decoded.reference.to_hex(), hex);
    }

    #[test]
    fn rejects_encrypted_reference() {
        let reference = Reference::from_hex(&"a".repeat(128)).unwrap();
        assert!(convert_reference_to_cid(&reference, CidType::Feed).is_err());
    }

    #[test]
    fn rejects_unknown_codec() {
        // Hand-craft a CID with an invalid codec byte.
        let header: [u8; 5] = [1, 0x55, 1, 0x1b, 32];
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&header);
        bytes.extend_from_slice(&[0u8; 32]);
        let cid = format!("b{}", b32_encode(&bytes).to_ascii_lowercase());
        assert!(convert_cid_to_reference(&cid).is_err());
    }

    #[test]
    fn rejects_missing_prefix() {
        assert!(convert_cid_to_reference("acafef00").is_err());
    }

    #[test]
    fn rejects_short_cid() {
        let cid = format!("b{}", b32_encode(&[0u8; 8]).to_ascii_lowercase());
        assert!(convert_cid_to_reference(&cid).is_err());
    }

    #[test]
    fn base32_encode_matches_rfc4648_vectors() {
        // RFC 4648 §10 test vectors (no padding).
        assert_eq!(b32_encode(b""), "");
        assert_eq!(b32_encode(b"f"), "MY");
        assert_eq!(b32_encode(b"fo"), "MZXQ");
        assert_eq!(b32_encode(b"foo"), "MZXW6");
        assert_eq!(b32_encode(b"foob"), "MZXW6YQ");
        assert_eq!(b32_encode(b"fooba"), "MZXW6YTB");
        assert_eq!(b32_encode(b"foobar"), "MZXW6YTBOI");
    }

    #[test]
    fn base32_decode_matches_rfc4648_vectors() {
        assert_eq!(b32_decode("").unwrap(), b"");
        assert_eq!(b32_decode("MY").unwrap(), b"f");
        assert_eq!(b32_decode("MZXQ").unwrap(), b"fo");
        assert_eq!(b32_decode("MZXW6").unwrap(), b"foo");
        assert_eq!(b32_decode("MZXW6YQ").unwrap(), b"foob");
        assert_eq!(b32_decode("MZXW6YTB").unwrap(), b"fooba");
        assert_eq!(b32_decode("MZXW6YTBOI").unwrap(), b"foobar");
    }
}
