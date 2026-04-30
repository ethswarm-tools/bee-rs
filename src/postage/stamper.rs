//! Client-side postage stamper.
//!
//! Mirrors `pkg/postage/stamper.go` in bee-go and
//! `src/stamper/stamper.ts` in bee-js. Lets a caller produce a
//! per-chunk [`Envelope`] without round-tripping the node, which is
//! the primitive needed for `postEnvelope`-style flows and for
//! progressive uploads.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::swarm::errors::Error;
use crate::swarm::keys::PrivateKey;
use crate::swarm::typed_bytes::{BatchId, EthAddress, Signature};

/// Number of buckets in a postage batch (`2^16`).
pub const NUM_BUCKETS: usize = 1 << 16;

/// Bucket-depth floor: stamper depth must be **strictly greater than**
/// this value (matches bee-go and bee-js, which require `depth > 16`).
pub const MIN_DEPTH: u8 = 16;

/// Wire-format length of a marshaled postage stamp:
/// `batchID(32) || index(8) || timestamp(8) || signature(65)`.
pub const MARSHALED_STAMP_LENGTH: usize = 32 + 8 + 8 + 65;

/// Per-chunk postage envelope returned by [`Stamper::stamp`].
///
/// Mirrors bee-js `EnvelopeWithBatchId` and bee-go `postage.Envelope`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Envelope {
    /// Batch the chunk is stamped against.
    pub batch_id: BatchId,
    /// 8 bytes: `bucket (BE u32) || height (BE u32)`.
    pub index: [u8; 8],
    /// Issuer (signer's Ethereum address).
    pub issuer: EthAddress,
    /// 65-byte `R || S || V` signature with `V ∈ {27, 28}`.
    pub signature: Signature,
    /// 8 bytes: Unix milliseconds (BE u64), matching bee-js `Date.now()`.
    pub timestamp: [u8; 8],
}

/// Client-side stamper that tracks per-bucket utilisation and signs
/// envelopes for individual chunks.
///
/// Construct with [`Stamper::from_blank`] for a fresh batch or with
/// [`Stamper::from_state`] to resume from previously persisted bucket
/// counters.
#[derive(Clone, Debug)]
pub struct Stamper {
    signer: PrivateKey,
    batch_id: BatchId,
    buckets: Vec<u32>,
    depth: u8,
    max_slot: u32,
}

impl Stamper {
    /// New stamper with empty buckets.
    pub fn from_blank(signer: PrivateKey, batch_id: BatchId, depth: u8) -> Result<Self, Error> {
        Self::from_state(signer, batch_id, vec![0u32; NUM_BUCKETS], depth)
    }

    /// Resume a stamper from previously persisted bucket counters.
    /// `buckets.len()` must equal [`NUM_BUCKETS`].
    pub fn from_state(
        signer: PrivateKey,
        batch_id: BatchId,
        buckets: Vec<u32>,
        depth: u8,
    ) -> Result<Self, Error> {
        if depth <= MIN_DEPTH {
            return Err(Error::argument(format!(
                "stamper depth must be > {MIN_DEPTH}, got {depth}"
            )));
        }
        if buckets.len() != NUM_BUCKETS {
            return Err(Error::argument(format!(
                "buckets length must be {NUM_BUCKETS}, got {}",
                buckets.len()
            )));
        }
        let max_slot = 1u32 << (depth - MIN_DEPTH);
        Ok(Self {
            signer,
            batch_id,
            buckets,
            depth,
            max_slot,
        })
    }

