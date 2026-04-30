# bee-rs

Rust client for the [Swarm](https://www.ethswarm.org/) Bee API.

**Status:** scaffolding. Module skeleton in place.

The functional target is parity with [bee-js] (canonical TypeScript
client) and [bee-go] (typed Go port). bee-go is the primary reference
for shape and behavior since the typed-language → typed-language
mapping is direct; bee-js is the source of truth for wire-format edge
cases.

## Layout

| Module          | bee-go counterpart  | Scope                                                |
| --------------- | ------------------- | ---------------------------------------------------- |
| `bee::swarm`    | `pkg/swarm`         | Typed bytes, BMT, SOC, BZZ/DAI, Duration, Size, errs |
| `bee::api`      | `pkg/api`           | Upload/download options, pin, tag, grantee, envelope |
| `bee::file`     | `pkg/file`          | Data/file/chunk/SOC/feed/collection uploads          |
| `bee::postage`  | `pkg/postage`       | Batch CRUD, stamper, marshaled stamp                 |
| `bee::debug`    | `pkg/debug`         | Health, versions, accounting, chequebook, stake      |
| `bee::pss`      | `pkg/pss`           | PSS send/subscribe/receive (websocket)               |
| `bee::gsoc`     | `pkg/gsoc`          | GSOC send/subscribe + offline SOC address            |
| `bee::manifest` | `pkg/manifest`      | Mantaray trie, v0.2 wire format                      |

## Stack

- `reqwest` (rustls-tls) for HTTP, `tokio-tungstenite` for websockets
- `k256` for secp256k1, `sha3` for keccak256
- `thiserror` for typed errors, `serde` for JSON
- `num-bigint` for BZZ/DAI / chain-state amounts

[bee-js]: https://github.com/ethersphere/bee-js
[bee-go]: https://github.com/ethswarm-tools/bee-go
