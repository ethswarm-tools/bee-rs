//! `MantarayNode`, `Fork`, and the trie operations (`add_fork`,
//! `find`, `find_closest`, `remove_fork`). The marshal / unmarshal
//! pair lives in [`crate::manifest::wire`].

use std::collections::BTreeMap;

use crate::swarm::{Error, Reference};

use super::helpers::common_prefix;

// ---- type bitfield (used by the wire format) ---------------------------

/// Type bit: this node has a non-null target address.
pub const TYPE_VALUE: u8 = 2;
/// Type bit: this node has child forks.
pub const TYPE_EDGE: u8 = 4;
/// Type bit: this node's path contains a non-leading `/`.
pub const TYPE_WITH_PATH_SEPARATOR: u8 = 8;
/// Type bit: this fork carries metadata.
pub const TYPE_WITH_METADATA: u8 = 16;

/// Path separator byte.
pub const PATH_SEPARATOR: u8 = b'/';

/// Per-fork prefix length cap. Longer paths are split into chained
/// nodes with prefixes ≤ [`MAX_PREFIX_LENGTH`] each.
pub const MAX_PREFIX_LENGTH: usize = 30;

/// All-zero 32-byte target address — the canonical "no target" value.
pub const NULL_ADDRESS: [u8; 32] = [0; 32];

/// True iff `b` is empty or all zeros (the canonical null target).
pub fn is_null_address(b: &[u8]) -> bool {
    b.is_empty() || b.iter().all(|&x| x == 0)
}

// ---- types -------------------------------------------------------------

/// One edge of the trie. `prefix` is the path bytes that select this
/// child; `node` is the child node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fork {
    /// Path bytes selecting this child.
    pub prefix: Vec<u8>,
    /// Child node.
    pub node: MantarayNode,
}

/// One node of the Mantaray trie. Field names mirror bee-js / bee-go.
///
/// - `obfuscation_key`: 32-byte XOR mask used when serializing this
///   node. Freshly-constructed nodes use a zero key.
/// - `self_address`: chunk address of the marshaled node, populated
///   after [`crate::manifest::wire::calculate_self_address`] /
///   `save_recursively`. `None` means "not computed".
/// - `target_address`: file reference at this node, or `NULL_ADDRESS`
///   for pure directory nodes.
/// - `path`: edge label inherited from the parent fork's prefix; the
///   root has an empty path.
/// - `forks`: child edges keyed by first prefix byte (ordered by key).
/// - `metadata`: optional key/value annotations carried in the wire
///   format; ordered by key so JSON serialization is deterministic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MantarayNode {
    /// 32-byte XOR mask used during marshal.
    pub obfuscation_key: [u8; 32],
    /// Chunk address of the marshaled node, when known.
    pub self_address: Option<[u8; 32]>,
    /// File reference at this node (32 bytes; `NULL_ADDRESS` = none).
    pub target_address: [u8; 32],
    /// Optional metadata annotations.
    pub metadata: Option<BTreeMap<String, String>>,
    /// Edge label inherited from the parent fork.
    pub path: Vec<u8>,
    /// Child edges keyed by first prefix byte.
    pub forks: BTreeMap<u8, Fork>,
}

impl Default for MantarayNode {
    fn default() -> Self {
        Self::new()
    }
}

impl MantarayNode {
    /// Empty Mantaray root.
    pub fn new() -> Self {
        Self {
            obfuscation_key: [0; 32],
            self_address: None,
            target_address: NULL_ADDRESS,
            metadata: None,
            path: Vec::new(),
            forks: BTreeMap::new(),
        }
    }

    /// True iff this node's `target_address` is the null address.
    pub fn is_null_target(&self) -> bool {
        is_null_address(&self.target_address)
    }

    /// Pack the type bitfield used in this node's outgoing fork
    /// record.
    pub fn determine_type(&self) -> u8 {
        let mut t = 0u8;
        let path_is_root_dir = self.path.len() == 1 && self.path[0] == PATH_SEPARATOR;

        if !self.is_null_target() || path_is_root_dir {
            t |= TYPE_VALUE;
        }
        if !self.forks.is_empty() {
            t |= TYPE_EDGE;
        }
        if self.path.contains(&PATH_SEPARATOR) && !path_is_root_dir {
            t |= TYPE_WITH_PATH_SEPARATOR;
        }
        if self.metadata.is_some() {
            t |= TYPE_WITH_METADATA;
        }
        t
    }

