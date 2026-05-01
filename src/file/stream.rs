//! Streaming directory upload + recursive Mantaray persistence.
//!
//! Mirrors bee-js `Bee.streamDirectory` (`utils/chunk-stream.ts`) and
//! the recursive `MantarayNode.saveRecursively` flow. Walks a
//! filesystem directory, content-addresses each file in-process via
//! [`FileChunker`], uploads the chunks via `POST /chunks` with
//! bounded concurrency (`N=64`), assembles a Mantaray manifest with
//! one fork per file, and persists the manifest with
//! [`FileApi::save_manifest_recursively`].
//!
//! The optional `on_progress` callback fires once per uploaded chunk
//! (matching bee-js's `onUploadProgress` shape) so callers can render
//! a progress bar without polling.

use std::collections::BTreeMap;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use bytes::Bytes;
use futures_util::stream::{FuturesUnordered, StreamExt};

use crate::api::{CollectionUploadOptions, RedundantUploadOptions, UploadOptions, UploadResult};
use crate::manifest::{MantarayNode, marshal};
use crate::swarm::file_chunker::{FileChunker, SealedChunk};
use crate::swarm::{BatchId, Error, Reference};

use super::FileApi;
use super::bzz::{CollectionEntry, read_directory_entries};

/// Per-chunk upload signal emitted by [`FileApi::stream_directory`].
/// Mirrors bee-js `UploadProgress`.
#[derive(Clone, Copy, Debug)]
pub struct StreamProgress {
    /// Total chunks the upload will produce (computed before upload
    /// starts).
    pub total: usize,
    /// Chunks uploaded so far (post-this-callback).
    pub processed: usize,
}

/// Boxed callback for [`StreamProgress`] events.
pub type OnStreamProgressFn = Arc<dyn Fn(StreamProgress) + Send + Sync>;

/// Default in-flight upload cap. Matches bee-js's `AsyncQueue(64, 64)`.
const STREAM_CONCURRENCY: usize = 64;

type ChunkUploadFuture = Pin<Box<dyn Future<Output = Result<UploadResult, Error>> + Send>>;

impl FileApi {
    /// Persist a [`MantarayNode`] tree recursively, depth-first.
    ///
    /// Mirrors bee-js `MantarayNode.saveRecursively` — each child is
    /// uploaded first (so its `self_address` is populated), then the
    /// node itself is marshaled and uploaded via `/bytes`. The
    /// resulting reference is stored on the node's `self_address` and
    /// returned to the caller.
    pub async fn save_manifest_recursively(
        &self,
        node: &mut MantarayNode,
        batch_id: &BatchId,
        opts: Option<&UploadOptions>,
    ) -> Result<UploadResult, Error> {
        // Async recursion needs explicit boxing.
        Box::pin(save_manifest_recursively_inner(self, node, batch_id, opts)).await
    }

    /// Stream a directory upload chunk-by-chunk.
    ///
    /// For each regular file under `dir`, content-addresses it via
    /// [`FileChunker`], uploads the resulting chunks via `POST /chunks`
    /// with up to `N=64` concurrent in-flight uploads, then assembles a
    /// Mantaray manifest with one fork per file (path →
    /// content-addressed root). Finally calls
    /// [`FileApi::save_manifest_recursively`] to persist the manifest
    /// and returns its reference.
    ///
    /// Mirrors bee-js `Bee.streamDirectory`. The `on_progress` callback
    /// fires once per uploaded chunk with `(processed, total)` counts.
    ///
    /// Differences from bee-js:
    /// - File contents are read fully into memory before being fed to
    ///   the chunker. True file-streaming (read → seal → upload as a
    ///   pipeline) can be added later if a real use case lands.
    /// - Per-file metadata (Content-Type / Filename) is not yet set on
    ///   the Mantaray fork — bee-js sets `Content-Type` from the file
    ///   extension, but bee-rs leaves manifests metadata-free for now;
    ///   see [`Self::upload_collection`] for the tar-based path that
    ///   lets Bee infer types server-side.
    pub async fn stream_directory(
        &self,
        batch_id: &BatchId,
        dir: impl AsRef<Path>,
        opts: Option<&CollectionUploadOptions>,
        on_progress: Option<OnStreamProgressFn>,
    ) -> Result<UploadResult, Error> {
        let entries = read_directory_entries(dir.as_ref())?;
        self.stream_collection_entries(batch_id, &entries, opts, on_progress)
            .await
    }

