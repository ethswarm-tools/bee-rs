//! Feed reader / writer + auto-index update logic. Mirrors bee-go's
//! `pkg/file/feed.go`.
//!
//! A Swarm feed stores a sequence of SOC chunks under a fixed
//! `(owner, topic)` pair. Each update has a 64-bit big-endian index
//! and is signed by the owner. The chunk identifier is
//! `keccak256(topic || BE-uint64(index))`; the chunk payload is
//! `BE-uint64(timestamp) || data`.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use reqwest::Method;
use serde::Deserialize;

use crate::api::{DownloadOptions, UploadResult};
use crate::client::{Inner, request};
use crate::swarm::{
    BatchId, Error, EthAddress, Identifier, PrivateKey, Reference, Topic, bmt::keccak256,
    make_single_owner_chunk,
};

use super::FileApi;

/// Result of a feed lookup: the wrapped chunk payload plus the
/// indexes parsed from `swarm-feed-index` / `swarm-feed-index-next`.
///
/// Mirrors bee-js `FeedPayloadResult`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedUpdate {
    /// Raw chunk payload (`timestamp(8) || data`).
    pub payload: Bytes,
    /// Index of the returned update.
    pub index: u64,
    /// Index where the *next* update should be written (sequential
    /// feeds: `index + 1`).
    pub index_next: u64,
}

#[derive(Deserialize)]
struct ReferenceBody {
    reference: String,
}

impl FileApi {
    /// `POST /feeds/{owner}/{topic}` — create a feed manifest for
    /// the given pair. Returns the manifest reference.
    pub async fn create_feed_manifest(
        &self,
        batch_id: &BatchId,
        owner: &EthAddress,
        topic: &Topic,
    ) -> Result<Reference, Error> {
        let path = format!("feeds/{}/{}", owner.to_hex(), topic.to_hex());
        let builder = request(&self.inner, Method::POST, &path)?
            .header("Swarm-Postage-Batch-Id", batch_id.to_hex());
        let body: ReferenceBody = self.inner.send_json(builder).await?;
        Reference::from_hex(&body.reference)
    }

    /// `GET /feeds/{owner}/{topic}` — return the latest feed lookup.
    pub async fn get_feed_lookup(
        &self,
        owner: &EthAddress,
        topic: &Topic,
    ) -> Result<Reference, Error> {
        let path = format!("feeds/{}/{}", owner.to_hex(), topic.to_hex());
        let builder = request(&self.inner, Method::GET, &path)?;
        let body: ReferenceBody = self.inner.send_json(builder).await?;
        Reference::from_hex(&body.reference)
    }

    /// Fetch the most recent feed update.
    ///
    /// The body is the wrapped chunk payload; the `swarm-feed-index`
    /// and `swarm-feed-index-next` headers carry the indexes as
    /// 8-byte big-endian hex.
    pub async fn fetch_latest_feed_update(
        &self,
        owner: &EthAddress,
        topic: &Topic,
    ) -> Result<FeedUpdate, Error> {
        let path = format!("feeds/{}/{}", owner.to_hex(), topic.to_hex());
        let builder = request(&self.inner, Method::GET, &path)?;
        let resp = self.inner.send(builder).await?;
        let index = decode_feed_index_header(&resp, "swarm-feed-index")?;
        let index_next = decode_feed_index_header(&resp, "swarm-feed-index-next")?;
        let payload = resp.bytes().await?;
        Ok(FeedUpdate {
            payload,
            index,
            index_next,
        })
    }

    /// Return the index where the next feed update should be written.
    /// Bee returns 404 / 500 when the feed is empty; this helper
    /// translates those to `0`.
    pub async fn find_next_index(&self, owner: &EthAddress, topic: &Topic) -> Result<u64, Error> {
        match self.fetch_latest_feed_update(owner, topic).await {
            Ok(upd) => Ok(upd.index_next),
            Err(e) => match e.status() {
                Some(404) | Some(500) => Ok(0),
                _ => Err(e),
            },
        }
    }

    /// Update the feed at the next available index. The chunk payload
    /// is `BE-uint64(timestamp) || data`. Mirrors bee-js
    /// `updateFeedWithPayload`.
    pub async fn update_feed(
        &self,
        batch_id: &BatchId,
        signer: &PrivateKey,
        topic: &Topic,
        data: &[u8],
    ) -> Result<UploadResult, Error> {
        let owner = signer.public_key()?.address();
        let index = self.find_next_index(&owner, topic).await?;
        self.update_feed_with_index(batch_id, signer, topic, index, data)
            .await
    }

