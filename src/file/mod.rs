//! File / data / chunk / SOC / feed / collection uploads and
//! downloads. Mirrors `pkg/file` in bee-go.
//!
//! Get a [`FileApi`] from [`crate::Client::file`].

mod bzz;
mod chunk;
mod data;
mod feeds;
mod soc;

pub use bzz::{
    CollectionEntry, collection_size, hash_collection_entries, hash_directory,
    read_directory_entries,
};
pub use data::ReferenceInformation;
pub use feeds::{
    FeedReader, FeedUpdate, FeedWriter, feed_update_chunk_reference, make_feed_identifier,
};
pub use soc::{SocReader, SocWriter, soc_address};

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
