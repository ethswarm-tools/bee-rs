//! Foundational types for the Swarm protocol: typed bytes, BMT chunk
//! addressing, SOC primitives, and the crate-level error type.
//!
//! Mirrors `pkg/swarm` in bee-go.

pub mod bmt;
pub mod bytes;
pub mod cid;
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
pub use cid::{
    CidType, DecodedCid, FEED_CODEC, MANIFEST_CODEC, convert_cid_to_reference,
    convert_reference_to_cid,
};
pub use duration::Duration as BeeDuration;
pub use errors::{Error, RESPONSE_BODY_CAP, Result};
pub use file_chunker::{ChunkerRoot, FileChunker, MAX_BRANCHES, SealedChunk};
pub use gsoc::{GSOC_DEFAULT_PROXIMITY, GSOC_MINE_START, gsoc_mine, proximity};
pub use keys::{PrivateKey, PublicKey, eth_signed_message_digest};
pub use network::Network;
pub use size::Size;
pub use soc::{
    SingleOwnerChunk, calculate_single_owner_chunk_address, make_single_owner_chunk,
    unmarshal_single_owner_chunk,
};
pub use tokens::{BZZ_DIGITS, Bzz, DAI_DIGITS, Dai};
pub use typed_bytes::{
    BATCH_ID_LENGTH, BatchId, ENCRYPTED_REFERENCE_LENGTH, ETH_ADDRESS_LENGTH, EthAddress,
    FEED_INDEX_LENGTH, FeedIndex, IDENTIFIER_LENGTH, Identifier, PEER_ADDRESS_LENGTH,
    PRIVATE_KEY_LENGTH, PUBLIC_KEY_LENGTH, PeerAddress, REFERENCE_LENGTH, Reference,
    SIGNATURE_LENGTH, SPAN_LENGTH, Signature, Span, TOPIC_LENGTH, TRANSACTION_ID_LENGTH, Topic,
    TransactionId,
};
