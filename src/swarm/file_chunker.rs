//! Streaming Swarm content-addressed chunker. Mirrors bee-go's
//! `pkg/swarm/file_chunker.go` and the bee-js `MerkleTree` chunker
//! from `cafe-utility`.
//!
//! [`FileChunker`] turns an arbitrary byte stream into a tree of
//! 4-KiB content-addressed chunks (CACs). Leaves carry the raw
//! payload; intermediate chunks carry a flat list of child addresses
//! (32 bytes each). The fan-out at every level is
//! [`MAX_BRANCHES`] = `4096 / 32` = 128. Callers stream input via
//! [`FileChunker::write`] and finish with [`FileChunker::finalize`],
//! which returns the root chunk address.

use crate::swarm::bmt::{CHUNK_SIZE, SEGMENT_SIZE, calculate_chunk_address};
use crate::swarm::errors::Error;
use crate::swarm::typed_bytes::{Reference, SPAN_LENGTH, Span};

/// Fan-out at every intermediate level (`4096 / 32 = 128`).
pub const MAX_BRANCHES: usize = CHUNK_SIZE / SEGMENT_SIZE;

#[derive(Clone, Debug)]
struct LevelRef {
    addr: [u8; 32],
    span: u64,
}

/// Result of finalizing a chunker: the root address plus the total
/// number of bytes covered by the tree (the root span).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkerRoot {
    /// Root address of the chunk tree.
    pub address: Reference,
    /// Span of the root chunk: total bytes covered by the tree.
    pub span: Span,
}

/// Type alias for the on-chunk callback.
type OnChunkCallback = Box<dyn FnMut(SealedChunk) -> Result<(), Error> + Send>;

/// Streaming content-addressed chunker.
///
/// Construct one via [`FileChunker::new`], push bytes with
/// [`FileChunker::write`], and call [`FileChunker::finalize`] to get
/// the root address. Optionally pass an `on_chunk` callback to
/// [`FileChunker::with_callback`] to be notified as each chunk is
/// sealed (useful for streaming uploads).
pub struct FileChunker {
    on_chunk: Option<OnChunkCallback>,
    leaf_buf: Vec<u8>,
    levels: Vec<Vec<LevelRef>>,
}

/// Snapshot of a sealed chunk, passed to the `on_chunk` callback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SealedChunk {
    /// Chunk address.
    pub address: Reference,
    /// Chunk span (8-byte LE u64).
    pub span: Span,
    /// Chunk payload (≤ `CHUNK_SIZE` bytes).
    pub payload: Vec<u8>,
}

impl SealedChunk {
    /// Wire form: `span (8) || payload`.
    pub fn data(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(SPAN_LENGTH + self.payload.len());
        out.extend_from_slice(self.span.as_bytes());
        out.extend_from_slice(&self.payload);
        out
    }
}

impl Default for FileChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl FileChunker {
    /// New chunker without an on-chunk callback. Use this for offline
    /// hashing.
    pub fn new() -> Self {
        Self {
            on_chunk: None,
            leaf_buf: Vec::new(),
            levels: Vec::new(),
        }
    }

    /// New chunker with an on-chunk callback. The callback runs
    /// synchronously as each leaf or intermediate chunk is sealed.
    pub fn with_callback<F>(callback: F) -> Self
    where
        F: FnMut(SealedChunk) -> Result<(), Error> + Send + 'static,
    {
        Self {
            on_chunk: Some(Box::new(callback)),
            leaf_buf: Vec::new(),
            levels: Vec::new(),
        }
    }

    /// Append bytes to the input stream. As each leaf reaches
    /// [`CHUNK_SIZE`] it is sealed and propagated up the level stack.
    pub fn write(&mut self, data: &[u8]) -> Result<usize, Error> {
        let mut remaining = data;
        let mut written = 0;
        while !remaining.is_empty() {
            let room = CHUNK_SIZE - self.leaf_buf.len();
            let take = remaining.len().min(room);
            self.leaf_buf.extend_from_slice(&remaining[..take]);
            remaining = &remaining[take..];
            written += take;
            if self.leaf_buf.len() == CHUNK_SIZE {
                self.flush_leaf()?;
            }
        }
        Ok(written)
    }

