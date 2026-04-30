//! Upload / download options. Mirrors bee-go's `pkg/api/options.go`.
//!
//! Option fields use [`Option<bool>`] to distinguish "unset" from
//! "explicitly false" — matching bee-go's `*bool` semantics. `None`
//! omits the header; `Some(false)` sends the literal string `"false"`.

use crate::swarm::{BatchId, PublicKey, Reference};

/// Data redundancy level applied at upload time. Mirrors bee-js
/// `RedundancyLevel`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum RedundancyLevel {
    /// No redundancy.
    Off = 0,
    /// Medium redundancy.
    Medium = 1,
    /// Strong redundancy.
    Strong = 2,
    /// Insane redundancy.
    Insane = 3,
    /// Paranoid redundancy.
    Paranoid = 4,
}

impl RedundancyLevel {
    /// Numeric value used as the `Swarm-Redundancy-Level` header.
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Chunk-prefetch policy used when downloading erasure-coded data.
/// Mirrors bee-js `RedundancyStrategy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum RedundancyStrategy {
    /// No prefetch.
    None = 0,
    /// Data-only prefetch.
    Data = 1,
    /// Proximity-based prefetch.
    Proximity = 2,
    /// Race strategies.
    Race = 3,
}

impl RedundancyStrategy {
    /// Numeric value used as the `Swarm-Redundancy-Strategy` header.
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Base set of options accepted by every upload endpoint. Mirrors
/// bee-js / bee-go `UploadOptions`.
#[derive(Clone, Debug, Default)]
pub struct UploadOptions {
    /// When `Some(true)`, instruct Bee to wrap the upload in an Access
    /// Control Trie (ACT). The history root is returned via
    /// `Swarm-Act-History-Address`.
    pub act: Option<bool>,
    /// Existing ACT history root for re-upload under the same access
    /// policy.
    pub act_history_address: Option<Reference>,
    /// Pin the uploaded data on the local Bee node.
    pub pin: Option<bool>,
    /// Encrypt chunks; the returned reference is 64 bytes (CAC ||
    /// encryption key).
    pub encrypt: Option<bool>,
    /// Existing tag UID to attach for sync tracking. `0` omits the
    /// header (matching bee-go's `uint32(0)` zero-value semantics).
    pub tag: u32,
    /// Toggle "Bee waits for full sync" (`Some(false)`) vs
    /// "Bee accepts and syncs in background" (`Some(true)`, the Bee
    /// default).
    pub deferred: Option<bool>,
}

/// `UploadOptions` plus a redundancy level. Mirrors bee-go
/// `RedundantUploadOptions`.
#[derive(Clone, Debug, Default)]
pub struct RedundantUploadOptions {
    /// Inherited base options.
    pub base: UploadOptions,
    /// Redundancy level (`Off` omits the header).
    pub redundancy_level: Option<RedundancyLevel>,
}

/// File-specific upload options for `POST /bzz`. Mirrors bee-go
/// `FileUploadOptions`.
#[derive(Clone, Debug, Default)]
pub struct FileUploadOptions {
    /// Inherited base options.
    pub base: UploadOptions,
    /// Explicit `Content-Length` (use when uploading from a stream of
    /// unknown length).
    pub size: Option<u64>,
    /// Explicit `Content-Type`.
    pub content_type: Option<String>,
    /// Redundancy level (`Off` omits the header).
    pub redundancy_level: Option<RedundancyLevel>,
}

/// Collection upload options for tar `POST /bzz`. Mirrors bee-go
/// `CollectionUploadOptions`.
#[derive(Clone, Debug, Default)]
pub struct CollectionUploadOptions {
    /// Inherited base options.
    pub base: UploadOptions,
    /// Document served when the collection root is requested.
    pub index_document: Option<String>,
    /// Document served when a path inside the collection is missing.
    pub error_document: Option<String>,
    /// Redundancy level (`Off` omits the header).
    pub redundancy_level: Option<RedundancyLevel>,
}

/// Download options. All fields are optional; `Default::default()`
/// keeps Bee defaults. Mirrors bee-go `DownloadOptions`.
#[derive(Clone, Debug, Default)]
pub struct DownloadOptions {
    /// Erasure-coded prefetch policy.
    pub redundancy_strategy: Option<RedundancyStrategy>,
    /// Allow strategy fallback. Bee default is `true`.
    pub fallback: Option<bool>,
    /// Per-chunk retrieval timeout in milliseconds.
    pub timeout_ms: Option<u64>,
    /// ACT publisher public key.
    pub act_publisher: Option<PublicKey>,
    /// ACT history root for permission resolution.
    pub act_history_address: Option<Reference>,
    /// Unix timestamp at which to evaluate ACT permissions.
    pub act_timestamp: Option<i64>,
}

/// Postage stamp creation options. Mirrors bee-js / bee-go
/// `PostageBatchOptions`.
#[derive(Clone, Debug, Default)]
pub struct PostageBatchOptions {
    /// Human-readable label.
    pub label: Option<String>,
    /// Whether the batch is immutable.
    pub immutable: Option<bool>,
    /// Override the gas price (decimal string).
    pub gas_price: Option<String>,
    /// Override the gas limit (decimal string).
    pub gas_limit: Option<String>,
}

// ---- header preparation -------------------------------------------------

/// Header pairs: name + value. Used to push headers into a request
/// builder in the order they were added.
pub type HeaderPairs = Vec<(&'static str, String)>;

fn bool_str(b: bool) -> &'static str {
    if b { "true" } else { "false" }
}

fn push_upload_options(out: &mut HeaderPairs, opts: &UploadOptions) {
    if let Some(v) = opts.pin {
        out.push(("Swarm-Pin", bool_str(v).to_string()));
    }
    if let Some(v) = opts.encrypt {
        out.push(("Swarm-Encrypt", bool_str(v).to_string()));
    }
    if opts.tag > 0 {
        out.push(("Swarm-Tag", opts.tag.to_string()));
    }
    if let Some(v) = opts.deferred {
        out.push(("Swarm-Deferred-Upload", bool_str(v).to_string()));
    }
    if let Some(v) = opts.act {
        out.push(("Swarm-Act", bool_str(v).to_string()));
    }
    if let Some(ref r) = opts.act_history_address {
        out.push(("Swarm-Act-History-Address", r.to_hex()));
    }
}

/// Build the header set for a base upload. The batch is required.
pub fn prepare_upload_headers(batch_id: &BatchId, opts: Option<&UploadOptions>) -> HeaderPairs {
    let mut out = vec![("Swarm-Postage-Batch-Id", batch_id.to_hex())];
    if let Some(o) = opts {
        push_upload_options(&mut out, o);
    }
    out
}

/// Build the header set for a redundant upload.
pub fn prepare_redundant_upload_headers(
    batch_id: &BatchId,
    opts: Option<&RedundantUploadOptions>,
) -> HeaderPairs {
    match opts {
        None => prepare_upload_headers(batch_id, None),
        Some(o) => {
            let mut out = prepare_upload_headers(batch_id, Some(&o.base));
            if let Some(level) = o.redundancy_level {
                if !matches!(level, RedundancyLevel::Off) {
                    out.push(("Swarm-Redundancy-Level", level.as_u8().to_string()));
                }
            }
            out
        }
    }
}

/// Build the header set for a `POST /bzz` file upload.
pub fn prepare_file_upload_headers(
    batch_id: &BatchId,
    opts: Option<&FileUploadOptions>,
) -> HeaderPairs {
    match opts {
        None => prepare_upload_headers(batch_id, None),
        Some(o) => {
            let mut out = prepare_upload_headers(batch_id, Some(&o.base));
            if let Some(size) = o.size {
                out.push(("Content-Length", size.to_string()));
            }
            if let Some(ref ct) = o.content_type {
                out.push(("Content-Type", ct.clone()));
            }
            if let Some(level) = o.redundancy_level {
                if !matches!(level, RedundancyLevel::Off) {
                    out.push(("Swarm-Redundancy-Level", level.as_u8().to_string()));
                }
            }
            out
        }
    }
}

/// Build the header set for a tar `POST /bzz` collection upload.
pub fn prepare_collection_upload_headers(
    batch_id: &BatchId,
    opts: Option<&CollectionUploadOptions>,
) -> HeaderPairs {
    match opts {
        None => prepare_upload_headers(batch_id, None),
        Some(o) => {
            let mut out = prepare_upload_headers(batch_id, Some(&o.base));
            if let Some(ref idx) = o.index_document {
                out.push(("Swarm-Index-Document", idx.clone()));
            }
            if let Some(ref err) = o.error_document {
                out.push(("Swarm-Error-Document", err.clone()));
            }
            if let Some(level) = o.redundancy_level {
                if !matches!(level, RedundancyLevel::Off) {
                    out.push(("Swarm-Redundancy-Level", level.as_u8().to_string()));
                }
            }
            out
        }
    }
}

/// Build the header set for a download. Setting any of `act_publisher`
/// / `act_history_address` / `act_timestamp` implicitly turns
/// `Swarm-Act` on.
pub fn prepare_download_headers(opts: Option<&DownloadOptions>) -> HeaderPairs {
    let mut out = HeaderPairs::new();
    let Some(o) = opts else { return out };

    if let Some(s) = o.redundancy_strategy {
        out.push(("Swarm-Redundancy-Strategy", s.as_u8().to_string()));
    }
    if let Some(v) = o.fallback {
        out.push(("Swarm-Redundancy-Fallback-Mode", bool_str(v).to_string()));
    }
    if let Some(ms) = o.timeout_ms {
        if ms > 0 {
            out.push(("Swarm-Chunk-Retrieval-Timeout", ms.to_string()));
        }
    }
    let mut act = false;
    if let Some(ref pk) = o.act_publisher {
        if let Ok(hex) = pk.compressed_hex() {
            out.push(("Swarm-Act-Publisher", hex));
            act = true;
        }
    }
    if let Some(ref r) = o.act_history_address {
        out.push(("Swarm-Act-History-Address", r.to_hex()));
        act = true;
    }
    if let Some(ts) = o.act_timestamp {
        if ts > 0 {
            out.push(("Swarm-Act-Timestamp", ts.to_string()));
            act = true;
        }
    }
    if act {
        out.push(("Swarm-Act", "true".to_string()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch() -> BatchId {
        BatchId::new(&[0xab; 32]).unwrap()
    }

    fn header<'a>(h: &'a [(&'static str, String)], name: &str) -> Option<&'a str> {
        h.iter().find(|(k, _)| *k == name).map(|(_, v)| v.as_str())
    }

    #[test]
    fn upload_headers_omit_unset_fields() {
        let h = prepare_upload_headers(&batch(), None);
        assert_eq!(header(&h, "Swarm-Postage-Batch-Id"), Some("ab".repeat(32).as_str()));
        assert!(header(&h, "Swarm-Pin").is_none());
        assert!(header(&h, "Swarm-Encrypt").is_none());
    }

    #[test]
    fn upload_headers_distinguish_none_from_some_false() {
        let opts = UploadOptions {
            pin: Some(false),
            ..Default::default()
        };
        let h = prepare_upload_headers(&batch(), Some(&opts));
        assert_eq!(header(&h, "Swarm-Pin"), Some("false"));
    }

    #[test]
    fn redundancy_level_off_is_omitted() {
        let opts = RedundantUploadOptions {
            redundancy_level: Some(RedundancyLevel::Off),
            ..Default::default()
        };
        let h = prepare_redundant_upload_headers(&batch(), Some(&opts));
        assert!(header(&h, "Swarm-Redundancy-Level").is_none());
    }

    #[test]
    fn redundancy_level_medium_emits_header() {
        let opts = RedundantUploadOptions {
            redundancy_level: Some(RedundancyLevel::Medium),
            ..Default::default()
        };
        let h = prepare_redundant_upload_headers(&batch(), Some(&opts));
        assert_eq!(header(&h, "Swarm-Redundancy-Level"), Some("1"));
    }

    #[test]
    fn collection_upload_uses_swarm_index_document_header() {
        let opts = CollectionUploadOptions {
            index_document: Some("index.html".into()),
            ..Default::default()
        };
        let h = prepare_collection_upload_headers(&batch(), Some(&opts));
        assert_eq!(header(&h, "Swarm-Index-Document"), Some("index.html"));
    }

    #[test]
    fn download_act_implies_swarm_act_true() {
        let opts = DownloadOptions {
            act_history_address: Some(Reference::from_hex(&"00".repeat(32)).unwrap()),
            ..Default::default()
        };
        let h = prepare_download_headers(Some(&opts));
        assert_eq!(header(&h, "Swarm-Act"), Some("true"));
    }

    #[test]
    fn download_no_options_no_headers() {
        assert!(prepare_download_headers(None).is_empty());
        assert!(prepare_download_headers(Some(&DownloadOptions::default())).is_empty());
    }
}
