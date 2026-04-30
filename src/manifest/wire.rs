//! Mantaray v0.2 wire format. Mirrors bee-go's
//! `pkg/manifest/manifest.go` Marshal / Unmarshal block.
//!
//! Layout (after the 32-byte obfuscation key, body XORed with the key):
//!
//! ```text
//! key (32) | versionHash (31) | targetLen (1) | target (targetLen)
//!         | forkBitmap (32)
//!         | for each set bit i in 0..256:
//!               type (1) | prefixLen (1) | prefix (30, 0-padded)
//!                       | childAddr (selfAddrLen)
//!                       | if type & WITH_METADATA:
//!                             metaLen (2 BE) | metaJson (metaLen, 0x0a-padded to /32)
//! ```

use crate::swarm::errors::Error;
use crate::swarm::file_chunker::FileChunker;

use super::helpers::{Reader, get_bit_le, has_type, pad_end_to_multiple, set_bit_le, xor_in_place};
use super::node::{
    Fork, MAX_PREFIX_LENGTH, MantarayNode, NULL_ADDRESS, PATH_SEPARATOR, TYPE_WITH_METADATA,
    is_null_address,
};

/// Hex string of the v0.2 version hash. Only the first 31 bytes are
/// written into the marshaled header; the 32nd byte slot is reused
/// as the target-address length.
pub const VERSION_02_HASH_HEX: &str =
    "5768b3b6a7db56d21d1abff40d41cebfc83448fed8d7e9b06ec0d3b073f28f7b";

fn version_02_hash() -> [u8; 32] {
    let mut out = [0u8; 32];
    let bytes = hex::decode(VERSION_02_HASH_HEX).expect("static hex constant");
    out.copy_from_slice(&bytes);
    out
}

// ---- marshal ----------------------------------------------------------

/// Serialize a node (and the references to its children) using the
/// v0.2 wire format. Children must already have `self_address`
/// populated; pass through [`calculate_self_address`] (or
/// `save_recursively`, P1) to populate them.
pub fn marshal(node: &MantarayNode) -> Result<Vec<u8>, Error> {
    // Children must have self_address populated.
    for fork in node.forks.values() {
        if fork.node.self_address.is_none() {
            return Err(Error::argument(
                "marshal: child self_address not populated; call calculate_self_address first",
            ));
        }
    }

    let v02 = version_02_hash();

    // Header: 31 bytes of v02 hash + 1 byte for target-address length.
    let mut header = [0u8; 32];
    header[..31].copy_from_slice(&v02[..31]);
    let path_is_root_dir = node.path.len() == 1 && node.path[0] == PATH_SEPARATOR;
    let root_dir_node = is_null_address(&node.target_address) && path_is_root_dir;
    if root_dir_node {
        header[31] = 0;
    } else {
        header[31] = node.target_address.len() as u8;
    }

    // Fork bitmap.
    let mut fork_bitmap = [0u8; 32];
    for &k in node.forks.keys() {
        set_bit_le(&mut fork_bitmap, k as usize);
    }

    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(&header);
    if !root_dir_node {
        body.extend_from_slice(&node.target_address);
    }
    body.extend_from_slice(&fork_bitmap);

    // Forks emitted in ascending bitmap order so encoders agree.
    for i in 0..256u16 {
        if !get_bit_le(&fork_bitmap, i as usize) {
            continue;
        }
        if let Some(fork) = node.forks.get(&(i as u8)) {
            let fb = marshal_fork(fork)?;
            body.extend_from_slice(&fb);
        }
    }

    xor_in_place(&mut body, &node.obfuscation_key);

    let mut out = Vec::with_capacity(32 + body.len());
    out.extend_from_slice(&node.obfuscation_key);
    out.extend_from_slice(&body);
    Ok(out)
}

