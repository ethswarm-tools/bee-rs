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
    pub(crate) async fn send(&self, builder: RequestBuilder) -> Result<reqwest::Response, Error> {
        let request = builder.build()?;
        let method = request.method().to_string();
        let url = request.url().to_string();
        let resp = self.http.execute(request).await?;
        if resp.status().is_success() {
            return Ok(resp);
        }
        let status = resp.status().as_u16();
        let status_text = format!(
            "{status} {}",
            resp.status().canonical_reason().unwrap_or("")
        )
        .trim_end()
        .to_string();
        let body = resp.bytes().await.map(|b| b.to_vec()).unwrap_or_default();
        let n = body.len().min(RESPONSE_BODY_CAP);
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

    /// Borrow the configured base URL.
    pub fn base_url(&self) -> &Url {
        &self.inner.base_url
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
