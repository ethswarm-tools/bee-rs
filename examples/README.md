# bee-rs examples

Forty-something runnable programs that show how to use the bee-rs
client against a live Bee node. Every example is a single file under
this directory and runs via `cargo run --example <name> -- <args>`.

The examples are grouped by tier:

- [Setup](#setup) — what you need before running anything.
- [Quickstart & basics](#quickstart--basics) — the ~5 examples to read first.
- [Tier A — feature demos](#tier-a--feature-demos) — one example per
  primitive (manifests, ACT, SOC, GSOC, feeds, stamps, …).
- [Tier B — starter projects](#tier-b--starter-projects) — small CLI
  tools (`swarm-paste`, `swarm-deploy`, `swarm-vault`, …) that
  combine multiple primitives into something you'd actually ship.

Every example prints a usage line on `--help` (or on missing
arguments). Read the file header — each one starts with a docstring
explaining what it does, the exact CLI shape, and the env vars it
reads.

---

## Setup

All examples accept these environment variables:

| Variable | Purpose | Default |
|---|---|---|
| `BEE_URL` | Bee node base URL | `http://localhost:1633` |
| `BEE_BATCH_ID` | Hex postage batch ID for uploads | required for any upload |
| `BEE_SIGNER_HEX` | 32-byte hex private key | required for feeds, SOC, GSOC, ACT |

Generate a signer once:

```sh
openssl rand -hex 32 > .signer
export BEE_SIGNER_HEX=$(cat .signer)
```

Buy a batch once (or reuse an existing one — see
[`buy-batch`](buy-batch.rs)):

```sh
cargo run --example buy-batch
export BEE_BATCH_ID=<the hex printed above>
```

> On Sepolia the first usability of a fresh batch takes several
> minutes. Reuse `BEE_BATCH_ID` whenever possible.

---

## Quickstart & basics

Read these in order if you're new to bee-rs.

| Example | What it shows |
|---|---|
| [`quickstart`](quickstart.rs) | End-to-end smoke test: health → batch → upload → download. |
| [`basic-usage`](basic-usage.rs) | Health and node-info round-trip. |
| [`status`](status.rs) | Pretty-print `Status`, `Health`, `NodeInfo`. |
| [`buy-batch`](buy-batch.rs) | Buy a postage batch and wait until it's usable. |
| [`upload-picture`](upload-picture.rs) / [`download-picture`](download-picture.rs) | The "hello world" of file uploads. |
| [`integration-check`](integration-check.rs) | Sanity-check a Bee node before running other examples. |

---

## Tier A — feature demos

One example per primitive. Most run against any Bee node (no Sepolia
traffic required) given a usable batch.

### Pinning & retrievability

| Example | Pattern |
|---|---|
| [`pinning-workflow`](pinning-workflow.rs) | `pin` → `list` → `is_retrievable` → `reupload` → `unpin` → re-pin |
| [`tag-upload-progress`](tag-upload-progress.rs) | Tag-tracked deferred upload + progress polling |

### Manifests & encrypted folders

| Example | Pattern |
|---|---|
| [`manifest-add-file`](manifest-add-file.rs) | Build a manifest offline, upload, verify path serves |
| [`manifest-move-file`](manifest-move-file.rs) | `add_fork` / `remove_fork` to rename a path |
| [`encrypted-upload`](encrypted-upload.rs) | `encrypt: true` upload, 64-byte ref round-trip |
| [`encrypted-folder-walk`](encrypted-folder-walk.rs) | Walk an encrypted manifest by downloading the root chunk + offline `unmarshal` |

### Feeds

| Example | Pattern |
|---|---|
| [`feed-update`](feed-update.rs) | `update_feed` → `fetch_latest_feed_update` |
| [`feed-manifest`](feed-manifest.rs) | `create_feed_manifest` → stable `/bzz/<feedRef>/` URL across updates |
| [`feed-history`](feed-history.rs) | Walk `0..N` indexes via `make_feed_identifier` + SOC reader |

### Single-Owner Chunks & GSOC

| Example | Pattern |
|---|---|
| [`soc-write-read`](soc-write-read.rs) | `make_soc_writer` → upload at distinct identifiers → verify via reader |
| [`gsoc-mined-pubsub`](gsoc-mined-pubsub.rs) | `gsoc_mine` for the local overlay → subscribe + send → receive |

### Access Control Trie (ACT)

| Example | Pattern |
|---|---|
| [`act-share`](act-share.rs) | Upload with `act: true` → `create_grantees` → `patch_grantees(add, revoke)` → download as publisher |

### PSS

| Example | Pattern |
|---|---|
| [`pss-send-receive`](pss-send-receive.rs) | `subscribe` + `send` over a topic prefix |

### Postage stamps

| Example | Pattern |
|---|---|
| [`list-batches`](list-batches.rs) | Tabular `get_postage_batches` with TTL/utilization/usable/immutable flags |
| [`stamp-utilization`](stamp-utilization.rs) | Per-bucket fill via `get_postage_batch_buckets` |
| [`stamp-cost`](stamp-cost.rs) | Offline cost projection (`get_stamp_cost`) |
| [`stamp-cost-live`](stamp-cost-live.rs) | Live `chain_state` price → full preview |
| [`chain-state`](chain-state.rs) | Block, chain tip, current price, total amount |
| [`stamper-client-side`](stamper-client-side.rs) | Client-side `Stamper` with bucket state + persist/resume |
| [`redundant-upload`](redundant-upload.rs) | Upload with each `RedundancyLevel`, show overhead |

### Offline conversion / utilities

| Example | Pattern |
|---|---|
| [`ref-to-cid`](ref-to-cid.rs) | `convert_reference_to_cid` (manifest + feed codecs) |
| [`ens-locator`](ens-locator.rs) | `ResourceLocator::from_ens` / `from_reference` / `parse` |
| [`key-gen`](key-gen.rs) | Generate secp256k1 key, derive Ethereum address |

### Misc

| Example | Pattern |
|---|---|
| [`upload-directory`](upload-directory.rs) | `upload_collection` from a local directory |

---

## Tier B — starter projects

These compose multiple primitives into mini CLI apps. Each one
maintains a small JSON state file in the working directory; treat
them as scaffolds you'd fork and extend.

| Example | Pitch | Composes |
|---|---|---|
| [`swarm-paste`](swarm-paste.rs) | Pastebin: stdin → upload → `/bzz/<ref>/` URL. | `upload_file` |
| [`swarm-deploy`](swarm-deploy.rs) | `git push`-style site deploy with feed manifest + history rollback. | `upload_collection` + feed manifest |
| [`swarm-blog`](swarm-blog.rs) | Markdown blog with one stable URL, regenerated HTML on each `publish`. | collections + feed manifest |
| [`swarm-fs`](swarm-fs.rs) | Filesystem-style staging tree (`add` / `mv` / `rm`) → `publish` builds a manifest. | local JSON + collection upload |
| [`swarm-chat`](swarm-chat.rs) | Terminal chat over PSS with username envelopes (run two instances). | PSS subscribe + send |
| [`swarm-vault`](swarm-vault.rs) | Personal encrypted dropbox with stable URL. | encrypted upload + index manifest + feed |
| [`swarm-share`](swarm-share.rs) | Revocable file sharing (`share` / `revoke` / `grantees`). | ACT upload + grantee CRUD |
| [`swarm-pinner`](swarm-pinner.rs) | Daemon: watch dir, upload+pin new files, retrievability poll. | pin + `is_retrievable` |
| [`swarm-feed-rss`](swarm-feed-rss.rs) | Read-only aggregator over N feeds (no signer needed). | feeds |
| [`swarm-relay`](swarm-relay.rs) | Single-batch upload gateway with persisted bucket bookkeeping. | client-side `Stamper` + chunker |
| [`swarm-keyring`](swarm-keyring.rs) | Passphrase-encrypted secp256k1 key store (scrypt + AES-GCM). | offline crypto |
| [`swarm-cost-monitor`](swarm-cost-monitor.rs) | Operator dashboard: batch TTLs, live price, projected refill cost. | `get_postage_batches` + `chain_state` |

---

## Caveats and gotchas

- **PSS doesn't loop back.** A node won't receive its own PSS
  messages. To exercise [`pss-send-receive`](pss-send-receive.rs) or
  [`swarm-chat`](swarm-chat.rs), run two instances against different
  Bee URLs (or different batches).
- **GSOC mining lands chunks in the local neighbourhood.** If your
  Bee node has no peers (single-node dev), GSOC pubsub still works
  because every chunk is "local" in a 1-node network.
- **ACT downloads need the publisher identity.** Bee resolves ACT
  permissions using the *node's* key; on a single-node setup, you can
  download as the publisher but cannot exercise the
  grantee-downloads-as-different-identity flow without a second node.
- **Sepolia batch usability is slow.** Reuse `BEE_BATCH_ID` whenever
  possible. First-time usability of a fresh batch can take several
  minutes.
- **Mutable feed-manifest URLs** stay the same across updates;
  per-update upload references change. Use the manifest URL for
  sharing, the per-update reference for verification.

---

## Adding a new example

1. Create `examples/<name>.rs` with a `//! …` docstring header
   covering: purpose, CLI shape (`cargo run --example <name> -- …`),
   and required env vars.
2. Wire `main` through a `run() -> Result<(), Error>` returning
   `ExitCode`, matching the existing examples.
3. Verify with `cargo check --example <name>`.

The header docstring is the example's documentation — keep it
honest and runnable.
