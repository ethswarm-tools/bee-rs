//! Postage batch data types. Mirrors bee-go's `pkg/postage/postage.go`.

use num_bigint::BigInt;
use serde::{Deserialize, Deserializer};

use crate::swarm::BatchId;

/// Postage batch as returned by `GET /stamps` / `GET /stamps/{id}`
/// (owner-only view; includes utilization, label, blockNumber).
///
/// **Wire-format gotcha** caught against a live Bee: the per-chunk
/// price is keyed `"amount"` not `"value"`, and the immutable flag
/// is keyed `"immutableFlag"` not `"immutable"`. bee-go landed both
/// fixes in v1.0; we get them right by construction here.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostageBatch {
    /// Batch identifier.
    #[serde(rename = "batchID")]
    pub batch_id: BatchId,
    /// Per-chunk amount (PLUR).
    #[serde(default, deserialize_with = "deserialize_optional_bigint_string")]
    pub amount: Option<BigInt>,
    /// Block number when the batch was bought.
    #[serde(default)]
    pub start: u64,
    /// Owner address (hex).
    #[serde(default)]
    pub owner: String,
    /// Batch depth.
    pub depth: u8,
    /// Bucket depth.
    pub bucket_depth: u8,
    /// Whether the batch is immutable.
    #[serde(default, rename = "immutableFlag")]
    pub immutable: bool,
    /// Estimated TTL in seconds.
    #[serde(default, rename = "batchTTL")]
    pub batch_ttl: i64,
    /// Bucket utilization stats.
    #[serde(default)]
    pub utilization: u32,
    /// Whether the batch is currently usable.
    #[serde(default)]
    pub usable: bool,
    /// Whether Bee currently knows about the batch.
    #[serde(default)]
    pub exists: bool,
    /// Optional label.
    #[serde(default)]
    pub label: String,
    /// Block number reported by Bee.
    #[serde(default)]
    pub block_number: u64,
}

/// Postage batch as returned by `GET /batches` (chain-wide view; drops
/// owner-only fields).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalPostageBatch {
    /// Batch identifier.
    #[serde(rename = "batchID")]
    pub batch_id: BatchId,
    /// Per-chunk value (PLUR). The chain-wide endpoint uses `"value"`
    /// here, unlike the owner endpoint's `"amount"`.
    #[serde(default, deserialize_with = "deserialize_optional_bigint_string")]
    pub value: Option<BigInt>,
    /// Block number when the batch was bought.
    #[serde(default)]
    pub start: u64,
    /// Owner address (hex).
    #[serde(default)]
    pub owner: String,
    /// Batch depth.
    pub depth: u8,
    /// Bucket depth.
    pub bucket_depth: u8,
    /// Whether the batch is immutable. (Note: the chain-wide endpoint
    /// uses `"immutable"` whereas the owner endpoint uses
    /// `"immutableFlag"`.)
    #[serde(default)]
    pub immutable: bool,
    /// Estimated TTL in seconds.
    #[serde(default, rename = "batchTTL")]
    pub batch_ttl: i64,
}

/// Per-bucket collision count from `GET /stamps/{id}/buckets`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchBucket {
    /// Bucket index.
    #[serde(rename = "bucketID")]
    pub bucket_id: u32,
    /// Number of chunks in this bucket.
    pub collisions: u32,
}

/// Aggregated bucket fill stats for one batch.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostageBatchBuckets {
    /// Batch depth.
    pub depth: u8,
    /// Bucket depth.
    pub bucket_depth: u8,
    /// Per-bucket cap.
    pub bucket_upper_bound: u32,
    /// Per-bucket collision counts.
    pub buckets: Vec<BatchBucket>,
}

fn deserialize_optional_bigint_string<'de, D>(d: D) -> Result<Option<BigInt>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(d)?;
    match opt {
        None => Ok(None),
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => s
            .parse::<BigInt>()
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}