    // ---- trie traversal -------------------------------------------------

    /// Walk down forks while `path` matches. Returns the deepest node
    /// reached and the bytes of `path` matched along the way.
    pub fn find_closest<'a>(&'a self, path: &[u8]) -> (&'a MantarayNode, Vec<u8>) {
        let mut cur = self;
        let mut matched: Vec<u8> = Vec::new();
        let mut remaining = path;
        loop {
            if remaining.is_empty() {
                return (cur, matched);
            }
            let Some(fork) = cur.forks.get(&remaining[0]) else {
                return (cur, matched);
            };
            let common = common_prefix(&fork.prefix, remaining);
            if common.len() == fork.prefix.len() {
                matched.extend_from_slice(&fork.prefix);
                remaining = &remaining[fork.prefix.len()..];
                cur = &fork.node;
                continue;
            }
            return (cur, matched);
        }
    }

    /// Return the node whose full path equals `path`, or `None`.
    pub fn find(&self, path: &[u8]) -> Option<&MantarayNode> {
        let (closest, matched) = self.find_closest(path);
        if matched.len() == path.len() {
            Some(closest)
        } else {
            None
        }
    }

    // ---- collect helpers ------------------------------------------------

    /// All descendants whose `target_address` is non-null, with their
    /// full paths. The path is the concatenation of every fork prefix
    /// from the root down to the node.
    pub fn collect(&self) -> Vec<(Vec<u8>, &MantarayNode)> {
        let mut out = Vec::new();
        self.collect_into(&[], &mut out);
        out
    }

    fn collect_into<'a>(&'a self, prefix: &[u8], out: &mut Vec<(Vec<u8>, &'a MantarayNode)>) {
        for fork in self.forks.values() {
            let mut full = prefix.to_vec();
            full.extend_from_slice(&fork.prefix);
            if !fork.node.is_null_target() {
                out.push((full.clone(), &fork.node));
            }
            fork.node.collect_into(&full, out);
        }
    }

    /// `{full_path: reference}` for every leaf with a valid reference.
    /// Mirrors bee-go `CollectAndMap`.
    pub fn collect_and_map(&self) -> std::collections::HashMap<String, Reference> {
        let mut out = std::collections::HashMap::new();
        for (path, node) in self.collect() {
            if let Ok(r) = Reference::new(&node.target_address) {
                out.insert(String::from_utf8_lossy(&path).into_owned(), r);
            }
        }
        out
    }

    // ---- mutation: add_fork --------------------------------------------

    /// Insert a `(path, target, metadata)` entry. Long paths are
    /// chunked into chained nodes of up to [`MAX_PREFIX_LENGTH`]
    /// bytes each. Metadata is attached to the terminal node only.
    ///
    /// Invalidates `self_address` on every node touched along the
    /// way; call `calculate_self_address` (or `save_recursively`)
    /// before marshaling.
    pub fn add_fork(
        &mut self,
        path: &[u8],
        target: Option<&Reference>,
        metadata: Option<&BTreeMap<String, String>>,
    ) {
        self.self_address = None;
        self.add_fork_inner(path, target, metadata);
    }

    fn add_fork_inner(
        &mut self,
        path: &[u8],
        target: Option<&Reference>,
        metadata: Option<&BTreeMap<String, String>>,
    ) {
        if path.is_empty() {
            // Empty path: setting target/metadata on this node directly.
            // Bee-go's AddFork doesn't ever call into this with empty path
            // because chunks are always non-empty; this branch covers the
            // case where a fork's prefix equals path exactly.
            if let Some(r) = target {
                let bytes = r.as_bytes();
                let n = bytes.len().min(32);
                let mut ta = NULL_ADDRESS;
                ta[..n].copy_from_slice(&bytes[..n]);
                self.target_address = ta;
            }
            if let Some(m) = metadata {
                self.metadata = Some(m.clone());
            }
            return;
        }

        let chunk_len = path.len().min(MAX_PREFIX_LENGTH);
        let chunk: Vec<u8> = path[..chunk_len].to_vec();
        let after: Vec<u8> = path[chunk_len..].to_vec();
        let is_last = after.is_empty();
        let key = chunk[0];

        if let Some(existing) = self.forks.get(&key) {
            let common_len = common_prefix(&existing.prefix, &chunk).len();
            if common_len == existing.prefix.len() {
                // Existing fork's prefix is a prefix of (or equals) chunk;
                // descend into the existing node with whatever's left of
                // chunk plus `after`.
                let mut next: Vec<u8> = chunk[common_len..].to_vec();
                next.extend_from_slice(&after);
                let existing_mut = self.forks.get_mut(&key).unwrap();
                existing_mut.node.self_address = None;
                existing_mut.node.add_fork_inner(&next, target, metadata);
                return;
            }
            // Split: common_len < existing.prefix.len(). Take ownership
            // of the existing fork, build a new fork for `chunk` (leaf
            // values applied only if this chunk is the last), and merge.
            let existing_fork = self.forks.remove(&key).unwrap();
            let new_node = make_subtree_node_for_chunk(&chunk, is_last, target, metadata);
            let new_fork = Fork {
                prefix: chunk.clone(),
                node: new_node,
            };
            let merged = split_forks(new_fork, existing_fork);
            self.forks.insert(key, merged);

            if !is_last {
                // Descend to the node corresponding to `chunk` and continue
                // adding `after` underneath it.
                let merged_ref = self.forks.get_mut(&key).unwrap();
                if merged_ref.prefix == chunk {
                    // Case 1: chunk became parent of the existing fork.
                    merged_ref.node.add_fork_inner(&after, target, metadata);
                } else {
                    // Case 3: branch became parent; chunk's node sits one
                    // level below, keyed by chunk[merged_ref.prefix.len()].
                    let descend_byte = chunk[merged_ref.prefix.len()];
                    let chunk_fork = merged_ref
                        .node
                        .forks
                        .get_mut(&descend_byte)
                        .expect("split must produce chunk fork");
                    chunk_fork.node.add_fork_inner(&after, target, metadata);
                }
            }
            return;
        }

        // No existing fork at this key — insert a fresh one and recurse if
        // there's still more path left.
        let new_node = make_subtree_node_for_chunk(&chunk, is_last, target, metadata);
        self.forks.insert(
            key,
            Fork {
                prefix: chunk,
                node: new_node,
            },
        );
        if !is_last {
            let inserted = &mut self.forks.get_mut(&key).unwrap().node;
            inserted.add_fork_inner(&after, target, metadata);
        }
    }

    // ---- mutation: remove_fork -----------------------------------------

    /// Delete the fork rooted at `path`. If the removed node had
    /// children, they are re-inserted under the parent so the trie
    /// stays consistent.
    pub fn remove_fork(&mut self, path: &[u8]) -> Result<(), Error> {
        if path.is_empty() {
            return Err(Error::argument("remove_fork: path cannot be empty"));
        }
        if self.find(path).is_none() {
            return Err(Error::argument("remove_fork: not found"));
        }
        self.self_address = None;

        // Walk to the parent, recording the prefixes consumed at each
        // step so we can climb back down with mutable access.
        let mut chain: Vec<u8> = Vec::new(); // sequence of fork keys
        {
            let mut cur: &MantarayNode = self;
            let mut remaining: &[u8] = path;
            while !remaining.is_empty() {
                let key = remaining[0];
                let fork = cur
                    .forks
                    .get(&key)
                    .ok_or_else(|| Error::argument("remove_fork: chain broken"))?;
                let consumed = fork.prefix.len();
                chain.push(key);
                remaining = &remaining[consumed..];
                cur = &fork.node;
            }
        }

        let last_key = *chain.last().expect("chain non-empty when find found path");

        // Climb back down with mutable access to the parent.
        let mut parent: &mut MantarayNode = self;
        for key in &chain[..chain.len() - 1] {
            parent = &mut parent
                .forks
                .get_mut(key)
                .ok_or_else(|| Error::argument("remove_fork: chain broken"))?
                .node;
        }
        let removed = parent
            .forks
            .remove(&last_key)
            .ok_or_else(|| Error::argument("remove_fork: missing fork"))?;
        parent.self_address = None;

        // Re-insert each grandchild under self (root) using the
        // original full path of the removed node + each grandchild's
        // prefix. For each grandchild we know:
        //   * its prefix == full path of grandchild relative to removed node
        //   * its node may itself have grandchildren, so we recurse via
        //     its own collect to enumerate every leaf.
        for fork in removed.node.forks.into_values() {
            reinsert_subtree(self, path, &fork.prefix, fork.node);
        }
        Ok(())
    }
}

