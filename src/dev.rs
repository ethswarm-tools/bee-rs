//! Bee dev-mode wrapper. The dev-mode endpoint surface is a strict
//! subset of production Bee, so this is mostly a documentation /
//! discovery signal: callers can `let bee = DevClient::new(url)?;`
//! and use the same accessors they already know from [`Client`].
//!
//! Mirrors bee-js `BeeDev`. The endpoints currently exposed
//! (`addresses`, `topology`) accept a slimmer payload in dev mode but
//! the same accessors work; bee-rs avoids splitting the type system
//! here and reuses the regular debug handles.

use crate::Client;
use crate::swarm::Error;

/// Thin newtype around [`Client`] for use against Bee in dev mode.
/// Cheap to clone.
#[derive(Clone, Debug)]
pub struct DevClient {
    inner: Client,
}

impl DevClient {
    /// Construct a dev-mode client from a base URL. Same shape as
    /// [`Client::new`].
    pub fn new(url: &str) -> Result<Self, Error> {
        Ok(Self {
            inner: Client::new(url)?,
        })
    }

    /// Construct a dev-mode client with a caller-provided HTTP client.
    pub fn with_http_client(url: &str, http: reqwest::Client) -> Result<Self, Error> {
        Ok(Self {
            inner: Client::with_http_client(url, http)?,
        })
    }

    /// Borrow the wrapped [`Client`] for full API access.
    pub fn client(&self) -> &Client {
        &self.inner
    }

    /// Convert into the wrapped [`Client`].
    pub fn into_client(self) -> Client {
        self.inner
    }
}

impl std::ops::Deref for DevClient {
    type Target = Client;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
