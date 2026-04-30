# Changelog

All notable changes to bee-rs will be documented in this file. The
format follows [Keep a Changelog]; the project adheres to
[Semantic Versioning].

[Keep a Changelog]: https://keepachangelog.com/en/1.1.0/
[Semantic Versioning]: https://semver.org/spec/v2.0.0.html

## [Unreleased]

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
- **Doc lints fixed.** Cleared two pre-existing broken intra-doc
  links so `cargo doc --no-deps` runs clean under
  `RUSTDOCFLAGS=-Dwarnings`.

### Changed (P4 — live-Bee soak)
- **`debug::ChainState.chain_tip`**: `String` → `u64`. Live Bee
  returns this field as a JSON integer (head-of-chain block number),
  not a hex hash string. Matches bee-go `ChainStateResponse.ChainTip`
  and bee-js `ChainState.chainTip`.
- **`debug::SUPPORTED_API_VERSION`**: `7.4.1` → `8.0.0`.
- **`debug::SUPPORTED_BEE_VERSION_EXACT`**: `2.7.1-61fab37b` →
  `2.7.2-rc1-83612d37` (matches the build the integration check is
  green against).
- **`examples/integration-check`** retries the post-update feed
  lookup with backoff up to 30 s — newly uploaded SOC chunks need a
  moment to be retrievable on a live network.

### Notes (P4)
- **24/24 pass against a live Bee 2.7.2-rc1 on Sepolia**: read-only
  connectivity, postage batch lifecycle, bytes/file/collection
  upload + download, pin/tag, feeds, PSS, GSOC. Reused an existing
  batch via `BEE_BATCH_ID` (Sepolia first-usability is slow).

### Notes
- bee-go's three live-Bee bug fixes are baked in from day one:
  the `amount` JSON tag, the `immutableFlag` JSON tag, and the
  bigint-as-string chain-state decoder. The SOC eth-signed-message
  digest and the `span || payload` SOC body framing are correct
  from day one too.
- 225 unit + integration tests pass; clippy clean; doc clean.