    /// Stamp a chunk address. Increments the per-bucket counter and
    /// returns a freshly signed [`Envelope`]. Errors with
    /// [`Error::Argument`] if the bucket is full or the address length
    /// is wrong.
    pub fn stamp(&mut self, chunk_addr: &[u8]) -> Result<Envelope, Error> {
        if chunk_addr.len() != 32 {
            return Err(Error::argument(format!(
                "chunk address must be 32 bytes, got {}",
                chunk_addr.len()
            )));
        }

        let bucket = u16::from_be_bytes([chunk_addr[0], chunk_addr[1]]) as usize;
        let height = self.buckets[bucket];
        if height >= self.max_slot {
            return Err(Error::argument(format!(
                "bucket {bucket} is full (height={height}, max_slot={})",
                self.max_slot
            )));
        }
        self.buckets[bucket] = height + 1;

        let mut index = [0u8; 8];
        index[..4].copy_from_slice(&(bucket as u32).to_be_bytes());
        index[4..].copy_from_slice(&height.to_be_bytes());

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let timestamp = now_ms.to_be_bytes();

        let mut to_sign = Vec::with_capacity(32 + 32 + 8 + 8);
        to_sign.extend_from_slice(chunk_addr);
        to_sign.extend_from_slice(self.batch_id.as_bytes());
        to_sign.extend_from_slice(&index);
        to_sign.extend_from_slice(&timestamp);

        let signature = self.signer.sign(&to_sign)?;
        let issuer = self.signer.public_key()?.address();

        Ok(Envelope {
            batch_id: self.batch_id,
            index,
            issuer,
            signature,
            timestamp,
        })
    }

    /// Snapshot of the current bucket counters. Useful for persisting
    /// and resuming via [`Stamper::from_state`].
    pub fn state(&self) -> &[u32] {
        &self.buckets
    }

    /// Configured depth.
    pub fn depth(&self) -> u8 {
        self.depth
    }

    /// Maximum height per bucket (`2^(depth - 16)`).
    pub fn max_slot(&self) -> u32 {
        self.max_slot
    }

    /// Configured batch ID.
    pub fn batch_id(&self) -> &BatchId {
        &self.batch_id
    }
}

/// Concatenate the components of a postage stamp into the wire format
/// Bee expects when a stamp travels alongside a chunk:
/// `batchID(32) || index(8) || timestamp(8) || signature(65)` — 113
/// bytes total. Mirrors bee-go `postage.MarshalStamp` and bee-js
/// `marshalStamp`.
pub fn marshal_stamp(
    batch_id: &BatchId,
    index: &[u8],
    timestamp: &[u8],
    signature: &Signature,
) -> Result<[u8; MARSHALED_STAMP_LENGTH], Error> {
    if index.len() != 8 {
        return Err(Error::argument(format!(
            "invalid index length: {}",
            index.len()
        )));
    }
    if timestamp.len() != 8 {
        return Err(Error::argument(format!(
            "invalid timestamp length: {}",
            timestamp.len()
        )));
    }
    let mut out = [0u8; MARSHALED_STAMP_LENGTH];
    out[..32].copy_from_slice(batch_id.as_bytes());
    out[32..40].copy_from_slice(index);
    out[40..48].copy_from_slice(timestamp);
    out[48..].copy_from_slice(signature.as_bytes());
    Ok(out)
}