    /// Seal any trailing partial leaf, collapse the level stack, and
    /// return the root chunk's address + span. An empty chunker
    /// (zero bytes written) is rejected as it has no valid root.
    pub fn finalize(mut self) -> Result<ChunkerRoot, Error> {
        if self.levels.is_empty() && self.leaf_buf.is_empty() {
            return Err(Error::argument("FileChunker: no input"));
        }
        if !self.leaf_buf.is_empty() {
            self.flush_leaf()?;
        }

        // Collapse the level stack from the bottom up. Skip levels
        // that are already empty; for the highest level holding a
        // single ref, that ref is the root.
        let mut level = 0;
        while level < self.levels.len() {
            if level == self.levels.len() - 1 && self.levels[level].len() == 1 {
                break;
            }
            if !self.levels[level].is_empty() {
                self.collapse_level(level)?;
            }
            level += 1;
        }

        let root_level = self.levels.len() - 1;
        let root = self.levels[root_level][0].clone();

        Ok(ChunkerRoot {
            address: Reference::new(&root.addr)?,
            span: Span::from_u64(root.span),
        })
    }

    fn flush_leaf(&mut self) -> Result<(), Error> {
        let payload = std::mem::take(&mut self.leaf_buf);
        if payload.is_empty() {
            return Ok(());
        }
        let span = Span::from_u64(payload.len() as u64);

        let mut full = Vec::with_capacity(SPAN_LENGTH + payload.len());
        full.extend_from_slice(span.as_bytes());
        full.extend_from_slice(&payload);
        let addr = calculate_chunk_address(&full)?;

        if let Some(cb) = self.on_chunk.as_mut() {
            cb(SealedChunk {
                address: Reference::new(&addr)?,
                span,
                payload: payload.clone(),
            })?;
        }

        if self.levels.is_empty() {
            self.levels.push(Vec::new());
        }
        self.levels[0].push(LevelRef {
            addr,
            span: payload.len() as u64,
        });
        if self.levels[0].len() == MAX_BRANCHES {
            self.collapse_level(0)?;
        }
        Ok(())
    }

    fn collapse_level(&mut self, level: usize) -> Result<(), Error> {
        let refs = std::mem::take(&mut self.levels[level]);
        if refs.is_empty() {
            return Ok(());
        }

        let mut payload = Vec::with_capacity(refs.len() * SEGMENT_SIZE);
        let mut total_span = 0u64;
        for r in &refs {
            payload.extend_from_slice(&r.addr);
            total_span += r.span;
        }
        let span = Span::from_u64(total_span);

        let mut full = Vec::with_capacity(SPAN_LENGTH + payload.len());
        full.extend_from_slice(span.as_bytes());
        full.extend_from_slice(&payload);
        let addr = calculate_chunk_address(&full)?;

        if let Some(cb) = self.on_chunk.as_mut() {
            cb(SealedChunk {
                address: Reference::new(&addr)?,
                span,
                payload: payload.clone(),
            })?;
        }

        if level + 1 >= self.levels.len() {
            self.levels.push(Vec::new());
        }
        self.levels[level + 1].push(LevelRef {
            addr,
            span: total_span,
        });
        if self.levels[level + 1].len() == MAX_BRANCHES {
            self.collapse_level(level + 1)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swarm::bmt::make_content_addressed_chunk;

    #[test]
    fn single_chunk_matches_direct_cac() {
        let mut chunker = FileChunker::new();
        chunker.write(b"hello world").unwrap();
        let root = chunker.finalize().unwrap();
        let direct = make_content_addressed_chunk(b"hello world").unwrap();
        assert_eq!(root.address, direct.address);
        assert_eq!(root.span, direct.span);
    }

    #[test]
    fn empty_input_errors() {
        let chunker = FileChunker::new();
        assert!(chunker.finalize().is_err());
    }

    #[test]
    fn callback_fires_for_every_chunk() {
        // Two full leaves + one parent = 3 callbacks. Use Arc<Mutex> to
        // share a counter into the FnMut closure.
        use std::sync::{Arc, Mutex};
        let count = Arc::new(Mutex::new(0usize));
        let count_clone = count.clone();
        let mut chunker = FileChunker::with_callback(move |_c| {
            *count_clone.lock().unwrap() += 1;
            Ok(())
        });
        let payload = vec![0xabu8; CHUNK_SIZE * 2];
        chunker.write(&payload).unwrap();
        let _ = chunker.finalize().unwrap();
        // 2 leaves + 1 parent
        assert_eq!(*count.lock().unwrap(), 3);
    }

    #[test]
    fn root_span_is_total_byte_count() {
        let mut chunker = FileChunker::new();
        let payload = vec![0xcdu8; CHUNK_SIZE * 2 + 10];
        chunker.write(&payload).unwrap();
        let root = chunker.finalize().unwrap();
        assert_eq!(root.span.to_u64(), (CHUNK_SIZE * 2 + 10) as u64);
    }
}