// ---- helpers used by add_fork / remove_fork ----------------------------

fn make_subtree_node_for_chunk(
    chunk: &[u8],
    is_last: bool,
    target: Option<&Reference>,
    metadata: Option<&BTreeMap<String, String>>,
) -> MantarayNode {
    let mut n = MantarayNode::new();
    n.path = chunk.to_vec();
    if is_last {
        if let Some(r) = target {
            let bytes = r.as_bytes();
            let len = bytes.len().min(32);
            n.target_address[..len].copy_from_slice(&bytes[..len]);
        }
        if let Some(m) = metadata {
            n.metadata = Some(m.clone());
        }
    }
    n
}

/// Re-insert `subtree` into `root` under the path
/// `path_prefix_under_root || subtree_prefix`. Walks the subtree's
/// own children and re-inserts each leaf separately so the
/// re-insertion process passes through `add_fork`'s normal split
/// logic.
fn reinsert_subtree(
    root: &mut MantarayNode,
    path_prefix_under_root: &[u8],
    subtree_prefix: &[u8],
    subtree: MantarayNode,
) {
    // Its own leaf, if any.
    let mut full_path = path_prefix_under_root.to_vec();
    full_path.extend_from_slice(subtree_prefix);

    if !subtree.is_null_target() {
        let target = Reference::new(&subtree.target_address).ok();
        root.add_fork(&full_path, target.as_ref(), subtree.metadata.as_ref());
    } else if subtree.metadata.is_some() {
        // Metadata-only intermediate — preserve.
        root.add_fork(&full_path, None, subtree.metadata.as_ref());
    }

    // Recurse into subtree's children.
    for fork in subtree.forks.into_values() {
        reinsert_subtree(root, &full_path, &fork.prefix, fork.node);
    }
}