    /// Same as [`Self::stream_directory`] but takes pre-built
    /// in-memory entries instead of walking the filesystem.
    pub async fn stream_collection_entries(
        &self,
        batch_id: &BatchId,
        entries: &[CollectionEntry],
        opts: Option<&CollectionUploadOptions>,
        on_progress: Option<OnStreamProgressFn>,
    ) -> Result<UploadResult, Error> {
        // Pre-count chunks across every file so the progress callback
        // can render a stable total. The chunker fan-out is fixed
        // (CHUNK_SIZE leaves, MAX_BRANCHES = 128 per intermediate
        // level), so the count is a pure function of file size.
        let total_chunks: usize = entries.iter().map(|e| total_chunks_for(e.data.len())).sum();
        let processed = Arc::new(AtomicUsize::new(0));

        // Upload-options inherit from the collection's base options.
        let upload_opts: Option<UploadOptions> = opts.map(|o| o.base.clone());

        let mut manifest = MantarayNode::new();
        let mut has_index_html = false;

        for entry in entries {
            // The chunker callback is `FnMut + Send + 'static`, so it
            // can't borrow a stack-local Vec. Share an `Arc<Mutex<…>>`
            // instead — locking is uncontended (callback fires
            // synchronously from this thread) so the cost is just a
            // pair of atomic ops per chunk.
            let chunks_buf: Arc<Mutex<Vec<SealedChunk>>> = Arc::new(Mutex::new(Vec::new()));
            {
                let chunks_buf = Arc::clone(&chunks_buf);
                let mut chunker = FileChunker::with_callback(move |sealed| {
                    chunks_buf
                        .lock()
                        .map_err(|_| Error::argument("chunker buffer poisoned"))?
                        .push(sealed);
                    Ok(())
                });
                chunker.write(&entry.data)?;
                let _root = chunker.finalize()?;
            }
            let chunks = Arc::try_unwrap(chunks_buf)
                .map_err(|_| Error::argument("chunker buffer still shared"))?
                .into_inner()
                .map_err(|_| Error::argument("chunker buffer poisoned"))?;

            // Compute file root from chunker output (last chunk in the
            // collapsed level stack). FileChunker.finalize already
            // returns it, so capture it before draining.
            let root: Reference = chunks
                .last()
                .map(|c| c.address.clone())
                .ok_or_else(|| Error::argument(format!("empty file: {}", entry.path)))?;

            // Stream the chunks: bounded N concurrent uploads.
            let mut inflight: FuturesUnordered<ChunkUploadFuture> = FuturesUnordered::new();

            for sealed in chunks {
                while inflight.len() >= STREAM_CONCURRENCY {
                    if let Some(res) = inflight.next().await {
                        res?;
                        bump_progress(&processed, total_chunks, on_progress.as_ref());
                    }
                }
                let api = self.clone();
                let batch = *batch_id;
                let opts_clone = upload_opts.clone();
                let body: Bytes = Bytes::from(sealed.data());
                let fut: ChunkUploadFuture =
                    Box::pin(
                        async move { api.upload_chunk(&batch, body, opts_clone.as_ref()).await },
                    );
                inflight.push(fut);
            }
            while let Some(res) = inflight.next().await {
                res?;
                bump_progress(&processed, total_chunks, on_progress.as_ref());
            }

            // Add a fork to the manifest pointing at the file's root
            // chunk. Per bee-js, `Content-Type` / `Filename` would
            // also be set here; we leave the metadata field empty for
            // parity with `upload_collection_entries` until the live
            // soak shows it matters.
            manifest.add_fork(entry.path.as_bytes(), Some(&root), None);
            if entry.path == "index.html" {
                has_index_html = true;
            }
        }

        // Apply index_document / error_document metadata on the root
        // fork. Mirrors bee-js's `mantaray.addFork('/', NULL_ADDRESS, …)`
        // path.
        if has_index_html
            || opts.and_then(|o| o.index_document.as_deref()).is_some()
            || opts.and_then(|o| o.error_document.as_deref()).is_some()
        {
            let mut metadata: BTreeMap<String, String> = BTreeMap::new();
            if let Some(idx) = opts.and_then(|o| o.index_document.as_deref()) {
                metadata.insert("website-index-document".to_string(), idx.to_string());
            } else if has_index_html {
                metadata.insert(
                    "website-index-document".to_string(),
                    "index.html".to_string(),
                );
            }
            if let Some(err) = opts.and_then(|o| o.error_document.as_deref()) {
                metadata.insert("website-error-document".to_string(), err.to_string());
            }
            // The root metadata fork uses `/` as the path and a null
            // target — Bee treats this as the manifest root's
            // metadata.
            manifest.add_fork(b"/", None, Some(&metadata));
        }

        self.save_manifest_recursively(&mut manifest, batch_id, upload_opts.as_ref())
            .await
    }
}