fn marshal_fork(f: &Fork) -> Result<Vec<u8>, Error> {
    let self_addr = f
        .node
        .self_address
        .ok_or_else(|| Error::argument("marshal_fork: child self_address not populated"))?;
    if f.prefix.len() > MAX_PREFIX_LENGTH {
        return Err(Error::argument(format!(
            "marshal_fork: prefix length {} exceeds max {}",
            f.prefix.len(),
            MAX_PREFIX_LENGTH
        )));
    }

    let mut out: Vec<u8> = Vec::with_capacity(1 + 1 + MAX_PREFIX_LENGTH + 32);
    out.push(f.node.determine_type());
    out.push(f.prefix.len() as u8);
    out.extend_from_slice(&f.prefix);
    if f.prefix.len() < MAX_PREFIX_LENGTH {
        out.resize(out.len() + (MAX_PREFIX_LENGTH - f.prefix.len()), 0);
    }
    out.extend_from_slice(&self_addr);

    if let Some(ref m) = f.node.metadata {
        // Metadata layout: 2-byte BE length of (json + padding) +
        // json + 0x0a padding to 32-byte multiple. The 2-byte length
        // value covers (json + padding), excluding itself.
        let json = serde_json::to_vec(m)?;
        let mut body = vec![0u8, 0u8];
        body.extend_from_slice(&json);
        let padded = pad_end_to_multiple(body, 32, 0x0a);
        // Now write the actual length (padded.len() - 2).
        let total = (padded.len() - 2) as u16;
        let mut padded = padded;
        padded[0] = (total >> 8) as u8;
        padded[1] = (total & 0xff) as u8;
        out.extend_from_slice(&padded);
    }

    Ok(out)
}

// ---- unmarshal --------------------------------------------------------

/// Parse a marshaled node. `self_address` is the chunk address that
/// produced `data`; it determines the per-fork child address length
/// (32 bytes for standard Swarm references). Children are referenced
/// by `self_address` only and remain unloaded — call `load_recursively`
/// (P1) to follow them.
pub fn unmarshal(data: &[u8], self_address: &[u8]) -> Result<MantarayNode, Error> {
    if data.len() < 32 {
        return Err(Error::argument("mantaray: data too short"));
    }
    let mut obfuscation_key = [0u8; 32];
    obfuscation_key.copy_from_slice(&data[..32]);
    let mut body = data[32..].to_vec();
    xor_in_place(&mut body, &obfuscation_key);

    let mut r = Reader::new(&body);
    let version_hash = r
        .read(31)
        .ok_or_else(|| Error::argument("mantaray: short version hash"))?;
    if version_hash != &version_02_hash()[..31] {
        return Err(Error::argument("mantaray: invalid version hash"));
    }
    let target_address_length = r
        .read_byte()
        .ok_or_else(|| Error::argument("mantaray: missing target length"))?;
    let mut target = NULL_ADDRESS;
    if target_address_length > 0 {
        let raw = r
            .read(target_address_length as usize)
            .ok_or_else(|| Error::argument("mantaray: short target"))?;
        let n = raw.len().min(32);
        target[..n].copy_from_slice(&raw[..n]);
    }
    let fork_bitmap_slice = r
        .read(32)
        .ok_or_else(|| Error::argument("mantaray: short fork bitmap"))?;
    let mut fork_bitmap = [0u8; 32];
    fork_bitmap.copy_from_slice(fork_bitmap_slice);

    let mut self_address_arr = [0u8; 32];
    let take = self_address.len().min(32);
    self_address_arr[..take].copy_from_slice(&self_address[..take]);

    let mut node = MantarayNode {
        obfuscation_key,
        self_address: Some(self_address_arr),
        target_address: target,
        metadata: None,
        path: Vec::new(),
        forks: std::collections::BTreeMap::new(),
    };

    for i in 0..256u16 {
        if !get_bit_le(&fork_bitmap, i as usize) {
            continue;
        }
        let fork = unmarshal_fork(&mut r, self_address.len())
            .map_err(|e| Error::argument(format!("mantaray fork {i}: {e}")))?;
        node.forks.insert(i as u8, fork);
    }
    Ok(node)
}

