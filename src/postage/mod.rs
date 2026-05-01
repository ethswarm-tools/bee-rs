//! Postage batch CRUD + stamp metadata. Mirrors `pkg/postage` in
//! bee-go.
//!
//! The endpoint methods live on [`PostageApi`] (get one from
//! [`crate::Client::postage`]). Pure stamp math — `get_stamp_cost`,
//! `get_stamp_effective_bytes`, etc. — is exposed as free functions
//! since it has no I/O.
//!
//! # Batch-usability delay
//!
//! A batch is not usable for uploads immediately after
//! [`PostageApi::create_postage_batch`] returns. Bee waits for N
//! confirmations of the purchase transaction (typically 8) before
//! flipping [`PostageBatch::usable`] to `true`. On Gnosis (5-second
//! blocks) this is ~40 s; on Sepolia (12-second blocks) it can be 2-3
//! minutes and occasionally longer.
//!
//! Poll [`PostageApi::get_postage_batch`] in a loop until `usable` is
//! true, or supply the batch via an environment variable so tests can
//! reuse a known-good batch. Uploading against a not-yet-usable batch
//! returns HTTP 422 "stamp not usable", surfaced as
//! [`crate::Error::Response`] with `status = 422`.
//!
//! # Dilute is one-way
//!
//! [`PostageApi::dilute_batch`] only allows depth to grow. Once a
//! batch's depth is increased its previously-issued stamps remain
//! valid; there is no way to shrink depth and reclaim funds.

mod endpoints;
mod stamp_math;
mod stamper;
mod types;

use std::sync::Arc;

use crate::client::Inner;

pub use stamp_math::{
    EFFECTIVE_SIZE_BREAKPOINTS, get_depth_for_size, get_stamp_cost, get_stamp_effective_bytes,
    get_stamp_theoretical_bytes, get_stamp_usage,
};
pub use stamper::{
    Envelope, MARSHALED_STAMP_LENGTH, MIN_DEPTH, NUM_BUCKETS, Stamper,
    convert_envelope_to_marshaled_stamp, marshal_stamp,
};
pub use types::{BatchBucket, GlobalPostageBatch, PostageBatch, PostageBatchBuckets};

/// Handle exposing the postage endpoints. Cheap to clone.
#[derive(Clone, Debug)]
pub struct PostageApi {
    pub(crate) inner: Arc<Inner>,
}

impl PostageApi {
    pub(crate) fn new(inner: Arc<Inner>) -> Self {
        Self { inner }
    }
}