// ---- fork split (resolves prefix collisions) ---------------------------

/// Resolve a prefix collision between two forks at the same first
/// byte. Returns the fork the parent should store, reshuffling node
/// paths as needed to keep the patricia invariant. Mirrors bee-js
/// `Fork.split`.
pub(super) fn split_forks(mut a: Fork, mut b: Fork) -> Fork {
    let common = common_prefix(&a.prefix, &b.prefix).to_vec();

    if common.len() == a.prefix.len() {
        // b lives under a.
        let remaining = b.prefix[common.len()..].to_vec();
        b.node.path = remaining.clone();
        b.prefix = remaining;
        a.node.forks.insert(b.prefix[0], b);
        return a;
    }
    if common.len() == b.prefix.len() {
        // a lives under b.
        let remaining = a.prefix[common.len()..].to_vec();
        a.node.path = remaining.clone();
        a.prefix = remaining;
        b.node.forks.insert(a.prefix[0], a);
        return b;
    }

    // Neither contains the other: insert a branching node.
    let mut branch = MantarayNode::new();
    branch.path = common.clone();
    let a_remaining = a.prefix[common.len()..].to_vec();
    let b_remaining = b.prefix[common.len()..].to_vec();
    a.node.path = a_remaining.clone();
    b.node.path = b_remaining.clone();
    a.prefix = a_remaining.clone();
    b.prefix = b_remaining.clone();
    branch.forks.insert(a.prefix[0], a);
    branch.forks.insert(b.prefix[0], b);

    Fork {
        prefix: common,
        node: branch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r32(byte: u8) -> Reference {
        Reference::new(&[byte; 32]).unwrap()
    }

    #[test]
    fn add_and_find_simple() {
        let mut root = MantarayNode::new();
        root.add_fork(b"hello", Some(&r32(0x11)), None);
        let n = root.find(b"hello").unwrap();
        assert_eq!(n.target_address, [0x11; 32]);
    }

    #[test]
    fn find_closest_returns_match_only_when_descended() {
        // bee-go / bee-js semantics: matched is the path actually consumed
        // by descending into existing forks. If a fork's prefix diverges
        // from the search path before being fully consumed, we don't
        // descend and the matched length stays at the last fully-consumed
        // boundary.
        let mut root = MantarayNode::new();
        root.add_fork(b"hello", Some(&r32(0x11)), None);
        // "helicopter" diverges inside fork prefix "hello" → no descent.
        let (_, matched) = root.find_closest(b"helicopter");
        assert_eq!(matched, b"");
        // "hello/world" descends through fork "hello" and stops there.
        let (_, matched) = root.find_closest(b"hello/world");
        assert_eq!(matched, b"hello");
    }

    #[test]
    fn add_two_paths_share_prefix() {
        let mut root = MantarayNode::new();
        root.add_fork(b"hello", Some(&r32(0x11)), None);
        root.add_fork(b"help", Some(&r32(0x22)), None);
        assert_eq!(root.find(b"hello").unwrap().target_address, [0x11; 32]);
        assert_eq!(root.find(b"help").unwrap().target_address, [0x22; 32]);
    }

    #[test]
    fn long_path_chunks_into_chained_nodes() {
        let mut root = MantarayNode::new();
        let path = vec![b'a'; 100]; // > MAX_PREFIX_LENGTH
        root.add_fork(&path, Some(&r32(0x33)), None);
        assert_eq!(root.find(&path).unwrap().target_address, [0x33; 32]);
    }

    #[test]
    fn remove_fork_removes_and_keeps_other_paths() {
        let mut root = MantarayNode::new();
        root.add_fork(b"foo/a", Some(&r32(0xaa)), None);
        root.add_fork(b"foo/b", Some(&r32(0xbb)), None);
        root.remove_fork(b"foo/a").unwrap();
        assert!(root.find(b"foo/a").is_none());
        assert_eq!(root.find(b"foo/b").unwrap().target_address, [0xbb; 32]);
    }

    #[test]
    fn remove_fork_reinserts_grandchildren() {
        // Add three leaves so the removed leaf has siblings whose
        // common-prefix structure must be rebuilt after re-insertion.
        let mut root = MantarayNode::new();
        root.add_fork(b"a/b/c", Some(&r32(0x01)), None);
        root.add_fork(b"a/b/d", Some(&r32(0x02)), None);
        root.add_fork(b"a/x", Some(&r32(0x03)), None);
        // Remove an exact leaf path; the trie should still contain
        // every other leaf.
        root.remove_fork(b"a/b/c").unwrap();
        assert!(root.find(b"a/b/c").is_none());
        assert_eq!(root.find(b"a/b/d").unwrap().target_address, [0x02; 32]);
        assert_eq!(root.find(b"a/x").unwrap().target_address, [0x03; 32]);
    }

    #[test]
    fn remove_fork_unknown_path_errors() {
        let mut root = MantarayNode::new();
        root.add_fork(b"hello", Some(&r32(0x01)), None);
        assert!(root.remove_fork(b"world").is_err());
    }

    #[test]
    fn collect_returns_full_paths_for_leaves() {
        let mut root = MantarayNode::new();
        root.add_fork(b"a", Some(&r32(0x01)), None);
        root.add_fork(b"ab", Some(&r32(0x02)), None);
        root.add_fork(b"abc", Some(&r32(0x03)), None);
        let mut paths: Vec<Vec<u8>> = root.collect().into_iter().map(|(p, _)| p).collect();
        paths.sort();
        assert_eq!(paths, vec![b"a".to_vec(), b"ab".to_vec(), b"abc".to_vec()]);
    }

    #[test]
    fn collect_and_map_returns_string_keyed_refs() {
        let mut root = MantarayNode::new();
        root.add_fork(b"hello.txt", Some(&r32(0xee)), None);
        let map = root.collect_and_map();
        assert_eq!(map.len(), 1);
        let r = map.get("hello.txt").unwrap();
        assert_eq!(r.as_bytes(), &[0xee; 32]);
    }

    #[test]
    fn determine_type_packs_bits() {
        let mut root = MantarayNode::new();
        root.add_fork(b"hello", Some(&r32(0x11)), None);
        let leaf = root.find(b"hello").unwrap();
        let t = leaf.determine_type();
        assert!(t & TYPE_VALUE != 0);
        assert!(t & TYPE_EDGE == 0);
    }

    #[test]
    fn metadata_is_preserved() {
        let mut root = MantarayNode::new();
        let meta: BTreeMap<String, String> =
            [("k".to_string(), "v".to_string())].into_iter().collect();
        root.add_fork(b"foo", Some(&r32(0xaa)), Some(&meta));
        assert_eq!(root.find(b"foo").unwrap().metadata, Some(meta));
    }
}