/// Marshal an [`Envelope`] into the 113-byte stamp wire format. Thin
/// wrapper over [`marshal_stamp`] for callers that already hold a
/// structured envelope (typically from [`Stamper::stamp`]). Mirrors
/// bee-go `ConvertEnvelopeToMarshaledStamp` / bee-js
/// `convertEnvelopeToMarshaledStamp`.
pub fn convert_envelope_to_marshaled_stamp(
    env: &Envelope,
) -> Result<[u8; MARSHALED_STAMP_LENGTH], Error> {
    marshal_stamp(&env.batch_id, &env.index, &env.timestamp, &env.signature)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signer() -> PrivateKey {
        PrivateKey::new(&[0x11; 32]).unwrap()
    }

    fn batch() -> BatchId {
        BatchId::new(&[0u8; 32]).unwrap()
    }

    #[test]
    fn stamp_increments_bucket_and_signs() {
        let mut stamper = Stamper::from_blank(signer(), batch(), 20).unwrap();
        let addr = [0u8; 32];
        let env = stamper.stamp(&addr).unwrap();

        assert_eq!(env.batch_id, batch());
        assert_eq!(env.signature.as_bytes().len(), 65);
        assert_eq!(env.index.len(), 8);
        assert_eq!(env.issuer.as_bytes().len(), 20);
        assert_eq!(stamper.state()[0], 1);

        // Signature verifies against the issuer.
        let mut to_sign = Vec::new();
        to_sign.extend_from_slice(&addr);
        to_sign.extend_from_slice(batch().as_bytes());
        to_sign.extend_from_slice(&env.index);
        to_sign.extend_from_slice(&env.timestamp);
        assert!(env.signature.is_valid(&to_sign, env.issuer));

        let env2 = stamper.stamp(&addr).unwrap();
        assert_eq!(stamper.state()[0], 2);
        // Index height bumped from 0 → 1.
        assert_eq!(&env2.index[4..], &1u32.to_be_bytes());
    }

    #[test]
    fn rejects_depth_at_or_below_floor() {
        assert!(Stamper::from_blank(signer(), batch(), 16).is_err());
        assert!(Stamper::from_blank(signer(), batch(), 0).is_err());
        assert!(Stamper::from_blank(signer(), batch(), 17).is_ok());
    }

    #[test]
    fn rejects_bad_chunk_address_length() {
        let mut stamper = Stamper::from_blank(signer(), batch(), 20).unwrap();
        assert!(stamper.stamp(&[0u8; 31]).is_err());
        assert!(stamper.stamp(&[0u8; 33]).is_err());
    }

    #[test]
    fn bucket_full_errors() {
        let mut stamper = Stamper::from_blank(signer(), batch(), 17).unwrap();
        // depth 17 → max_slot = 2^1 = 2. Two stamps fit, third overflows.
        let addr = [0u8; 32];
        stamper.stamp(&addr).unwrap();
        stamper.stamp(&addr).unwrap();
        assert!(stamper.stamp(&addr).is_err());
    }

    #[test]
    fn from_state_round_trips() {
        let mut a = Stamper::from_blank(signer(), batch(), 18).unwrap();
        a.stamp(&[0u8; 32]).unwrap();
        a.stamp(&[0u8; 32]).unwrap();
        let snapshot = a.state().to_vec();
        let b = Stamper::from_state(signer(), batch(), snapshot, 18).unwrap();
        assert_eq!(b.state()[0], 2);
    }

    #[test]
    fn rejects_wrong_state_length() {
        assert!(Stamper::from_state(signer(), batch(), vec![0u32; 10], 18).is_err());
    }

    #[test]
    fn marshal_stamp_round_trip_matches_layout() {
        let batch_id = BatchId::new(&[0xaa; 32]).unwrap();
        let mut stamper = Stamper::from_blank(signer(), batch_id, 17).unwrap();
        let chunk_addr = [0x42u8; 32];
        let env = stamper.stamp(&chunk_addr).unwrap();

        let bytes = convert_envelope_to_marshaled_stamp(&env).unwrap();
        assert_eq!(bytes.len(), MARSHALED_STAMP_LENGTH);
        assert_eq!(&bytes[..32], batch_id.as_bytes());
        assert_eq!(&bytes[32..40], &env.index);
        assert_eq!(&bytes[40..48], &env.timestamp);
        assert_eq!(&bytes[48..], env.signature.as_bytes());

        // Timestamp parses as a sane Unix-ms value (within 24 h of now).
        let mut ts = [0u8; 8];
        ts.copy_from_slice(&bytes[40..48]);
        let ts = u64::from_be_bytes(ts);
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        assert!(ts <= now_ms);
        assert!(now_ms - ts < 24 * 60 * 60 * 1000);
    }

    #[test]
    fn marshal_stamp_rejects_short_index_or_timestamp() {
        let batch_id = BatchId::new(&[0u8; 32]).unwrap();
        let sig = crate::swarm::typed_bytes::Signature::new(&[0xab; 65]).unwrap();
        assert!(marshal_stamp(&batch_id, &[1, 2, 3], &[0u8; 8], &sig).is_err());
        assert!(marshal_stamp(&batch_id, &[0u8; 8], &[1, 2], &sig).is_err());
        assert!(marshal_stamp(&batch_id, &[0u8; 8], &[0u8; 8], &sig).is_ok());
    }

    #[test]
    fn bucket_routing_uses_first_two_bytes_be() {
        let mut stamper = Stamper::from_blank(signer(), batch(), 20).unwrap();
        let mut addr = [0u8; 32];
        addr[0] = 0xab;
        addr[1] = 0xcd;
        stamper.stamp(&addr).unwrap();
        assert_eq!(stamper.state()[0xabcd], 1);
        assert_eq!(stamper.state()[0], 0);
    }
}
