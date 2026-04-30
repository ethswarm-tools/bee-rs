//! Mantaray manifest trie. Mirrors `pkg/manifest` in bee-go and
//! `src/manifest` in bee-js.
//!
//! Construct an empty manifest with [`MantarayNode::new`]; insert
//! `(path, target, metadata)` entries with [`MantarayNode::add_fork`];
//! serialize with [`wire::marshal`] (after
//! [`wire::populate_self_addresses`]); parse a chunk back with
//! [`wire::unmarshal`].

mod helpers;
pub mod locator;
pub mod node;
pub mod wire;

pub use locator::{ResourceLocator, resolve_path, target_reference};
pub use node::{
    Fork, MAX_PREFIX_LENGTH, MantarayNode, NULL_ADDRESS, PATH_SEPARATOR, TYPE_EDGE, TYPE_VALUE,
    TYPE_WITH_METADATA, TYPE_WITH_PATH_SEPARATOR, is_null_address,
};
pub use wire::{
    VERSION_02_HASH_HEX, calculate_self_address, marshal, populate_self_addresses, unmarshal,
};
