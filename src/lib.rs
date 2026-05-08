//! bee-rs — Rust client for the Swarm Bee API.
//!
//! Functional parity target with [bee-js] (the canonical TypeScript
//! client) and [bee-go] (the typed Go port). bee-go is the primary
//! reference for shape and behavior; bee-js is the source of truth for
//! wire-format edge cases.
//!
//! # Quick start
//!
//! Connect to a local Bee node, buy a postage batch, upload a few
//! bytes, and download them back:
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use bee::{Client, storage::StorageOptions};
//! use bee::swarm::Size;
//! use bytes::Bytes;
//!
//! # async fn run() -> Result<(), bee::Error> {
//! let client = Client::new("http://localhost:1633")?;
//!
//! // Health check the node before we do anything else.
//! let health = client.debug().health().await?;
//! assert_eq!(health.status, "ok");
//!
//! // Buy 1 GB of storage for 30 days at the current chain price.
//! let size = Size::from_gigabytes(1.0)?;
//! let duration = Duration::from_secs(30 * 24 * 60 * 60);
//! let batch_id = bee::storage::buy_storage(
//!     &client, size, duration, &StorageOptions::default(),
//! ).await?;
//!
//! // Upload + download round-trip.
//! let result = client
//!     .file()
//!     .upload_data(&batch_id, Bytes::from_static(b"Hello Swarm!"), None)
//!     .await?;
//! let body = client.file().download_data(&result.reference, None).await?;
//! assert_eq!(&body[..], b"Hello Swarm!");
//! # Ok(()) }
//! ```
//!
//! # Module map
//!
//! bee-rs is a sub-service client: the top-level [`Client`] yields one
//! handle per Bee API domain. Sub-services are cheap to construct
//! (`Arc<Inner>` + accessor) so it is fine to call them on every
//! request.
//!
//! - [`api`] — generic `/api/*` endpoints (pin, tag, stewardship,
//!   grantee, envelope) plus the upload / download option structs
//!   and rich return types.
//! - [`mod@file`] — bytes, files, chunks, SOC, feeds, and tar-packed
//!   collections; offline `hash_directory` / `hash_collection_entries`;
//!   chunk-by-chunk `stream_directory` with progress callback.
//! - [`postage`] — postage batch CRUD plus pure stamp math
//!   (`get_stamp_cost`, `get_amount_for_duration`,
//!   `get_depth_for_size`, …) and an offline `Stamper`.
//! - [`debug`] — operator endpoints: health, versions, peers,
//!   accounting, chequebook, stake, transactions.
//! - [`pss`] — Postal Service: send + websocket subscribe / receive.
//! - [`gsoc`] — Generic Single-Owner Chunk send / subscribe.
//! - [`manifest`] — Mantaray trie, `ResourceLocator`, offline
//!   `resolve_path`.
//! - [`swarm`] — typed bytes, BMT chunk addressing, SOC primitives,
//!   token math, [`BeeDuration`](swarm::BeeDuration),
//!   [`Size`](swarm::Size), and the crate-level [`Error`] enum.
//! - [`storage`] — high-level `(size, duration)` helpers built on top
//!   of [`Client`] and [`postage`].
//! - [`dev`] — [`DevClient`] wrapper for talking to `bee dev`.
//!
//! # Bee version compatibility
//!
//! This client targets Bee `2.7.2-rc1` / API `8.0.0` (the values pinned
//! in [`debug::SUPPORTED_BEE_VERSION_EXACT`] and
//! [`debug::SUPPORTED_API_VERSION`]). At startup, prefer
//! [`debug::DebugApi::is_supported_api_version`] for a major-compatible
//! check or [`debug::DebugApi::is_supported_exact_version`] for a
//! strict pin. Older or newer Bee instances usually work — unknown
//! response fields are ignored — but new endpoints will return 404 and
//! breaking wire-format changes will surface as [`Error::Json`].
//!
//! # Authentication, timeouts, and proxies
//!
//! [`Client::new`] constructs a default [`reqwest::Client`] with no
//! auth, no request timeout, and no custom TLS roots. For anything
//! beyond a trusted local node, build a [`reqwest::Client`] with the
//! settings you need and pass it via [`Client::with_http_client`]:
//!
//! ```no_run
//! use std::time::Duration;
//! use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
//!
//! # fn run() -> Result<(), bee::Error> {
//! let mut headers = HeaderMap::new();
//! headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer …"));
//! let http = reqwest::Client::builder()
//!     .timeout(Duration::from_secs(30))
//!     .default_headers(headers)
//!     .build()
//!     .map_err(bee::Error::Transport)?;
//! let client = bee::Client::with_http_client("https://bee.example.com", http)?;
//! # let _ = client; Ok(()) }
//! ```
//!
//! Bee gates `/stamps`, `/chequebook`, `/stake`, `/transactions`, and
//! the operator endpoints behind tokens in production. Proxies are
//! honored from the `HTTP_PROXY` / `HTTPS_PROXY` env vars unless you
//! call `.no_proxy()` on the builder.
//!
//! No automatic retries are performed. Use a layered HTTP client (e.g.
//! [`reqwest-middleware`] + [`reqwest-retry`]) if you need them.
//!
//! [`reqwest-middleware`]: https://docs.rs/reqwest-middleware
//! [`reqwest-retry`]: https://docs.rs/reqwest-retry
//!
//! # Concurrency
//!
//! [`Client`] is `Send + Sync + Clone`. Cloning is `Arc`-cheap
//! (`Arc<Inner>` under the hood), so pass a clone into spawned tasks
//! rather than wrapping in `Arc<Mutex<…>>`. Sub-service handles
//! ([`file::FileApi`], [`postage::PostageApi`], …) are likewise cheap
//! and share the same underlying HTTP client + connection pool.
//!
//! # Cancellation
//!
//! Dropping the future returned by any endpoint cancels the in-flight
//! HTTP request. For uploads, chunks already accepted by the local
//! Bee node may remain in the local reserve but the upload is not
//! committed (no manifest reference is returned).
//! [`file::FileApi::stream_directory`] and
//! [`file::FileApi::stream_collection_entries`] upload chunk-by-chunk
//! and can leave orphan chunks if cancelled mid-stream.
//!
//! # Streaming vs. buffered transfers
//!
//! [`file::FileApi::download_data`] / [`file::FileApi::download_file`]
//! buffer the full body into [`bytes::Bytes`] before returning — fine
//! for ≤ a few hundred MB, but will OOM on multi-GB references. For
//! larger downloads, use [`file::FileApi::download_data_response`] (or
//! `download_file_response`) which returns the raw [`reqwest::Response`]
//! so you can drive [`reqwest::Response::bytes_stream`] yourself.
//!
//! Uploads accept `impl Into<Bytes>` and stream the body to Bee. The
//! chunk-by-chunk variants ([`file::FileApi::stream_directory`] and
//! `stream_collection_entries`) bound peak memory at the BMT chunk
//! size regardless of file size and report progress via a
//! caller-supplied [`file::OnStreamProgressFn`].
//!
//! # Errors
//!
//! Every fallible call returns [`Result<T, Error>`](Result). [`Error`]
//! is a single enum so callers can match on the variant of interest:
//!
//! ```no_run
//! # async fn run(client: bee::Client) -> Result<(), bee::Error> {
//! match client.debug().health().await {
//!     Ok(h) if h.status == "ok" => println!("ready: bee {}", h.version),
//!     Ok(h) => println!("not healthy: status={}", h.status),
//!     Err(bee::Error::Response { status, .. }) => println!("bee returned {status}"),
//!     Err(e) => return Err(e),
//! }
//! # Ok(()) }
//! ```
//!
//! As a rule of thumb: 5xx responses and [`Error::Transport`] errors
//! (DNS, connection refused, EOF) are retry candidates with backoff;
//! 4xx [`Error::Response`] errors (invalid batch ID, depth out of
//! range, immutable-flag mismatch) are caller bugs and re-issuing the
//! same request will fail the same way. `POST /bytes` is idempotent
//! for the same `(data, batch_id)` tuple — Bee returns the same
//! content reference.
//!
//! # Observability
//!
//! bee-rs depends on [`tracing`] but does not currently emit spans or
//! events. Wrap [`reqwest::Client`] with [`reqwest-tracing`] (or your
//! own middleware) before passing it to [`Client::with_http_client`]
//! to get request-level spans. Bee's own
//! [`debug::DebugApi::set_logger_verbosity`] controls server-side
//! verbosity at runtime.
//!
//! [`tracing`]: https://docs.rs/tracing
//! [`reqwest-tracing`]: https://docs.rs/reqwest-tracing
//!
//! # Cargo features
//!
//! `default = []` — there are no opt-in features today. The crate
//! always builds with `reqwest`'s `rustls-tls` backend; native-TLS
//! (OS trust store) is not currently exposed. Open an issue if you
//! need it for a corporate-CA deployment.
//!
//! # MSRV
//!
//! Minimum supported Rust version is **1.85** (Rust 2024 edition).
//! See [`Cargo.toml`] `rust-version`.
//!
//! [`Cargo.toml`]: https://github.com/ethswarm-tools/bee-rs/blob/main/Cargo.toml
//!
//! # Testing
//!
//! Point [`Client::new`] at a [`wiremock`] mock server (a dev-dep of
//! this crate, used in our own tests) to test code that calls bee-rs
//! without a real Bee:
//!
//! [`wiremock`]: https://docs.rs/wiremock
//!
//! ```no_run
//! # async fn run() -> Result<(), bee::Error> {
//! # use wiremock::{MockServer, Mock, ResponseTemplate};
//! # use wiremock::matchers::method;
//! let server = MockServer::start().await;
//! Mock::given(method("GET"))
//!     .respond_with(ResponseTemplate::new(200)
//!         .set_body_string(r#"{"status":"ok","version":"2.7.2","apiVersion":"8.0.0"}"#))
//!     .mount(&server)
//!     .await;
//! let client = bee::Client::new(&server.uri())?;
//! let h = client.debug().health().await?;
//! assert_eq!(h.status, "ok");
//! # Ok(()) }
//! ```
//!
//! # Common pitfalls
//!
//! - A freshly-purchased postage batch is not usable for ~2-3 minutes
//!   on Sepolia (block time × N confirmations). Poll
//!   [`postage::PostageBatch::usable`] before uploading or you will
//!   get HTTP 422 "stamp not usable".
//! - Dilute is one-way: depth can only grow, never shrink.
//! - [`swarm::Reference`] values are 32 bytes (plain) or 64 bytes
//!   (encrypted). Passing an encrypted reference to a plain download
//!   silently returns garbage — match the reference type to how the
//!   data was uploaded.
//! - Feed updates require the same `(topic, signer)` pair every time;
//!   a new signer creates a new feed, not an update.
//! - On a `bee dev` node, all chain / chequebook / stake endpoints
//!   return 404 — see [`DevClient`].
//!
//! [bee-js]: https://github.com/ethersphere/bee-js
//! [bee-go]: https://github.com/ethswarm-tools/bee-go

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod api;
pub(crate) mod client;
pub mod debug;
pub mod dev;
pub mod file;
pub mod gsoc;
pub mod manifest;
pub mod postage;
pub mod pss;
pub mod storage;
pub mod swarm;

pub use dev::DevClient;

pub use client::{Client, MAX_JSON_RESPONSE_BYTES};
pub use swarm::{Error, Result};
