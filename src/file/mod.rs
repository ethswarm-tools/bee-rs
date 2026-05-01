//! File / data / chunk / SOC / feed / collection uploads and
//! downloads. Mirrors `pkg/file` in bee-go.
//!
//! Get a [`FileApi`] from [`crate::Client::file`].
//!
//! # Streaming vs. buffered transfers
//!
//! [`FileApi::download_data`] / [`FileApi::download_file`] buffer the
//! full body into [`bytes::Bytes`] before returning — fine for ≤ a few
//! hundred MB but will OOM on multi-GB references. For larger
//! downloads, use [`FileApi::download_data_response`] (or
//! `download_file_response`) which returns the raw [`reqwest::Response`]
//! so you can drive [`reqwest::Response::bytes_stream`] yourself.
//!
//! Uploads accept `impl Into<Bytes>` and stream the body to Bee. The
//! chunk-by-chunk variants ([`FileApi::stream_directory`] and
//! [`FileApi::stream_collection_entries`]) bound peak memory at the
//! BMT chunk size regardless of file size and report progress via a
//! caller-supplied [`OnStreamProgressFn`].
//!
//! # Cancellation
//!
//! Dropping the future returned by any upload / download cancels the
//! in-flight HTTP request. For [`FileApi::stream_directory`] and
//! [`FileApi::stream_collection_entries`], chunks already accepted by
//! the local Bee node remain in the local reserve but the manifest is
//! not finalized — the resulting orphan chunks are eventually pruned
//! but cost reserve space until then.

mod bzz;
mod chunk;
mod data;
mod feeds;
mod soc;
mod stream;

pub use bzz::{
    CollectionEntry, collection_size, hash_collection_entries, hash_directory,
    read_directory_entries,
};
pub use data::ReferenceInformation;
pub use feeds::{
    FeedReader, FeedUpdate, FeedWriter, feed_update_chunk_reference, make_feed_identifier,
};
pub use soc::{SocReader, SocWriter, soc_address};
pub use stream::{OnStreamProgressFn, StreamProgress};

use std::sync::Arc;

use crate::client::Inner;

/// Handle exposing the file/data/chunk endpoints. Cheap to clone
/// (holds an `Arc<Inner>`).
#[derive(Clone, Debug)]
pub struct FileApi {
    pub(crate) inner: Arc<Inner>,
}

impl FileApi {
    pub(crate) fn new(inner: Arc<Inner>) -> Self {
        Self { inner }
    }
}
