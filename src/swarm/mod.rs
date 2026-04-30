//! Foundational types for the Swarm protocol: typed bytes, BMT chunk
//! addressing, SOC primitives, and the crate-level error type.
//!
//! Mirrors `pkg/swarm` in bee-go.

pub mod bmt;
pub mod bytes;
pub mod duration;
pub mod errors;
pub mod file_chunker;
pub mod gsoc;
pub mod keys;
pub mod network;
pub mod size;
pub mod soc;
pub mod tokens;
pub mod typed_bytes;

pub use bmt::{
    CHUNK_SIZE, Chunk, MAX_PAYLOAD_SIZE, MIN_PAYLOAD_SIZE, SEGMENT_SIZE, SEGMENTS_COUNT,
    calculate_chunk_address, keccak256, make_content_addressed_chunk,
};
pub use errors::{Error, RESPONSE_BODY_CAP, Result};
pub use file_chunker::{ChunkerRoot, FileChunker, MAX_BRANCHES, SealedChunk};
pub use gsoc::{GSOC_DEFAULT_PROXIMITY, GSOC_MINE_START, gsoc_mine, proximity};
pub use keys::{PrivateKey, PublicKey, eth_signed_message_digest};
pub use duration::Duration as BeeDuration;
pub use network::Network;
pub use size::Size;
pub use tokens::{BZZ_DIGITS, Bzz, DAI_DIGITS, Dai};
pub use soc::{
    SingleOwnerChunk, calculate_single_owner_chunk_address, make_single_owner_chunk,
    unmarshal_single_owner_chunk,
};
pub use typed_bytes::{
    BATCH_ID_LENGTH, BatchId, ENCRYPTED_REFERENCE_LENGTH, ETH_ADDRESS_LENGTH, EthAddress,
    FEED_INDEX_LENGTH, FeedIndex, IDENTIFIER_LENGTH, Identifier, PEER_ADDRESS_LENGTH, PeerAddress,
    PRIVATE_KEY_LENGTH, PUBLIC_KEY_LENGTH, REFERENCE_LENGTH, Reference, SIGNATURE_LENGTH,
    SPAN_LENGTH, Signature, Span, TOPIC_LENGTH, TRANSACTION_ID_LENGTH, Topic, TransactionId,
};
