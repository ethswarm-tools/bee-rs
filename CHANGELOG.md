# Changelog

All notable changes to bee-rs will be documented in this file. The
format follows [Keep a Changelog]; the project adheres to
[Semantic Versioning].

[Keep a Changelog]: https://keepachangelog.com/en/1.1.0/
[Semantic Versioning]: https://semver.org/spec/v2.0.0.html

## [Unreleased]

## [1.4.0] - 2026-05-07

### Added

- **Extended `Topology` parse.** `GET /topology` now exposes the full
  bee-go shape: 32 per-bin [`BinInfo`] entries (population, connected,
  connected/disconnected peer lists with per-peer metrics), the
  `reachability` and `networkAvailability` strings, and a separate
  `light_nodes` bin. Backwards-compatible — older Bee builds and
  stripped-down dev mocks parse cleanly with empty defaults.
- **`PeerInfo`** with optional [`MetricSnapshotView`] (lastSeen,
  latency EWMA, session direction, healthy flag, per-peer
  reachability) for the entries inside each bin.
- Bins are exposed as a `Vec<BinInfo>` of length 32, indexable by
  bin number — the flat `bin_0`..`bin_31` JSON keys are folded into
  one indexable container by a custom deserializer. Empty
  `connectedPeers` / `disconnectedPeers` are accepted as either `[]`
  or JSON `null` (Go's default for nil slices), so the parse is
  robust across Bee versions.

### Notes

- This release is a strict additive extension; no existing fields or
  method signatures changed. Consumers on `1.3` can upgrade to `1.4`
  without code changes.

## [1.3.0] - 2026-05-06

### Added

- **`Client::ping`.** Returns the round-trip [`Duration`] of a `GET
  /health` against the configured node. Useful for connection-status
  indicators in dashboards and TUIs.
- **`Client::with_token`.** Convenience constructor that builds a
  client which sends `Authorization: Bearer <token>` on every request.
  Equivalent to building a `reqwest::Client` with `default_headers` by
  hand and passing it to [`Client::with_http_client`], but one line.
- **Tracing instrumentation on the HTTP send path.** Every request
  now emits a `tracing::debug!` event at target `bee::http` carrying
  `method`, `url`, `status`, and `elapsed_ms`. Subscribe with
  `RUST_LOG=bee::http=debug` (or any custom subscriber) to surface
  live API traffic — designed for the bee-tui command-log pane.
- **`DebugApi::time_settlements`.** `GET /timesettlements` — the
  pseudo-settle / refresh-rate counterpart to `settlements()`. Same
  `Settlements` schema, distinguishes time-based settlements from
  cheque-based settlements per peer.
- **`DebugApi::r_chash`.** `GET /rchash/{depth}/{anchor1}/{anchor2}`
  — reserve-commitment hash with sample inclusion proofs. Returned
  `RCHashResponse.duration_seconds` is the natural sampler benchmark
  (does the node hardware finish a sample inside the round deadline?).
  New types: `RCHashResponse`, `ChunkInclusionProofs`,
  `ChunkInclusionProof`, `PostageProof`, `SocProof` — re-exported
  from `bee::debug`.
- **`FileApi::chunks_stream`.** `WS /chunks/stream` — websocket-driven
  chunk upload session. Each [`ChunkStream::send_chunk`] sends one
  binary frame and awaits the server's single-byte ack; the session
  remains open until [`ChunkStream::close`] (or until dropped). Maps
  to bee-go's chunks-stream handler. Pairs with `swarm-tag` query for
  buffered uploads. Foundation for live upload trackers in dashboards
  (notably bee-tui) without per-chunk HTTP round-trip overhead.

## [1.2.0] - 2026-05-04

### Changed

- **ECDSA backend swapped from `k256` (pure Rust) to `secp256k1` (the
  libsecp256k1 C bindings used by Bitcoin Core, alloy, ethers, reth).**
  ~3.6× faster signing on the eth-signed-message scheme: `PrivateKey::sign`
  on a 32-byte message dropped from ~118 µs to ~33 µs in our benches.
  Public API is unchanged — k256 was never exposed through any
  `pub` type — so this is non-breaking. Adds a C build dependency
  (`secp256k1-sys`).

### Added

- **`benches/hashing.rs`.** Criterion harness covering `keccak256`
  (`sha3` vs `tiny-keccak`), the BMT chunk-address pipeline, and
  ECDSA sign throughput. Used to validate the ECDSA backend swap;
  also surfaced that `sha3` is *not* a bottleneck (it ties with
  `tiny-keccak` to within noise), so the keccak crate is unchanged.

## [1.1.1] - 2026-05-04

### Security

- **`PrivateKey` now scrubs its bytes on drop.** Derives `Zeroize` and
  `ZeroizeOnDrop` (via the `zeroize` crate), so the 32 secret bytes
  are overwritten with zeros when the value is dropped — mitigating
  exposure through heap reuse, panics, and core dumps. `PartialEq`
  is now constant-time via `subtle::ConstantTimeEq`. Public API is
  unchanged. New deps: `zeroize` (with `zeroize_derive`), `subtle`.

## [1.1.0] - 2026-05-02

### Added

- **Crate-level docs.rs landing page.** Expanded the `//!` block in
  `src/lib.rs` with a Quick Start section (runnable `no_run`
  doctest covering connect → health → buy_storage → upload →
  download), a module map, and an error-handling example. The page
  previously opened with three lines of mirrors-to-other-clients;
  it now opens with a complete onboarding read.
- **`# Examples` doctests on key methods.** Added runnable `no_run`
  examples to `FileApi::upload_data`, `FileApi::download_data`, and
  `PostageApi::create_postage_batch`. Doctests are exercised by
  `cargo test --doc`, so they cannot drift out of sync with the
  surface they document.
- **`examples/quickstart.rs`.** Minimal end-to-end example (connect,
  health, buy or reuse a batch via `BEE_BATCH_ID`, upload + download
  round-trip). Complements `examples/integration-check` (the full
  soak) with a fast onboarding path.
- **Operational sections in `lib.rs` `//!` block.** Bee version
  compatibility (pinned to 2.7.2-rc1 / API 8.0.0), authentication +
  timeouts + proxies (with `Client::with_http_client` snippet using
  `default_headers` for bearer tokens), concurrency notes (`Send +
  Sync + Clone`, `Arc`-cheap cloning), cancellation semantics,
  streaming vs. buffered transfers (call out `download_data` buffers
  fully and point at `download_data_response` for streaming),
  observability (`tracing` is a dep but no spans emitted today),
  Cargo features (none today; rustls-only), MSRV (1.85), testing
  (with `wiremock` example), common pitfalls (batch usability, dilute
  one-way, encrypted-vs-plain references, feed signer pairing,
  dev-mode 404s), and an errors-and-retryability paragraph.
- **Postage usability + dilute-one-way notes** in
  `src/postage/mod.rs`: ~2-3 minute Sepolia confirmation delay before
  `PostageBatch::usable` flips, and `dilute_batch` one-way semantics.
- **File streaming + cancellation notes** in `src/file/mod.rs`: call
  out that `download_data` / `download_file` buffer the full body and
  point at the `_response` variants for streaming; document
  `stream_directory` / `stream_collection_entries` orphan-chunk
  behavior on future drop.
- **Dev-mode 404 list** in `src/dev.rs`: explicit list of every
  endpoint that returns 404 against `bee dev` (chequebook,
  settlements, stake, pending transactions, chain-state reads,
  accounting, balances, RC hash, and the high-level
  `crate::storage::*` helpers that internally call them).

## [1.0.1] - 2026-05-01

### Fixed
- `Cargo.toml` `keywords`: replaced `"decentralized-storage"` (21
  chars) with `"storage"` so the package metadata satisfies
  crates.io's 20-char keyword cap. Metadata-only — no code changes
  vs. v1.0.0.

## [1.0.0] - 2026-05-01

### Added
- **P0 foundations.** Typed bytes (`BatchId`, `Reference`, `Signature`,
  `Identifier`, `Topic`, `EthAddress`, `PublicKey`, `Span`, …),
  BMT chunk addressing, file chunker, SOC primitives, GSOC mining,
  Mantaray manifest (v0.2 wire format), keys (secp256k1 with
  eth-signed-message scheme), errors, options + rich return types.
- **P1 API surface.** Async-first `Client` + `Inner` HTTP plumbing
  built on `reqwest`. Sub-services for `file` (data, chunks, SOC,
  feeds, bzz file/collection upload + download, filesystem-walked
  `upload_collection(dir)`, offline `hash_collection_entries` /
  `hash_directory`), `postage` (batch CRUD + pure stamp math),
  `debug` (health, status, peers, accounting, transactions, stake,
  wallet, chequebook, settlements, loggers), `api` (pin / tag /
  stewardship / grantee / envelope), `pss` (HTTP send + websocket
  subscribe / receive on `tokio_tungstenite`), `gsoc` (send +
  subscribe). `Network` + `Size` types, `crate::storage` helpers
  (`get_storage_cost`, `buy_storage`, extension-cost previews),
  `crate::dev::DevClient` wrapper.
- **P2 utilities.** `swarm::tokens::{Bzz, Dai}` fixed-point token
  math (PLUR / wei base units, full arithmetic + comparison + decimal
  rendering + exchange rate). `swarm::duration::Duration` with
  parser for `"1d 4h 5m 30s"`-style strings. `manifest::ResourceLocator`
  (`Reference` or ENS name) and offline `manifest::resolve_path` for
  `GET /bzz/{ref}/{path}`-style lookups against an unmarshaled
  Mantaray.
- **P3 release infra.** `.github/workflows/ci.yml` (fmt / clippy /
  test on stable + MSRV 1.85 / doc), `rustfmt.toml`, `clippy.toml`,
  this changelog, `RELEASE.md`, and an `examples/integration-check`
  smoke test against a live Bee node.
- **P5 parity gaps.** Closed the remaining gaps surfaced by the
  bee-js / bee-go audit:
  - `postage::Stamper` — client-side postage stamper. Per-bucket
    counters, `from_blank` / `from_state` constructors, signed
    `Envelope` per chunk. Mirrors bee-js `Stamper` and bee-go
    `postage.Stamper`.
  - `postage::marshal_stamp` /
    `postage::convert_envelope_to_marshaled_stamp` — serialize a
    stamp into the 113-byte wire format Bee expects when a stamp
    travels alongside a chunk (`batchID || index || timestamp ||
    signature`). Mirrors bee-go `MarshalStamp` /
    `ConvertEnvelopeToMarshaledStamp` and bee-js `marshalStamp` /
    `convertEnvelopeToMarshaledStamp`.
  - `swarm::cid` — `convert_reference_to_cid` /
    `convert_cid_to_reference` for the Swarm manifest (`0xfa`) and
    feed (`0xfb`) multicodecs. Inline RFC 4648 base32 (no padding).
  - `file::FeedReader` / `file::FeedWriter` plus
    `FileApi::make_feed_reader` / `make_feed_writer` — bee-js-style
    factory wrappers around the existing free-method feed surface.
  - `api::CollectionUploadOptions::on_entry` — per-entry progress
    callback fired before the collection is packed and uploaded.
    Matches bee-js `streamDirectory` `onUploadProgress`.
- **`FileApi::stream_directory` / `stream_collection_entries`** —
  chunk-by-chunk directory upload. Each file is content-addressed
  via `FileChunker`, chunks are uploaded via `POST /chunks` with up
  to 64 concurrent in-flight requests, and the resulting Mantaray
  manifest is persisted recursively via `FileApi::save_manifest_recursively`
  (depth-first save children → marshal self → upload via `/bytes`).
  Per-chunk `OnStreamProgressFn` callback reports `(processed, total)`
  on every uploaded chunk. Mirrors bee-js `Bee.streamDirectory`
  + `MantarayNode.saveRecursively`. Tar-based `upload_collection`
  remains the right path when you need server-side `Content-Type`
  inference; `stream_directory` trades that for incremental uploads
  and live progress.
- **Doc lints fixed.** Cleared two pre-existing broken intra-doc
  links so `cargo doc --no-deps` runs clean under
  `RUSTDOCFLAGS=-Dwarnings`.

### Changed (P4 — live-Bee soak)
- **`debug::ChainState.chain_tip`**: `String` → `u64`. Live Bee
  returns this field as a JSON integer (head-of-chain block number),
  not a hex hash string. Matches bee-go `ChainStateResponse.ChainTip`
  and bee-js `ChainState.chainTip`.
- **`debug::Wallet`**: `bzz_address` / `native_address` /
  `chequebook` are now `Option<String>` and the chequebook field is
  renamed to `chequebook_contract_address` (with serde alias for the
  legacy `chequebook` key). Bee 2.7.2 / API 8.0.0 dropped the two
  address fields entirely and renamed the chequebook key. Both old
  and new wire shapes round-trip; covered by two new wiremock tests.
- **`debug::SUPPORTED_API_VERSION`**: `7.4.1` → `8.0.0`.
- **`debug::SUPPORTED_BEE_VERSION_EXACT`**: `2.7.1-61fab37b` →
  `2.7.2-rc1-83612d37` (matches the build the integration check is
  green against).

### Added (P4)
- **`PostageApi::create_postage_batch_with_options`** — full
  `PostageBatchOptions` support (label / immutable / gas-price /
  gas-limit). The legacy three-arg `create_postage_batch` now routes
  through it under the hood. Needed to buy non-immutable batches for
  the live mutable-batch lifecycle test.
- **`examples/integration-check`** is now a comprehensive soak:
  - Read-only operator/accounting/chequebook/loggers sweep
    (status / peers / readiness / wallet / balances / accounting /
    stake / redistribution_state / chequebook_balance / settlements /
    loggers / pending_transactions, plus `is_gateway` / `reserve_state`).
  - `get_postage_batches` (list).
  - `get_storage_cost` end-to-end on the live chain state.
  - Direct `upload_chunk` / `download_chunk` round-trip.
  - Encrypted upload (`encrypt: Some(true)`, 64-byte reference,
    body round-trip).
  - `create_feed_manifest` / `find_next_index` / `is_retrievable`
    against a freshly-uploaded reference.
  - `FeedReader` / `FeedWriter` round-trip with retry budget.
  - `reupload` (stewardship).
  - `post_envelope`, `create_grantees`, `get_grantees` (ACT).
  - PSS + GSOC websocket subscribe smoke (single-node testnet
    cannot exercise cross-node delivery; subscription open + clean
    cancel is the verifiable bit).
  - Mutable-batch lifecycle (gated on `BEE_MUTABLE_BATCH_ID` or
    `BEE_BUY_MUTABLE=1`): poll for usable, top_up_batch,
    dilute_batch.

### Notes (P4)
- The expanded soak passes against a live Bee 2.7.2-rc1 on Sepolia.
  Two real wire-format bugs surfaced and were fixed (`chain_tip`
  type, `Wallet` field renames). The remaining single-node
  limitations (PSS/GSOC self-send across neighborhoods) are
  documented in the example.

### Notes
- bee-go's three live-Bee bug fixes are baked in from day one:
  the `amount` JSON tag, the `immutableFlag` JSON tag, and the
  bigint-as-string chain-state decoder. The SOC eth-signed-message
  digest and the `span || payload` SOC body framing are correct
  from day one too.
- 225 unit + integration tests pass; clippy clean; doc clean.
