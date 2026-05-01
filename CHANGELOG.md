# Changelog

All notable changes to bee-rs will be documented in this file. The
format follows [Keep a Changelog]; the project adheres to
[Semantic Versioning].

[Keep a Changelog]: https://keepachangelog.com/en/1.1.0/
[Semantic Versioning]: https://semver.org/spec/v2.0.0.html

## [Unreleased]

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
