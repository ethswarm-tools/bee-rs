//! [`ResourceLocator`] — a `Reference`-or-ENS-name wrapper used as
//! the path segment of `/bzz/{ref}` and `/bytes/{ref}`. Mirrors
//! bee-js `ResourceLocator`.
//!
//! Also defines [`resolve_path`], an offline manifest path-resolution
//! helper: given an unmarshaled [`MantarayNode`] and a slash-delimited
//! path, return the chunk reference of the target leaf.

use std::fmt;

use crate::manifest::{MantarayNode, is_null_address};
use crate::swarm::{Error, Reference};

/// Either a content reference (`Reference`) or an ENS name
/// (`<label>.eth`). Either form is valid as the `{ref}` segment in
/// Bee URLs; ENS resolution happens server-side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceLocator {
    /// A content reference (32 or 64 bytes).
    Reference(Reference),
    /// An ENS name. Bee accepts these in place of a hex reference.
    Ens(String),
}

impl ResourceLocator {
    /// Build from any input. Strings containing `.eth` are treated
    /// as ENS names; otherwise the input is parsed as a hex
    /// reference. Mirrors the `new ResourceLocator(...)` constructor
    /// in bee-js.
    pub fn parse(input: &str) -> Result<Self, Error> {
        if input.contains(".eth") {
            return Ok(Self::Ens(input.to_owned()));
        }
        Ok(Self::Reference(Reference::from_hex(input)?))
    }

    /// Build from an existing reference.
    pub fn from_reference(r: Reference) -> Self {
        Self::Reference(r)
    }

    /// Build from a string assumed to be an ENS name. Empty input
    /// returns [`Error::Argument`]; deeper validation is left to
    /// Bee.
    pub fn from_ens(name: impl Into<String>) -> Result<Self, Error> {
        let name = name.into();
        if name.is_empty() {
            return Err(Error::argument("empty ENS name"));
        }
        Ok(Self::Ens(name))
    }
}

impl fmt::Display for ResourceLocator {
    /// Renders the path-segment form: hex for [`ResourceLocator::Reference`],
    /// the raw label for [`ResourceLocator::Ens`].
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResourceLocator::Reference(r) => f.write_str(&r.to_hex()),
            ResourceLocator::Ens(name) => f.write_str(name),
        }
    }
}

impl From<Reference> for ResourceLocator {
    fn from(r: Reference) -> Self {
        Self::Reference(r)
    }
}

/// Offline manifest path resolution: walk `manifest` and return the
/// target [`Reference`] for `path`. Mirrors Bee's server-side
/// behavior for `GET /bzz/{ref}/{path}`.
///
/// `path` is a slash-delimited UTF-8 string (a leading `/` is
/// allowed and ignored). Returns [`Error::Argument`] when no fork
/// matches the path or when the matched node has no target.
pub fn resolve_path(manifest: &MantarayNode, path: &str) -> Result<Reference, Error> {
    let trimmed = path.trim_start_matches('/');
    let node = manifest.find(trimmed.as_bytes()).ok_or_else(|| {
        Error::argument(format!("no manifest entry for path {path:?}"))
    })?;
    target_reference(node)
        .ok_or_else(|| Error::argument(format!("manifest entry for {path:?} has no target")))
}

/// The fork target of `node` as an owned [`Reference`], or `None`
/// for branch-only nodes (NULL_ADDRESS).
pub fn target_reference(node: &MantarayNode) -> Option<Reference> {
    if is_null_address(&node.target_address) {
        return None;
    }
    Reference::new(&node.target_address).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ref_at(byte: u8) -> Reference {
        Reference::from_hex(&format!("{byte:02x}").repeat(32)).unwrap()
    }

    #[test]
    fn resource_locator_parses_ens_names() {
        let loc = ResourceLocator::parse("hello.eth").unwrap();
        assert_eq!(loc.to_string(), "hello.eth");
        assert!(matches!(loc, ResourceLocator::Ens(_)));
    }

    #[test]
    fn resource_locator_parses_hex_reference() {
        let hex = "ab".repeat(32);
        let loc = ResourceLocator::parse(&hex).unwrap();
        assert_eq!(loc.to_string(), hex);
        assert!(matches!(loc, ResourceLocator::Reference(_)));
    }

    #[test]
    fn resource_locator_rejects_invalid_hex() {
        assert!(ResourceLocator::parse("not-hex").is_err());
    }

    #[test]
    fn resource_locator_from_reference_round_trip() {
        let r = Reference::from_hex(&"cd".repeat(32)).unwrap();
        let loc = ResourceLocator::from_reference(r.clone());
        assert_eq!(loc.to_string(), r.to_hex());
    }

    #[test]
    fn resource_locator_from_ens_rejects_empty() {
        assert!(ResourceLocator::from_ens("").is_err());
    }

    #[test]
    fn resolve_path_finds_added_fork() {
        let mut m = MantarayNode::new();
        let target = ref_at(0xaa);
        m.add_fork(b"index.html", Some(&target), None);

        let got = resolve_path(&m, "/index.html").unwrap();
        assert_eq!(got, target);
    }

    #[test]
    fn resolve_path_handles_nested_paths() {
        let mut m = MantarayNode::new();
        let leaf = ref_at(0xbb);
        m.add_fork(b"assets/logo.png", Some(&leaf), None);

        let got = resolve_path(&m, "assets/logo.png").unwrap();
        assert_eq!(got, leaf);
    }

    #[test]
    fn resolve_path_returns_error_for_missing_path() {
        let m = MantarayNode::new();
        assert!(resolve_path(&m, "/nope").is_err());
    }

    #[test]
    fn resolve_path_with_leading_slash_works() {
        let mut m = MantarayNode::new();
        let leaf = ref_at(0xcc);
        m.add_fork(b"a", Some(&leaf), None);
        let got = resolve_path(&m, "/a").unwrap();
        assert_eq!(got, leaf);
    }
}
