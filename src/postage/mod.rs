//! Postage batch CRUD + stamp metadata. Mirrors `pkg/postage` in
//! bee-go.
//!
//! The endpoint methods live on [`PostageApi`] (get one from
//! [`crate::Client::postage`]). Pure stamp math — `get_stamp_cost`,
//! `get_stamp_effective_bytes`, etc. — is exposed as free functions
//! since it has no I/O.

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
