//! Go `pprof` runtime-profile endpoints under `/debug/pprof/*`.
//!
//! These are exposed by Bee only when the node is started with
//! `--debug-api-enable=true`. They return raw Go `pprof` binary blobs
//! (not JSON) suitable for feeding straight into `go tool pprof`. The
//! CPU `profile` and `trace` endpoints block for `seconds` while they
//! sample; the others (`heap`, `goroutine`, `allocs`) are instantaneous.
//!
//! Mirrors the one capability from the bee-scripts `pprof.sh` helper
//! that no client previously exposed.

use bytes::Bytes;
use reqwest::Method;

use crate::client::request;
use crate::swarm::Error;

use super::DebugApi;

impl DebugApi {
    /// `GET /debug/pprof/{name}` — fetch a Go runtime profile as a raw
    /// binary blob. Pass `seconds` for the sampling endpoints
    /// (`profile`, `trace`); leave it `None` for the instantaneous ones
    /// (`heap`, `goroutine`, `allocs`).
    ///
    /// The body is returned verbatim ([`Bytes`]); it is *not* JSON, so
    /// feed it directly to `go tool pprof`. A 404 means the node was not
    /// started with `--debug-api-enable=true`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use bee::Client;
    ///
    /// # async fn run() -> Result<(), bee::Error> {
    /// let client = Client::new("http://localhost:1633")?;
    /// let heap = client.debug().pprof("heap", None).await?;
    /// std::fs::write("heap.pprof", &heap)?;
    /// # Ok(()) }
    /// ```
    pub async fn pprof(&self, name: &str, seconds: Option<u64>) -> Result<Bytes, Error> {
        let path = format!("debug/pprof/{name}");
        let mut builder = request(&self.inner, Method::GET, &path)?;
        if let Some(s) = seconds {
            builder = builder.query(&[("seconds", s.to_string())]);
        }
        let resp = self.inner.send(builder).await?;
        Ok(resp.bytes().await?)
    }

    /// `GET /debug/pprof/profile?seconds=N` — CPU profile. Blocks for
    /// `seconds` while Bee samples, then returns the raw `pprof` blob.
    pub async fn pprof_profile(&self, seconds: u64) -> Result<Bytes, Error> {
        self.pprof("profile", Some(seconds)).await
    }

    /// `GET /debug/pprof/trace?seconds=N` — execution trace. Blocks for
    /// `seconds`, then returns the raw trace blob.
    pub async fn pprof_trace(&self, seconds: u64) -> Result<Bytes, Error> {
        self.pprof("trace", Some(seconds)).await
    }

    /// `GET /debug/pprof/heap` — instantaneous heap profile.
    pub async fn pprof_heap(&self) -> Result<Bytes, Error> {
        self.pprof("heap", None).await
    }

    /// `GET /debug/pprof/goroutine` — instantaneous goroutine dump.
    pub async fn pprof_goroutine(&self) -> Result<Bytes, Error> {
        self.pprof("goroutine", None).await
    }

    /// `GET /debug/pprof/allocs` — instantaneous allocation profile.
    pub async fn pprof_allocs(&self) -> Result<Bytes, Error> {
        self.pprof("allocs", None).await
    }
}