    /// Update the feed to point at `reference`. The chunk payload is
    /// `BE-uint64(timestamp) || reference(32 or 64)`. If `index` is
    /// `None`, [`FileApi::find_next_index`] is called.
    pub async fn update_feed_with_reference(
        &self,
        batch_id: &BatchId,
        signer: &PrivateKey,
        topic: &Topic,
        reference: &Reference,
        index: Option<u64>,
    ) -> Result<UploadResult, Error> {
        let owner = signer.public_key()?.address();
        let idx = match index {
            Some(i) => i,
            None => self.find_next_index(&owner, topic).await?,
        };
        self.update_feed_with_index(batch_id, signer, topic, idx, reference.as_bytes())
            .await
    }

    /// Update the feed at a specific index.
    ///
    /// The chunk identifier is `keccak256(topic || BE-uint64(index))`;
    /// the payload is `BE-uint64(now_unix_seconds) || data`. The
    /// chunk is signed via SOC and uploaded to `/soc/{owner}/{id}`.
    pub async fn update_feed_with_index(
        &self,
        batch_id: &BatchId,
        signer: &PrivateKey,
        topic: &Topic,
        index: u64,
        data: &[u8],
    ) -> Result<UploadResult, Error> {
        let identifier = make_feed_identifier(topic, index);

        let mut payload = Vec::with_capacity(8 + data.len());
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        payload.extend_from_slice(&timestamp.to_be_bytes());
        payload.extend_from_slice(data);

        let soc = make_single_owner_chunk(&identifier, &payload, signer)?;

        // SOC body: span || payload (the live-Bee SOC bug bee-go hit).
        let mut full = Vec::with_capacity(soc.span.as_bytes().len() + soc.payload.len());
        full.extend_from_slice(soc.span.as_bytes());
        full.extend_from_slice(&soc.payload);

        let owner = signer.public_key()?.address();
        self.upload_soc(batch_id, &owner, &identifier, &soc.signature, full, None)
            .await
    }

    /// True iff the feed currently resolves on the network. If
    /// `index` is `None`, only the latest update is checked. If
    /// `index` is `Some(i)`, every chunk from 0 through `i` is
    /// checked via [`FileApi::are_all_sequential_feeds_update_retrievable`].
    pub async fn is_feed_retrievable(
        &self,
        owner: &EthAddress,
        topic: &Topic,
        index: Option<u64>,
        opts: Option<&DownloadOptions>,
    ) -> Result<bool, Error> {
        if let Some(i) = index {
            return self
                .are_all_sequential_feeds_update_retrievable(owner, topic, i, opts)
                .await;
        }
        match self.fetch_latest_feed_update(owner, topic).await {
            Ok(_) => Ok(true),
            Err(e) => match e.status() {
                Some(404) | Some(500) => Ok(false),
                _ => Err(e),
            },
        }
    }

    /// True iff every feed-update chunk from `0` through `index`
    /// (inclusive) is currently retrievable. Used to validate that a
    /// feed can be replayed from its origin.
    pub async fn are_all_sequential_feeds_update_retrievable(
        &self,
        owner: &EthAddress,
        topic: &Topic,
        index: u64,
        opts: Option<&DownloadOptions>,
    ) -> Result<bool, Error> {
        for i in 0..=index {
            let r = feed_update_chunk_reference(owner, topic, i)?;
            match self.download_chunk(&r, opts).await {
                Ok(_) => continue,
                Err(e) => match e.status() {
                    Some(404) | Some(500) => return Ok(false),
                    _ => return Err(e),
                },
            }
        }
        Ok(true)
    }

    /// Construct a [`FeedReader`] bound to `(owner, topic)`. Mirrors
    /// bee-js `Bee.makeFeedReader`.
    pub fn make_feed_reader(&self, owner: EthAddress, topic: Topic) -> FeedReader {
        FeedReader {
            inner: self.inner.clone(),
            owner,
            topic,
        }
    }

    /// Construct a [`FeedWriter`] bound to `(signer, topic)`. Owner is
    /// derived from `signer.public_key().address()`. Mirrors bee-js
    /// `Bee.makeFeedWriter`.
    pub fn make_feed_writer(&self, signer: PrivateKey, topic: Topic) -> Result<FeedWriter, Error> {
        let owner = signer.public_key()?.address();
        Ok(FeedWriter {
            reader: FeedReader {
                inner: self.inner.clone(),
                owner,
                topic,
            },
            signer,
        })
    }
}

/// Reader bound to a `(owner, topic)` pair. Wraps the lower-level
/// `FileApi` feed methods for callers that prefer the bee-js
/// `FeedReader` shape.
#[derive(Clone, Debug)]
pub struct FeedReader {
    inner: Arc<Inner>,
    owner: EthAddress,
    topic: Topic,
}

impl FeedReader {
    /// Owner address.
    pub fn owner(&self) -> &EthAddress {
        &self.owner
    }