fn bump_progress(processed: &Arc<AtomicUsize>, total: usize, cb: Option<&OnStreamProgressFn>) {
    let n = processed.fetch_add(1, Ordering::SeqCst) + 1;
    if let Some(cb) = cb {
        cb(StreamProgress {
            total,
            processed: n,
        });
    }
}

/// Total number of chunks (leaves + intermediates) a file of `len`
/// bytes will produce in the Swarm BMT chunker. Matches bee-js
/// `totalChunks(size)` from `utils/chunk-size.ts`.
fn total_chunks_for(len: usize) -> usize {
    use crate::swarm::bmt::CHUNK_SIZE;
    use crate::swarm::file_chunker::MAX_BRANCHES;

    if len == 0 {
        return 0;
    }
    let mut leaves = len.div_ceil(CHUNK_SIZE);
    let mut total = leaves;
    while leaves > 1 {
        leaves = leaves.div_ceil(MAX_BRANCHES);
        total += leaves;
    }
    total
}

async fn save_manifest_recursively_inner(
    api: &FileApi,
    node: &mut MantarayNode,
    batch_id: &BatchId,
    opts: Option<&UploadOptions>,
) -> Result<UploadResult, Error> {
    // Save children first so each fork's self_address is populated
    // before we marshal this node.
    for fork in node.forks.values_mut() {
        let result = Box::pin(save_manifest_recursively_inner(
            api,
            &mut fork.node,
            batch_id,
            opts,
        ))
        .await?;
        fork.node.self_address = Some(reference_to_self_address(&result.reference)?);
    }
    let bytes = marshal(node)?;
    // upload_data takes RedundantUploadOptions (UploadOptions + a
    // redundancy level). Wrap the caller's UploadOptions so we get
    // the same pin/encrypt/act/tag/deferred behavior.
    let redundant = opts.map(|o| RedundantUploadOptions {
        base: o.clone(),
        redundancy_level: None,
    });
    let result = api.upload_data(batch_id, bytes, redundant.as_ref()).await?;
    node.self_address = Some(reference_to_self_address(&result.reference)?);
    Ok(result)
}

/// Truncate a reference (32 or 64 bytes) to the 32-byte chunk address
/// that goes into [`MantarayNode::self_address`]. For an encrypted
/// 64-byte reference, the first 32 bytes are the CAC address; the
/// trailing 32 bytes are the encryption key, which Mantaray stores
/// elsewhere.
fn reference_to_self_address(reference: &Reference) -> Result<[u8; 32], Error> {
    reference
        .as_bytes()
        .first_chunk::<32>()
        .copied()
        .ok_or_else(|| Error::argument("manifest child reference < 32 bytes"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_chunks_for_matches_known_sizes() {
        // Empty → 0.
        assert_eq!(total_chunks_for(0), 0);
        // Single leaf, no intermediate.
        assert_eq!(total_chunks_for(1), 1);
        assert_eq!(total_chunks_for(4096), 1);
        // 4097 bytes → 2 leaves + 1 intermediate.
        assert_eq!(total_chunks_for(4097), 3);
        // Exactly 128 leaves (one full intermediate level).
        let n = 4096 * 128;
        assert_eq!(total_chunks_for(n), 128 + 1);
        // 129 leaves → 2 intermediates + 1 root.
        let n = 4096 * 129;
        assert_eq!(total_chunks_for(n), 129 + 2 + 1);
    }
}
