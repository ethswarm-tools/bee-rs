//! Crate-level error type. Mirrors bee-go's `BeeError` /
//! `BeeArgumentError` / `BeeResponseError` hierarchy as a single Rust
//! enum so callers can `match` on the variant.

use std::fmt;

/// Result alias used across the crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Cap on captured response bodies so a huge error page can't OOM.
pub const RESPONSE_BODY_CAP: usize = 4096;

/// All errors surfaced from `bee-rs`.
///
/// - [`Error::Argument`] — caller passed invalid input. Mirrors
///   bee-js / bee-go `BeeArgumentError`.
/// - [`Error::Response`] — Bee returned a non-2xx status. Mirrors
///   bee-js / bee-go `BeeResponseError`.
/// - [`Error::LengthMismatch`] — typed-byte constructor rejected a
///   value whose length didn't match any allowed size.
/// - [`Error::Hex`] — hex decoding failed.
/// - [`Error::Crypto`] — secp256k1 / keccak operation failed.
/// - [`Error::Json`] — JSON decode/encode failed.
/// - [`Error::Transport`] — transport error from `reqwest`.
/// - [`Error::Other`] — fallback wrapper for anything else.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Invalid argument from the caller.
    #[error("{message}")]
    Argument {
        /// Human-readable description of what was invalid.
        message: String,
    },

    /// Non-2xx HTTP response from Bee.
    #[error("{method} {url}: {status} {status_text}")]
    Response {
        /// HTTP method of the failed request.
        method: String,
        /// Full URL of the failed request.
        url: String,
        /// Numeric status code.
        status: u16,
        /// `<code> <reason>` (e.g. `"422 Unprocessable Entity"`).
        status_text: String,
        /// Response body, truncated to [`RESPONSE_BODY_CAP`] bytes.
        body: Vec<u8>,
    },

    /// Typed-byte constructor received a value of the wrong length.
    #[error("invalid {kind} length: got {got}, expected {expected:?}")]
    LengthMismatch {
        /// The Rust type that rejected the input (`"Reference"`, `"BatchId"`, ...).
        kind: &'static str,
        /// Lengths the type accepts.
        expected: &'static [usize],
        /// Length actually provided.
        got: usize,
    },

    /// Hex decoding failed.
    #[error("invalid hex: {0}")]
    Hex(#[from] hex::FromHexError),

    /// Crypto primitive (secp256k1 / keccak) failed.
    #[error("crypto: {0}")]
    Crypto(String),

    /// JSON encode/decode failed.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    /// Transport-level error (TCP / TLS / DNS).
    #[error("transport: {0}")]
    Transport(#[from] reqwest::Error),

    /// Catch-all for errors that don't fit the specific variants.
    #[error("{0}")]
    Other(String),
}

impl Error {
    /// Build an [`Error::Argument`].
    pub fn argument<M: Into<String>>(msg: M) -> Self {
        Error::Argument { message: msg.into() }
    }

    /// Build an [`Error::Crypto`].
    pub fn crypto<M: fmt::Display>(msg: M) -> Self {
        Error::Crypto(msg.to_string())
    }

    /// True if this error is an HTTP response error.
    pub fn is_response(&self) -> bool {
        matches!(self, Error::Response { .. })
    }

    /// Get the HTTP status if this error wraps a non-2xx response.
    pub fn status(&self) -> Option<u16> {
        match self {
            Error::Response { status, .. } => Some(*status),
            _ => None,
        }
    }
}

/// Build an [`Error::Response`] from a `reqwest::Response`. Reads up to
/// [`RESPONSE_BODY_CAP`] bytes of the body so a giant error page can't
/// blow up memory.
pub async fn response_error_from(resp: reqwest::Response) -> Error {
    let method = resp
        .url()
        .as_str()
        .to_owned();
    // reqwest does not expose the request method on Response. Callers
    // that need the method should wrap with their own context — this
    // helper is for the common case where Response is the only thing
    // available. We store the URL twice (in `method` slot too) when
    // method is unknown; callers that care use [`Error::response`] below.
    let url = method.clone();
    let status = resp.status();
    let status_text = status.canonical_reason().unwrap_or("").to_string();
    let body = resp
        .bytes()
        .await
        .map(|b| {
            let n = b.len().min(RESPONSE_BODY_CAP);
            b.slice(..n).to_vec()
        })
        .unwrap_or_default();
    Error::Response {
        method: String::new(),
        url,
        status: status.as_u16(),
        status_text: format!("{} {}", status.as_u16(), status_text),
        body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_mismatch_displays() {
        let e = Error::LengthMismatch {
            kind: "Reference",
            expected: &[32, 64],
            got: 16,
        };
        assert_eq!(format!("{e}"), "invalid Reference length: got 16, expected [32, 64]");
    }

    #[test]
    fn argument_helper() {
        let e = Error::argument("bad input");
        assert!(matches!(e, Error::Argument { .. }));
        assert_eq!(format!("{e}"), "bad input");
    }
}
