//! Rich return types for upload + download. Mirrors bee-go's
//! `pkg/api/result.go`.

use crate::swarm::{Error, Reference};

/// Standardized return shape for every upload endpoint. Mirrors
/// bee-js / bee-go `UploadResult`.
///
/// `tag_uid` is `None` when Bee did not return a `Swarm-Tag` header.
/// `history_address` is `None` when no ACT history was created.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UploadResult {
    /// Reference returned by Bee in the JSON body.
    pub reference: Reference,
    /// Tag UID parsed from the `Swarm-Tag` response header.
    pub tag_uid: Option<u32>,
    /// ACT history root parsed from `Swarm-Act-History-Address`.
    pub history_address: Option<Reference>,
}

impl UploadResult {
    /// Parse from the JSON-decoded reference hex plus the response
    /// headers. Mirrors bee-go `ReadUploadResult`.
    pub fn from_response(ref_hex: &str, headers: &reqwest::header::HeaderMap) -> Result<Self, Error> {
        let reference = Reference::from_hex(ref_hex)?;
        let tag_uid = header_str(headers, "swarm-tag")
            .and_then(|s| s.parse::<u32>().ok());
        let history_address = header_str(headers, "swarm-act-history-address")
            .and_then(|s| Reference::from_hex(s).ok());
        Ok(Self {
            reference,
            tag_uid,
            history_address,
        })
    }
}

/// Parsed file-download response headers. Mirrors bee-go
/// `FileHeaders`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileHeaders {
    /// Filename extracted from `Content-Disposition`.
    pub name: Option<String>,
    /// Tag UID from `Swarm-Tag-Uid`.
    pub tag_uid: Option<u32>,
    /// `Content-Type` value.
    pub content_type: Option<String>,
}

impl FileHeaders {
    /// Extract the file metadata Bee places on download responses.
    /// Missing or malformed headers fall through silently.
    pub fn from_response(headers: &reqwest::header::HeaderMap) -> Self {
        Self {
            content_type: header_str(headers, "content-type").map(str::to_owned),
            name: header_str(headers, "content-disposition")
                .and_then(parse_content_disposition_filename),
            tag_uid: header_str(headers, "swarm-tag-uid").and_then(|s| s.parse::<u32>().ok()),
        }
    }
}

fn header_str<'h>(headers: &'h reqwest::header::HeaderMap, name: &str) -> Option<&'h str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

/// Extract the filename from a `Content-Disposition` header. Handles
/// both `filename="…"` and `filename*=UTF-8''…` forms; returns
/// `None` if neither is present. Mirrors bee-js's permissive parser.
pub fn parse_content_disposition_filename(header: &str) -> Option<String> {
    for raw_part in header.split(';') {
        let part = raw_part.trim();
        let lower = part.to_ascii_lowercase();
        if !lower.starts_with("filename=") && !lower.starts_with("filename*=") {
            continue;
        }
        let eq = part.find('=')?;
        let mut value = &part[eq + 1..];
        // filename*=UTF-8''actual-name
        if let Some(idx) = value.find("''") {
            value = &value[idx + 2..];
        }
        let trimmed = if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
            &value[1..value.len() - 1]
        } else {
            value
        };
        return Some(trimmed.to_owned());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};

    fn map(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut m = HeaderMap::new();
        for (k, v) in pairs {
            m.insert(
                reqwest::header::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        m
    }

    #[test]
    fn upload_result_parses_reference_and_headers() {
        let ref_hex = "ab".repeat(32);
        let headers = map(&[
            ("Swarm-Tag", "42"),
            ("Swarm-Act-History-Address", &"cd".repeat(32)),
        ]);
        let r = UploadResult::from_response(&ref_hex, &headers).unwrap();
        assert_eq!(r.reference.to_hex(), ref_hex);
        assert_eq!(r.tag_uid, Some(42));
        assert_eq!(r.history_address.unwrap().to_hex(), "cd".repeat(32));
    }

    #[test]
    fn upload_result_handles_missing_headers() {
        let ref_hex = "00".repeat(32);
        let headers = map(&[]);
        let r = UploadResult::from_response(&ref_hex, &headers).unwrap();
        assert_eq!(r.tag_uid, None);
        assert_eq!(r.history_address, None);
    }

    #[test]
    fn file_headers_parse_quoted_filename() {
        let h = map(&[
            ("Content-Type", "text/plain"),
            ("Content-Disposition", "attachment; filename=\"hello.txt\""),
            ("Swarm-Tag-Uid", "7"),
        ]);
        let fh = FileHeaders::from_response(&h);
        assert_eq!(fh.content_type.as_deref(), Some("text/plain"));
        assert_eq!(fh.name.as_deref(), Some("hello.txt"));
        assert_eq!(fh.tag_uid, Some(7));
    }

    #[test]
    fn file_headers_parse_rfc5987_filename() {
        assert_eq!(
            parse_content_disposition_filename(
                "attachment; filename*=UTF-8''my%20file.txt"
            ),
            Some("my%20file.txt".to_string())
        );
    }

    #[test]
    fn file_headers_parse_unquoted_filename() {
        assert_eq!(
            parse_content_disposition_filename("attachment; filename=plain.txt"),
            Some("plain.txt".to_string())
        );
    }

    #[test]
    fn file_headers_no_filename_returns_none() {
        assert_eq!(
            parse_content_disposition_filename("attachment"),
            None
        );
    }
}
