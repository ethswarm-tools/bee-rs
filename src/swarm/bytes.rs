//! Hex parsing / display helpers shared by every typed-byte newtype.
//! Mirrors bee-go's `pkg/swarm/bytes.go` `Bytes` base type, but in
//! Rust we don't need a runtime base struct — instead we expose
//! free helpers that the [`define_typed_bytes!`](crate::define_typed_bytes)
//! macro uses to build each newtype's `from_hex`, `to_hex`, and
//! serde impls.

use crate::swarm::errors::Error;

/// Decode a hex string (with or without `0x` prefix).
pub fn decode_hex(s: &str) -> Result<Vec<u8>, Error> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    Ok(hex::decode(s)?)
}

/// Encode bytes as lowercase hex (no `0x` prefix), matching bee-js /
/// bee-go wire format.
pub fn encode_hex(b: &[u8]) -> String {
    hex::encode(b)
}
