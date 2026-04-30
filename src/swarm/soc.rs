//! Single Owner Chunks (SOC). Mirrors bee-go's `pkg/swarm/soc.go`.
//!
//! A SOC is a chunk addressed by `keccak256(identifier || ownerAddress)`
//! rather than by content hash. The owner signs `identifier || cacAddress`
//! using the Ethereum signed-message scheme (`V ∈ {27, 28}` on the wire).
//! Bee verifies the signature against that scheme — bee-go's first cut
//! used raw keccak with `V ∈ {0, 1}` and every SOC upload returned
//! 401, so this is one of the spots that *will* break against a live
//! node if you get it wrong.

use crate::swarm::bmt::{calculate_chunk_address, keccak256};
use crate::swarm::errors::Error;
use crate::swarm::keys::PrivateKey;
use crate::swarm::typed_bytes::{
    EthAddress, IDENTIFIER_LENGTH, Identifier, Reference, SIGNATURE_LENGTH, SPAN_LENGTH, Signature,
    Span,
};

/// A single-owner chunk. The signature commits to `identifier ||
/// cacAddress`, where `cacAddress` is the BMT address of `span ||
/// payload`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SingleOwnerChunk {
    /// SOC identifier.
    pub identifier: Identifier,
    /// 65-byte signature over `identifier || cacAddress`.
    pub signature: Signature,
    /// Recovered owner address (last 20 bytes of `keccak(pubkey)`).
    pub owner: EthAddress,
    /// 8-byte little-endian span.
    pub span: Span,
    /// Raw payload (≤ 4096 bytes).
    pub payload: Vec<u8>,
}

impl SingleOwnerChunk {
    /// Compute this SOC's address: `keccak256(identifier || owner)`.
    pub fn address(&self) -> Result<Reference, Error> {
        calculate_single_owner_chunk_address(&self.identifier, &self.owner)
    }

    /// Wire form: `identifier (32) || signature (65) || span (8) || payload`.
    /// Suitable as the body of `POST /soc/{owner}/{id}` once `cacAddress`
    /// is in the URL.
    pub fn data(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            IDENTIFIER_LENGTH + SIGNATURE_LENGTH + SPAN_LENGTH + self.payload.len(),
        );
        out.extend_from_slice(self.identifier.as_bytes());
        out.extend_from_slice(self.signature.as_bytes());
        out.extend_from_slice(self.span.as_bytes());
        out.extend_from_slice(&self.payload);
        out
    }
}

/// Compute the SOC address for a given identifier and owner —
/// `keccak256(identifier || owner)`. Mirrors bee-js
/// `calculateSingleOwnerChunkAddress`.
pub fn calculate_single_owner_chunk_address(
    identifier: &Identifier,
    owner: &EthAddress,
) -> Result<Reference, Error> {
    let mut buf = Vec::with_capacity(identifier.as_bytes().len() + owner.as_bytes().len());
    buf.extend_from_slice(identifier.as_bytes());
    buf.extend_from_slice(owner.as_bytes());
    let addr = keccak256(&buf);
    Reference::new(&addr)
}

/// Build a SOC for `(identifier, payload)`, signed by `signer`. Mirrors
/// bee-js `makeSingleOwnerChunk` and bee-go `CreateSOC` /
/// `MakeSingleOwnerChunk`.
pub fn make_single_owner_chunk(
    identifier: &Identifier,
    payload: &[u8],
    signer: &PrivateKey,
) -> Result<SingleOwnerChunk, Error> {
    if payload.len() > crate::swarm::bmt::MAX_PAYLOAD_SIZE {
        return Err(Error::argument(format!(
            "SOC payload too large: {}",
            payload.len()
        )));
    }
    let span = Span::from_u64(payload.len() as u64);

    // Compute CAC address over span || payload.
    let mut full = Vec::with_capacity(SPAN_LENGTH + payload.len());
    full.extend_from_slice(span.as_bytes());
    full.extend_from_slice(payload);
    let cac_addr = calculate_chunk_address(&full)?;

    // Sign identifier || cacAddress.
    let mut to_sign = Vec::with_capacity(IDENTIFIER_LENGTH + 32);
    to_sign.extend_from_slice(identifier.as_bytes());
    to_sign.extend_from_slice(&cac_addr);
    let signature = signer.sign(&to_sign)?;

    let owner = signer.public_key()?.address();

    Ok(SingleOwnerChunk {
        identifier: *identifier,
        signature,
        owner,
        span,
        payload: payload.to_vec(),
    })
}

