# bee-rs

Rust client for the [Swarm](https://www.ethswarm.org/) Bee API.
Async-first, MSRV 1.85, `forbid(unsafe_code)`.

The functional target is parity with [bee-js] (canonical TypeScript
client) and [bee-go] (typed Go port). bee-go is the primary reference
for shape and behavior since the typed-language → typed-language
mapping is direct; bee-js is the source of truth for wire-format edge
cases.

## Quick start

```rust
use bee::Client;

#[tokio::main]
async fn main() -> Result<(), bee::Error> {
    let client = Client::new("http://localhost:1633")?;

    let health = client.debug().health().await?;
    println!("bee {} api {}", health.version, health.api_version);

    Ok(())
}
```

Run the live-Bee smoke test:

```bash
BEE_URL=http://localhost:1633 cargo run --example integration-check
```

Set `BEE_BATCH_ID=<hex>` to reuse an existing postage batch (the
first usability of a fresh batch on Sepolia takes minutes).

## Layout

| Module          | bee-go counterpart  | Scope                                                |
| --------------- | ------------------- | ---------------------------------------------------- |
| `bee::swarm`    | `pkg/swarm`         | Typed bytes, BMT, SOC, BZZ/DAI, Duration, Size, errs |
| `bee::api`      | `pkg/api`           | Upload/download options, pin, tag, grantee, envelope |
| `bee::file`     | `pkg/file`          | Data/file/chunk/SOC/feed/collection uploads          |
| `bee::postage`  | `pkg/postage`       | Batch CRUD, pure stamp math                          |
| `bee::debug`    | `pkg/debug`         | Health, versions, accounting, chequebook, stake      |
| `bee::pss`      | `pkg/pss`           | PSS send/subscribe/receive (websocket)               |
| `bee::gsoc`     | `pkg/gsoc`          | GSOC send/subscribe + offline SOC address            |
| `bee::manifest` | `pkg/manifest`      | Mantaray trie, v0.2 wire format, `ResourceLocator`   |
| `bee::storage`  | _bee-js only_       | High-level `buy_storage` / `extend_storage_*`        |
| `bee::dev`      | _bee-js only_       | `DevClient` newtype around `Client`                  |

## Stack

- `reqwest` (rustls-tls) for HTTP, `tokio-tungstenite` for websockets
- `k256` for secp256k1, `sha3` for keccak256
- `thiserror` for typed errors, `serde` for JSON
- `num-bigint` for BZZ/DAI / chain-state amounts
- `tar` for in-memory collection uploads

## Migration cheat sheet

bee-js / bee-go callers should find the bee-rs surface familiar.
Each row gives the bee-js method, the bee-go method, and the bee-rs
equivalent. All bee-rs methods are `async`.

### Connection + node info

| bee-js                   | bee-go                          | bee-rs                                  |
| ------------------------ | ------------------------------- | --------------------------------------- |
| `new Bee(url)`           | `bee.NewClient(url)`            | `Client::new(url)?`                     |
| `Bee.getHealth()`        | `c.Debug.GetHealth(ctx)`        | `client.debug().health().await`         |
| `Bee.getVersions()`      | `c.Debug.GetVersions(ctx)`      | `client.debug().versions().await`       |
| `Bee.isConnected()`      | `c.Debug.IsConnected(ctx)`      | `client.debug().is_connected().await`   |
| `Bee.getNodeAddresses()` | `c.Debug.Addresses(ctx)`        | `client.debug().addresses().await`      |
| `Bee.getTopology()`      | `c.Debug.Topology(ctx)`         | `client.debug().topology().await`       |
| `Bee.getChainState()`    | `c.Debug.ChainState(ctx)`       | `client.debug().chain_state().await`    |

### Postage batches

| bee-js                       | bee-go                                     | bee-rs                                                    |
| ---------------------------- | ------------------------------------------ | --------------------------------------------------------- |
| `Bee.createPostageBatch(…)`  | `c.Postage.CreatePostageBatch(ctx, …)`     | `client.postage().create_postage_batch(amount, depth, …)` |
| `Bee.getPostageBatches()`    | `c.Postage.GetPostageBatches(ctx)`         | `client.postage().get_postage_batches().await`            |
| `Bee.topUpBatch(id, amount)` | `c.Postage.TopUpBatch(ctx, id, amount)`    | `client.postage().top_up_batch(&id, &amount).await`       |
| `Bee.diluteBatch(id, depth)` | `c.Postage.DiluteBatch(ctx, id, depth)`    | `client.postage().dilute_batch(&id, depth).await`         |

### Bytes / file / collection

| bee-js                            | bee-go                                       | bee-rs                                                                         |
| --------------------------------- | -------------------------------------------- | ------------------------------------------------------------------------------ |
| `Bee.uploadData(batch, data)`     | `c.File.UploadData(ctx, batch, data, opts)`  | `client.file().upload_data(&batch, data, opts).await`                          |
| `Bee.downloadData(ref)`           | `c.File.DownloadData(ctx, ref, opts)`        | `client.file().download_data(&reference, opts).await`                          |
| `Bee.uploadFile(batch, data, …)`  | `c.File.UploadFile(ctx, batch, data, …)`     | `client.file().upload_file(&batch, data, name, content_type, opts).await`      |
| `Bee.downloadFile(ref, path?)`    | `c.File.DownloadFile(ctx, ref, opts)`        | `client.file().download_file(&reference, opts).await`                          |
| `Bee.uploadCollection(entries)`   | `c.File.UploadCollectionEntries(ctx, …)`     | `client.file().upload_collection_entries(&batch, &entries, opts).await`        |
| `Bee.uploadFilesFromDirectory(d)` | `c.File.UploadCollection(ctx, batch, dir)`   | `client.file().upload_collection(&batch, dir, opts).await`                     |
| `bee.hashCollectionEntries(…)`    | _n/a_                                        | `bee::file::hash_collection_entries(&entries)?`                                |
| `bee.hashDirectory(dir)`          | _n/a_                                        | `bee::file::hash_directory(dir)?`                                              |

### Pin / tag / stewardship / grantee / envelope

| bee-js                             | bee-go                                            | bee-rs                                                              |
| ---------------------------------- | ------------------------------------------------- | ------------------------------------------------------------------- |
| `Bee.pin(ref)` / `Bee.unpin(ref)`  | `c.API.Pin / Unpin`                               | `client.api().pin(&reference).await`                                |
| `Bee.isPinned(ref)`                | `c.API.GetPin(ctx, ref)`                          | `client.api().get_pin(&reference).await`                            |
| `Bee.createTag()`                  | `c.API.CreateTag(ctx)`                            | `client.api().create_tag().await`                                   |
| `Bee.reupload(ref, batch)`         | `c.API.Reupload(ctx, ref, batch)`                 | `client.api().reupload(&reference, &batch).await`                   |
| `Bee.isRetrievable(ref)`           | `c.API.IsRetrievable(ctx, ref)`                   | `client.api().is_retrievable(&reference).await`                     |
| `Bee.createGrantees(batch, list)`  | `c.API.CreateGrantees(ctx, batch, list)`          | `client.api().create_grantees(&batch, &grantees).await`             |
| `Bee.postEnvelope(ref, batch)`     | `c.API.PostEnvelope(ctx, ref, batch)`             | `client.api().post_envelope(&batch, &reference).await`              |

### Feeds + SOC + GSOC + PSS

| bee-js                                | bee-go                                              | bee-rs                                                          |
| ------------------------------------- | --------------------------------------------------- | --------------------------------------------------------------- |
| `Bee.updateFeed(signer, topic, data)` | `c.File.UpdateFeed(ctx, batch, signer, topic, …)`   | `client.file().update_feed(&batch, &signer, &topic, data).await` |
| `Bee.fetchLatestFeedUpdate(...)`      | `c.File.FetchLatestFeedUpdate(ctx, owner, topic)`   | `client.file().fetch_latest_feed_update(&owner, &topic).await`   |
| `Bee.makeFeedReader(topic, owner)`    | _n/a_                                               | `client.file().make_feed_reader(owner, topic)`                   |
| `Bee.makeFeedWriter(topic, signer)`   | _n/a_                                               | `client.file().make_feed_writer(signer, topic)?`                 |
| `Bee.uploadSoc(...)`                  | `c.File.UploadSOC(ctx, batch, owner, id, sig, …)`   | `client.file().upload_soc(&batch, &owner, &id, &sig, data, …)`   |
| `Bee.gsocSend(batch, signer, id, …)`  | `gsocSvc.Send(ctx, batch, signer, id, data, opts)`  | `client.gsoc().send(&batch, &signer, &id, data, opts).await`     |
| `Bee.gsocSubscribe(owner, id)`        | `gsocSvc.Subscribe(ctx, owner, id)`                 | `client.gsoc().subscribe(&owner, &id).await`                     |
| `Bee.pssSend(topic, target, data, …)` | `pssSvc.PssSend(ctx, batch, topic, target, …)`      | `client.pss().send(&batch, &topic, target, data, recipient).await` |
| `Bee.pssSubscribe(topic)`             | `api.PSSSubscribe(ctx, base, dialer, topic)`        | `client.pss().subscribe(&topic).await`                           |
| `Bee.pssReceive(topic, timeout)`      | _n/a_                                               | `client.pss().receive(&topic, timeout).await`                    |

### Offline primitives

| bee-js                                | bee-go                                                | bee-rs                                                                |
| ------------------------------------- | ----------------------------------------------------- | --------------------------------------------------------------------- |
| `new Stamper(signer, batchId, depth)` | `postage.NewStamper(signer, batchID, depth)`          | `bee::postage::Stamper::from_blank(signer, batch_id, depth)?`          |
| `Stamper.stamp(chunk)`                | `(*Stamper).Stamp(chunkAddr)`                         | `stamper.stamp(&chunk_addr)?` → `bee::postage::Envelope`               |
| `convertReferenceToCid(ref, type)`    | `swarm.ConvertReferenceToCID(ref, type)`              | `bee::swarm::convert_reference_to_cid(&reference, CidType::Feed)?`     |
| `convertCidToReference(cid)`          | `swarm.ConvertCIDToReference(cid)`                    | `bee::swarm::convert_cid_to_reference(&cid)?`                          |

### Storage helpers (top-level, bee-js only)

| bee-js                                | bee-rs                                                                |
| ------------------------------------- | --------------------------------------------------------------------- |
| `Bee.getStorageCost(size, duration)`  | `bee::storage::get_storage_cost(&client, size, duration, network)`     |
| `Bee.buyStorage(size, duration, …)`   | `bee::storage::buy_storage(&client, size, duration, &opts)`            |
| `Bee.extendStorageDuration(id, dur)`  | `bee::storage::extend_storage_duration(&client, &id, duration, …)`     |
| `Bee.extendStorageSize(id, size)`     | `bee::storage::extend_storage_size(&client, &id, size)`                |
| `Bee.calculateTopUpForBzz(id, bzz)`   | `bee::storage::calculate_top_up_for_bzz(&client, &id, &target_bzz)`    |

### Utility types

| Concept              | Type                              | Notes                                                                  |
| -------------------- | --------------------------------- | ---------------------------------------------------------------------- |
| Reference (32 / 64)  | `bee::swarm::Reference`           | Hex parse via `Reference::from_hex(s)`                                 |
| Batch ID             | `bee::swarm::BatchId`             | `BatchId::from_hex(s)?` / `BatchId::new(&[u8; 32])?`                   |
| Topic / Identifier   | `Topic` / `Identifier`            | Both have `from_string(label)` keccak256-derived constructors          |
| BZZ / DAI            | `bee::swarm::Bzz` / `Dai`         | Fixed-point on `BigInt`, `Display` + `FromStr`                         |
| Duration             | `bee::swarm::BeeDuration`         | `Duration::parse("1d 4h 5m 30s")?`                                     |
| Size                 | `bee::swarm::Size`                | Decimal base (1 kB = 1000 B), `Size::parse("28MB")?`                   |
| Resource locator     | `bee::manifest::ResourceLocator`  | `Reference` *or* `<label>.eth`                                         |

## Stability

The crate is currently `0.x` — breaking changes are allowed in minor
bumps. The `1.0` line will follow once the live Bee soak (P4) and
the integration-check coverage match bee-go's. See
[`CHANGELOG.md`](./CHANGELOG.md) and [`RELEASE.md`](./RELEASE.md).

[bee-js]: https://github.com/ethersphere/bee-js
[bee-go]: https://github.com/ethswarm-tools/bee-go