    /// Feed topic.
    pub fn topic(&self) -> &Topic {
        &self.topic
    }

    fn api(&self) -> FileApi {
        FileApi {
            inner: self.inner.clone(),
        }
    }

    /// Fetch the most recent feed update.
    pub async fn download(&self) -> Result<FeedUpdate, Error> {
        self.api()
            .fetch_latest_feed_update(&self.owner, &self.topic)
            .await
    }

    /// Resolve the feed manifest reference (`GET /feeds/{owner}/{topic}`).
    pub async fn lookup(&self) -> Result<Reference, Error> {
        self.api().get_feed_lookup(&self.owner, &self.topic).await
    }

    /// Index where the next feed update will be written.
    pub async fn next_index(&self) -> Result<u64, Error> {
        self.api().find_next_index(&self.owner, &self.topic).await
    }

    /// True iff the feed currently resolves on the network.
    pub async fn is_retrievable(
        &self,
        index: Option<u64>,
        opts: Option<&DownloadOptions>,
    ) -> Result<bool, Error> {
        self.api()
            .is_feed_retrievable(&self.owner, &self.topic, index, opts)
            .await
    }
}

/// Writer bound to a `(signer, topic)` pair. Owner is derived from
/// the signer.
#[derive(Clone, Debug)]
pub struct FeedWriter {
    reader: FeedReader,
    signer: PrivateKey,
}

impl FeedWriter {
    /// Owner address derived from the signer.
    pub fn owner(&self) -> &EthAddress {
        self.reader.owner()
    }

    /// Feed topic.
    pub fn topic(&self) -> &Topic {
        self.reader.topic()
    }

    /// Read side of this writer.
    pub fn reader(&self) -> &FeedReader {
        &self.reader
    }

    fn api(&self) -> FileApi {
        FileApi {
            inner: self.reader.inner.clone(),
        }
    }

    /// Update the feed at the next available index. Mirrors bee-js
    /// `FeedWriter.uploadPayload`.
    pub async fn upload_payload(
        &self,
        batch_id: &BatchId,
        data: &[u8],
    ) -> Result<UploadResult, Error> {
        self.api()
            .update_feed(batch_id, &self.signer, self.reader.topic(), data)
            .await
    }

    /// Update the feed to point at `reference`. Mirrors bee-js
    /// `FeedWriter.uploadReference`.
    pub async fn upload_reference(
        &self,
        batch_id: &BatchId,
        reference: &Reference,
        index: Option<u64>,
    ) -> Result<UploadResult, Error> {
        self.api()
            .update_feed_with_reference(
                batch_id,
                &self.signer,
                self.reader.topic(),
                reference,
                index,
            )
            .await
    }

    /// Update the feed at a specific index.
    pub async fn upload_payload_at(
        &self,
        batch_id: &BatchId,
        index: u64,
        data: &[u8],
    ) -> Result<UploadResult, Error> {
        self.api()
            .update_feed_with_index(batch_id, &self.signer, self.reader.topic(), index, data)
            .await
    }
}

/// Compute the feed identifier for a `(topic, index)` pair —
/// `keccak256(topic || BE-uint64(index))`.
pub fn make_feed_identifier(topic: &Topic, index: u64) -> Identifier {
    let mut input = Vec::with_capacity(topic.as_bytes().len() + 8);
    input.extend_from_slice(topic.as_bytes());
    input.extend_from_slice(&index.to_be_bytes());
    Identifier::new(&keccak256(&input)).expect("keccak256 returns 32 bytes")
}

/// Compute the SOC chunk address for a feed update at
/// `(owner, topic, index)` — `keccak256(identifier || owner)`. Use
/// this with [`FileApi::download_chunk`] to verify retrievability of
/// past updates.
pub fn feed_update_chunk_reference(
    owner: &EthAddress,
    topic: &Topic,
    index: u64,
) -> Result<Reference, Error> {
    let identifier = make_feed_identifier(topic, index);
    let mut input = Vec::with_capacity(identifier.as_bytes().len() + owner.as_bytes().len());
    input.extend_from_slice(identifier.as_bytes());
    input.extend_from_slice(owner.as_bytes());
    Reference::new(&keccak256(&input))
}

fn decode_feed_index_header(resp: &reqwest::Response, name: &str) -> Result<u64, Error> {
    let s = resp
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::argument(format!("missing {name} header")))?;
    let bytes = hex::decode(s)?;
    if bytes.len() != 8 {
        return Err(Error::argument(format!(
            "{name}: expected 8 bytes, got {}",
            bytes.len()
        )));
    }
    let mut arr = [0u8; 8];
    arr.copy_from_slice(&bytes);
    Ok(u64::from_be_bytes(arr))
}