/// Parse the wire form of a SOC chunk
/// (`identifier(32) || signature(65) || span(8) || payload(≤4096)`)
/// and verify it matches `expected_address`. Recovers the owner from
/// the signature using the eth-signed-message scheme.
pub fn unmarshal_single_owner_chunk(
    data: &[u8],
    expected_address: &Reference,
) -> Result<SingleOwnerChunk, Error> {
    let min_len = IDENTIFIER_LENGTH + SIGNATURE_LENGTH + SPAN_LENGTH;
    if data.len() < min_len {
        return Err(Error::argument(format!(
            "SOC data too short: {}",
            data.len()
        )));
    }

    let identifier = Identifier::new(&data[..IDENTIFIER_LENGTH])?;
    let sig_start = IDENTIFIER_LENGTH;
    let span_start = sig_start + SIGNATURE_LENGTH;
    let payload_start = span_start + SPAN_LENGTH;

    let signature = Signature::new(&data[sig_start..span_start])?;
    let span = Span::new(&data[span_start..payload_start])?;
    let payload = data[payload_start..].to_vec();

    // Recompute cac address over span || payload.
    let mut cac_data = Vec::with_capacity(SPAN_LENGTH + payload.len());
    cac_data.extend_from_slice(span.as_bytes());
    cac_data.extend_from_slice(&payload);
    let cac_addr = calculate_chunk_address(&cac_data)?;

    // Recover owner from signature over identifier || cacAddress.
    let mut signed = Vec::with_capacity(IDENTIFIER_LENGTH + 32);
    signed.extend_from_slice(identifier.as_bytes());
    signed.extend_from_slice(&cac_addr);
    let pub_key = signature.recover_public_key(&signed)?;
    let owner = pub_key.address();

    // Verify keccak256(id || owner) == expected.
    let mut soc_input = Vec::with_capacity(IDENTIFIER_LENGTH + owner.as_bytes().len());
    soc_input.extend_from_slice(identifier.as_bytes());
    soc_input.extend_from_slice(owner.as_bytes());
    let computed = keccak256(&soc_input);
    if computed != expected_address.as_bytes() {
        return Err(Error::argument("SOC data does not match expected address"));
    }

    Ok(SingleOwnerChunk {
        identifier,
        signature,
        owner,
        span,
        payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swarm::typed_bytes::PRIVATE_KEY_LENGTH;

    fn signer(byte: u8) -> PrivateKey {
        PrivateKey::new(&[byte; PRIVATE_KEY_LENGTH]).unwrap()
    }

    #[test]
    fn make_and_unmarshal_round_trip() {
        let id = Identifier::from_string("test-identifier");
        let pk = signer(0x55);
        let payload = b"single owner chunk payload";

        let soc = make_single_owner_chunk(&id, payload, &pk).unwrap();

        // Owner must match signer's address.
        assert_eq!(soc.owner, pk.public_key().unwrap().address());
        // V byte is normalized to {27, 28}.
        let v = soc.signature.as_bytes()[64];
        assert!(v == 27 || v == 28);

        let address = soc.address().unwrap();
        let parsed = unmarshal_single_owner_chunk(&soc.data(), &address).unwrap();
        assert_eq!(parsed.identifier, soc.identifier);
        assert_eq!(parsed.signature, soc.signature);
        assert_eq!(parsed.owner, soc.owner);
        assert_eq!(parsed.span, soc.span);
        assert_eq!(parsed.payload, soc.payload);
    }

    #[test]
    fn unmarshal_rejects_wrong_address() {
        let id = Identifier::from_string("id");
        let pk = signer(0x66);
        let soc = make_single_owner_chunk(&id, b"x", &pk).unwrap();

        let wrong = Reference::new(&[0u8; 32]).unwrap();
        assert!(unmarshal_single_owner_chunk(&soc.data(), &wrong).is_err());
    }

    #[test]
    fn unmarshal_rejects_short_data() {
        let any = Reference::new(&[0u8; 32]).unwrap();
        assert!(unmarshal_single_owner_chunk(&[0u8; 10], &any).is_err());
    }

    #[test]
    fn calculate_address_is_keccak_id_owner() {
        let id = Identifier::from_string("hello");
        let owner = EthAddress::from_hex("fb6916095ca1df60bb79ce92ce3ea74c37c5d359").unwrap();
        let addr = calculate_single_owner_chunk_address(&id, &owner).unwrap();

        let mut input = Vec::new();
        input.extend_from_slice(id.as_bytes());
        input.extend_from_slice(owner.as_bytes());
        let want = crate::swarm::bmt::keccak256(&input);
        assert_eq!(addr.as_bytes(), want);
    }
}