fn unmarshal_fork(r: &mut Reader<'_>, address_length: usize) -> Result<Fork, Error> {
    let t = r
        .read_byte()
        .ok_or_else(|| Error::argument("fork: missing type"))?;
    let prefix_length = r
        .read_byte()
        .ok_or_else(|| Error::argument("fork: missing prefix length"))?;
    let prefix = r
        .read(prefix_length as usize)
        .ok_or_else(|| Error::argument("fork: short prefix"))?;
    let prefix = prefix.to_vec();
    if (prefix_length as usize) < MAX_PREFIX_LENGTH {
        r.read(MAX_PREFIX_LENGTH - prefix_length as usize)
            .ok_or_else(|| Error::argument("fork: short prefix padding"))?;
    }
    let self_address = r
        .read(address_length)
        .ok_or_else(|| Error::argument("fork: short child address"))?;
    let mut self_address_arr = [0u8; 32];
    let take = self_address.len().min(32);
    self_address_arr[..take].copy_from_slice(&self_address[..take]);

    let metadata = if has_type(t, TYPE_WITH_METADATA) {
        let len_bytes = r
            .read(2)
            .ok_or_else(|| Error::argument("fork: short metadata length"))?;
        let mlen = ((len_bytes[0] as usize) << 8) | (len_bytes[1] as usize);
        let meta_bytes = r
            .read(mlen)
            .ok_or_else(|| Error::argument("fork: short metadata"))?;
        let trimmed: &[u8] = trim_right(meta_bytes, 0x0a);
        if trimmed.is_empty() {
            Some(std::collections::BTreeMap::new())
        } else {
            let parsed: std::collections::BTreeMap<String, String> =
                serde_json::from_slice(trimmed)?;
            Some(parsed)
        }
    } else {
        None
    };

    Ok(Fork {
        prefix: prefix.clone(),
        node: MantarayNode {
            obfuscation_key: [0; 32],
            self_address: Some(self_address_arr),
            target_address: NULL_ADDRESS,
            metadata,
            path: prefix,
            forks: std::collections::BTreeMap::new(),
        },
    })
}

fn trim_right(data: &[u8], byte: u8) -> &[u8] {
    let mut end = data.len();
    while end > 0 && data[end - 1] == byte {
        end -= 1;
    }
    &data[..end]
}

// ---- self-address calculation -----------------------------------------

/// Return this node's chunk address. If `self_address` is already
/// populated it is returned unchanged. Otherwise the marshaled body
/// is streamed through a [`FileChunker`], producing a single-leaf or
/// multi-level Merkle-tree root as appropriate.
pub fn calculate_self_address(node: &MantarayNode) -> Result<[u8; 32], Error> {
    if let Some(addr) = node.self_address {
        return Ok(addr);
    }
    let data = marshal(node)?;
    let mut chunker = FileChunker::new();
    chunker.write(&data)?;
    let root = chunker.finalize()?;
    let mut out = [0u8; 32];
    let bytes = root.address.as_bytes();
    let n = bytes.len().min(32);
    out[..n].copy_from_slice(&bytes[..n]);
    Ok(out)
}

