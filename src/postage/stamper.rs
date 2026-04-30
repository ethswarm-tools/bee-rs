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
