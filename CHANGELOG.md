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

### Notes
- bee-go's three live-Bee bug fixes are baked in from day one:
  the `amount` JSON tag, the `immutableFlag` JSON tag, and the
  bigint-as-string chain-state decoder. The SOC eth-signed-message
  digest and the `span || payload` SOC body framing are correct
  from day one too.
- 202 unit + integration tests pass; clippy clean.