/// Recursively populate `self_address` on this node and every child.
/// Mirrors bee-go's pre-marshal walk.
pub fn populate_self_addresses(node: &mut MantarayNode) -> Result<[u8; 32], Error> {
    if let Some(addr) = node.self_address {
        return Ok(addr);
    }
    // Children first.
    for fork in node.forks.values_mut() {
        let child_addr = populate_self_addresses(&mut fork.node)?;
        fork.node.self_address = Some(child_addr);
    }
    let addr = calculate_self_address(node)?;
    node.self_address = Some(addr);
    Ok(addr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swarm::Reference;

    fn r32(byte: u8) -> Reference {
        Reference::new(&[byte; 32]).unwrap()
    }

    #[test]
    fn empty_root_round_trips() {
        let mut root = MantarayNode::new();
        let addr = populate_self_addresses(&mut root).unwrap();
        let bytes = marshal(&root).unwrap();
        let parsed = unmarshal(&bytes, &addr).unwrap();
        assert_eq!(parsed.target_address, root.target_address);
        assert!(parsed.forks.is_empty());
    }

    #[test]
    fn single_fork_round_trips() {
        let mut root = MantarayNode::new();
        root.add_fork(b"hello", Some(&r32(0x11)), None);
        let addr = populate_self_addresses(&mut root).unwrap();
        let bytes = marshal(&root).unwrap();
        let parsed = unmarshal(&bytes, &addr).unwrap();
        assert_eq!(parsed.forks.len(), 1);
        let fork = parsed.forks.values().next().unwrap();
        assert_eq!(fork.prefix, b"hello".to_vec());
    }

    /// Cross-checked against bee-go: a fresh root with a single fork
    /// `("hello", 0x11…)` and a zero obfuscation key marshals to this
    /// exact byte sequence, and the chunk address matches.
    #[test]
    fn marshal_matches_bee_go_byte_for_byte() {
        let mut root = MantarayNode::new();
        root.add_fork(b"hello", Some(&r32(0x11)), None);
        let addr = populate_self_addresses(&mut root).unwrap();
        let bytes = marshal(&root).unwrap();

        let want_addr = "4a108a007edc05666fb74d907b5c9c03e893604d5b8306bdaf8e0fc5f5c772ac";
        let want_data = "00000000000000000000000000000000000000000000000000000000000000005768b3b6a7db56d21d1abff40d41cebfc83448fed8d7e9b06ec0d3b073f28f2000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001000000000000000000000000000000000000020568656c6c6f000000000000000000000000000000000000000000000000007900d12931b05071bd6860466d56a64d9eb2fc409725f0305d3df0083fa3d2fc";
        assert_eq!(hex::encode(addr), want_addr);
        assert_eq!(hex::encode(&bytes), want_data);
    }

    #[test]
    fn metadata_round_trips() {
        let mut root = MantarayNode::new();
        let meta: std::collections::BTreeMap<String, String> = [
            ("Content-Type".to_string(), "text/plain".to_string()),
            ("filename".to_string(), "hello.txt".to_string()),
        ]
        .into_iter()
        .collect();
        root.add_fork(b"hello.txt", Some(&r32(0xee)), Some(&meta));
        let addr = populate_self_addresses(&mut root).unwrap();
        let bytes = marshal(&root).unwrap();
        let parsed = unmarshal(&bytes, &addr).unwrap();

        let fork = parsed.forks.values().next().unwrap();
        assert_eq!(fork.node.metadata.as_ref().unwrap().len(), 2);
        assert_eq!(
            fork.node
                .metadata
                .as_ref()
                .unwrap()
                .get("filename")
                .unwrap(),
            "hello.txt"
        );
    }

    #[test]
    fn multiple_paths_round_trip_full_structure() {
        let mut root = MantarayNode::new();
        root.add_fork(b"a/b/c.txt", Some(&r32(0x01)), None);
        root.add_fork(b"a/b/d.txt", Some(&r32(0x02)), None);
        root.add_fork(b"x.txt", Some(&r32(0x03)), None);

        let addr = populate_self_addresses(&mut root).unwrap();
        let bytes = marshal(&root).unwrap();
        let _parsed = unmarshal(&bytes, &addr).unwrap();
    }

    #[test]
    fn obfuscation_key_round_trips() {
        let mut root = MantarayNode::new();
        root.add_fork(b"hello", Some(&r32(0x11)), None);
        root.obfuscation_key = [0xaa; 32];
        let addr = populate_self_addresses(&mut root).unwrap();
        let bytes = marshal(&root).unwrap();
        // First 32 bytes are the key in plaintext; rest XORed.
        assert_eq!(&bytes[..32], &[0xaa; 32]);
        let parsed = unmarshal(&bytes, &addr).unwrap();
        assert_eq!(parsed.obfuscation_key, [0xaa; 32]);
        assert_eq!(parsed.forks.len(), 1);
    }
}
