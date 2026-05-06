//! Top-level [`Client`] and the shared [`Inner`] HTTP plumbing.
//!
//! Mirrors bee-go's `bee.Client`. A `Client` is cheaply cloneable
//! (`Arc<Inner>`) and yields per-domain handles via accessors:
//!
//! ```no_run
//! # use bee::Client;
//! # async fn run() -> Result<(), bee::Error> {
//! let client = Client::new("http://localhost:1633")?;
//! let health = client.debug().health().await?;
//! # Ok(()) }
//! ```

use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::{Method, RequestBuilder};
use serde::de::DeserializeOwned;
use url::Url;

use crate::api::HeaderPairs;
use crate::swarm::{Error, RESPONSE_BODY_CAP};

/// Shared HTTP/state used by every sub-service.
#[derive(Debug)]
pub(crate) struct Inner {
    pub(crate) base_url: Url,
    pub(crate) http: reqwest::Client,
}

impl Inner {
    /// Resolve a path against the base URL. The base URL is normalized
    /// to end with `/`, so `path` is treated as relative.
    pub(crate) fn url(&self, path: &str) -> Result<Url, Error> {
        self.base_url
            .join(path)
            .map_err(|e| Error::argument(format!("invalid url: {e}")))
    }

    /// Build a request, send it, and translate non-2xx responses into
    /// [`Error::Response`] with method / URL / capped body captured.
    ///
    /// Emits `tracing::debug!` events at target `bee::http` carrying
    /// `method`, `url`, `status`, and `elapsed_ms` for every request.
    /// Subscribe with `RUST_LOG=bee::http=debug` (or any subscriber
    /// that captures spans/events) to surface live API traffic — the
    /// bee-tui command-log pane uses this.
    pub(crate) async fn send(&self, builder: RequestBuilder) -> Result<reqwest::Response, Error> {
        let request = builder.build()?;
        let method = request.method().to_string();
        let url = request.url().to_string();
        let start = Instant::now();

        let resp = self.http.execute(request).await?;
        let elapsed_ms = start.elapsed().as_millis() as u64;
        let status = resp.status().as_u16();

        if resp.status().is_success() {
            tracing::debug!(
                target: "bee::http",
                method = %method,
                url = %url,
                status,
                elapsed_ms,
                "bee api request"
            );
            return Ok(resp);
        }
        let status_text = format!(
            "{status} {}",
            resp.status().canonical_reason().unwrap_or("")
        )
        .trim_end()
        .to_string();
        let body = resp.bytes().await.map(|b| b.to_vec()).unwrap_or_default();
        let n = body.len().min(RESPONSE_BODY_CAP);
        tracing::debug!(
            target: "bee::http",
            method = %method,
            url = %url,
            status,
            elapsed_ms,
            body_len = body.len(),
            "bee api error response"
        );
        Err(Error::Response {
            method,
            url,
            status,
            status_text,
            body: body[..n].to_vec(),
        })
    }

    /// Send and parse the response body as JSON.
    pub(crate) async fn send_json<T: DeserializeOwned>(
        &self,
        builder: RequestBuilder,
    ) -> Result<T, Error> {
        let resp = self.send(builder).await?;
        let bytes = resp.bytes().await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Apply a list of header pairs to a request builder.
    pub(crate) fn apply_headers(builder: RequestBuilder, headers: HeaderPairs) -> RequestBuilder {
        let mut b = builder;
        for (name, value) in headers {
            b = b.header(name, value);
        }
        b
    }
}

/// Top-level Bee API client.
#[derive(Clone, Debug)]
pub struct Client {
    pub(crate) inner: Arc<Inner>,
}

impl Client {
    /// Construct a client from a base URL (e.g. `"http://localhost:1633"`).
    /// A trailing slash is appended if missing so relative paths resolve
    /// correctly.
    pub fn new(url: &str) -> Result<Self, Error> {
        let mut owned = url.to_owned();
        if !owned.ends_with('/') {
            owned.push('/');
        }
        let base_url =
            Url::parse(&owned).map_err(|e| Error::argument(format!("invalid url: {e}")))?;
        let http = reqwest::Client::builder()
            .build()
            .map_err(Error::Transport)?;
        Ok(Self {
            inner: Arc::new(Inner { base_url, http }),
        })
    }

    /// Construct a client with a caller-provided [`reqwest::Client`].
    /// Use this to share a connection pool with other code or to set
    /// custom timeouts / TLS roots.
    pub fn with_http_client(url: &str, http: reqwest::Client) -> Result<Self, Error> {
        let mut owned = url.to_owned();
        if !owned.ends_with('/') {
            owned.push('/');
        }
        let base_url =
            Url::parse(&owned).map_err(|e| Error::argument(format!("invalid url: {e}")))?;
        Ok(Self {
            inner: Arc::new(Inner { base_url, http }),
        })
    }

    /// Construct a client that sends `Authorization: Bearer <token>`
    /// on every request. Convenience for talking to a Bee node running
    /// with restricted-mode auth.
    ///
    /// For more control (custom timeouts, TLS roots, additional
    /// headers), build a [`reqwest::Client`] yourself and pass it via
    /// [`Client::with_http_client`].
    pub fn with_token(url: &str, token: &str) -> Result<Self, Error> {
        use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
        let value = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|e| Error::argument(format!("invalid token: {e}")))?;
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, value);
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .map_err(Error::Transport)?;
        Self::with_http_client(url, http)
    }

    /// Borrow the configured base URL.
    pub fn base_url(&self) -> &Url {
        &self.inner.base_url
    }

    /// `GET /health` round-trip latency. Useful for connection-status
    /// indicators in dashboards and TUIs. Returns the elapsed
    /// [`Duration`] regardless of body — the response is not parsed.
    pub async fn ping(&self) -> Result<Duration, Error> {
        let url = self.inner.url("health")?;
        let builder = self.inner.http.request(Method::GET, url);
        let start = Instant::now();
        let _ = self.inner.send(builder).await?;
        Ok(start.elapsed())
    }

    /// Sub-service: file / data / chunk / SOC / feed / collection
    /// uploads and downloads.
    pub fn file(&self) -> crate::file::FileApi {
        crate::file::FileApi::new(self.inner.clone())
    }

    /// Sub-service: postage batch CRUD + stamp metadata. Stamp math
    /// helpers live as free functions in [`crate::postage`].
    pub fn postage(&self) -> crate::postage::PostageApi {
        crate::postage::PostageApi::new(self.inner.clone())
    }

    /// Sub-service: debug / operator endpoints (health, versions,
    /// peers, accounting, chequebook, stake).
    pub fn debug(&self) -> crate::debug::DebugApi {
        crate::debug::DebugApi::new(self.inner.clone())
    }

    /// Sub-service: generic `/api/*` endpoints (pin, tag, stewardship,
    /// grantee, envelope).
    pub fn api(&self) -> crate::api::ApiService {
        crate::api::ApiService::new(self.inner.clone())
    }

    /// Sub-service: PSS send + websocket subscribe / receive.
    pub fn pss(&self) -> crate::pss::PssApi {
        crate::pss::PssApi::new(self.inner.clone())
    }

    /// Sub-service: GSOC send + websocket subscribe.
    pub fn gsoc(&self) -> crate::gsoc::GsocApi {
        crate::gsoc::GsocApi::new(self.inner.clone())
    }
}

/// Shorthand: build a `RequestBuilder` for `(method, path)` against
/// the inner HTTP client.
pub(crate) fn request(inner: &Inner, method: Method, path: &str) -> Result<RequestBuilder, Error> {
    let url = inner.url(path)?;
    Ok(inner.http.request(method, url))
}
