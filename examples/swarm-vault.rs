//! swarm-vault — personal encrypted file dropbox.
//!
//! Files are encrypt-uploaded (Bee returns a 64-byte reference whose
//! key travels inline). A `name → encrypted_ref` index is uploaded to
//! Swarm and pointed at by a feed manifest, so the vault has one
//! stable URL whose contents grow as files are added.
//!
//! The local `.swarm-vault.json` caches the current state for fast
//! lookups; copy it (or just the feed manifest URL) to access the
//! vault from another machine.
//!
//! ```text
//! swarm-vault init  <name>
//! swarm-vault put   <name>  <local-file>
//! swarm-vault get   <name>  <out-path>
//! swarm-vault list
//! swarm-vault rm    <name>
//! ```
//!
//! Environment:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BATCH_ID` — usable postage batch (required).
//! - `BEE_SIGNER_HEX` — 32-byte hex private key (required).

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use bee::api::{RedundantUploadOptions, UploadOptions};
use bee::swarm::{BatchId, PrivateKey, Reference, Topic};
use bee::{Client, Error};
use bytes::Bytes;
use serde::{Deserialize, Serialize};

const STATE_FILE: &str = ".swarm-vault.json";

#[derive(Serialize, Deserialize, Debug)]
struct VaultState {
    name: String,
    topic_hex: String,
    owner_hex: String,
    feed_manifest_ref: String,
    latest_index_ref: Option<String>,
    index: BTreeMap<String, IndexEntry>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct IndexEntry {
    encrypted_ref: String,
    size: u64,
    ts: u64,
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Error> {
    let url = env::var("BEE_URL").unwrap_or_else(|_| "http://localhost:1633".into());
    let mut args = env::args().skip(1);
    let cmd = args
        .next()
        .ok_or_else(|| Error::argument("usage: swarm-vault <init|put|get|list|rm> ..."))?;
    let client = Client::new(&url)?;

    match cmd.as_str() {
        "init" => {
            let name = args
                .next()
                .ok_or_else(|| Error::argument("usage: swarm-vault init <name>"))?;
            cmd_init(&client, &url, &name).await
        }
        "put" => {
            let name = args
                .next()
                .ok_or_else(|| Error::argument("usage: swarm-vault put <name> <local-file>"))?;
            let local = args
                .next()
                .ok_or_else(|| Error::argument("usage: swarm-vault put <name> <local-file>"))?;
            cmd_put(&client, &name, &local).await
        }
        "get" => {
            let name = args
                .next()
                .ok_or_else(|| Error::argument("usage: swarm-vault get <name> <out-path>"))?;
            let out = args
                .next()
                .ok_or_else(|| Error::argument("usage: swarm-vault get <name> <out-path>"))?;
            cmd_get(&client, &name, &out).await
        }
        "list" => cmd_list(),
        "rm" => {
            let name = args
                .next()
                .ok_or_else(|| Error::argument("usage: swarm-vault rm <name>"))?;
            cmd_rm(&client, &name).await
        }
        other => Err(Error::argument(format!("unknown command: {other}"))),
    }
}

async fn cmd_init(client: &Client, url: &str, name: &str) -> Result<(), Error> {
    if PathBuf::from(STATE_FILE).exists() {
        return Err(Error::argument(format!("{STATE_FILE} already exists")));
    }
    let batch_id = env_batch()?;
    let signer = env_signer()?;
    let owner = signer.public_key()?.address();
    let topic = Topic::from_string(&format!("swarm-vault:{name}"));

    let feed_manifest = client
        .file()
        .create_feed_manifest(&batch_id, &owner, &topic)
        .await?;

    let st = VaultState {
        name: name.into(),
        topic_hex: topic.to_hex(),
        owner_hex: owner.to_hex(),
        feed_manifest_ref: feed_manifest.to_hex(),
        latest_index_ref: None,
        index: BTreeMap::new(),
    };
    save(&st)?;

    let trimmed = url.trim_end_matches('/');
    println!("Initialised vault {name:?}");
    println!("  feed manifest: {}", feed_manifest.to_hex());
    println!("  vault URL:     {trimmed}/bzz/{}/", feed_manifest.to_hex());
    println!("  (treat the URL as the vault password — anyone with it can read filenames.)");
    Ok(())
}

async fn cmd_put(client: &Client, name: &str, local: &str) -> Result<(), Error> {
    let mut st = load()?;
    let batch_id = env_batch()?;
    let signer = env_signer()?;
    let topic = Topic::from_hex(&st.topic_hex)?;

    let body = fs::read(local).map_err(|e| Error::argument(format!("read {local}: {e}")))?;
    let size = body.len() as u64;

    println!("Uploading (encrypted) {} bytes...", body.len());
    let opts = RedundantUploadOptions {
        base: UploadOptions {
            encrypt: Some(true),
            ..Default::default()
        },
        ..Default::default()
    };
    // upload_data (POST /bytes) gives us a content-addressed leaf we
    // can download cleanly via /bytes again. upload_file (POST /bzz)
    // would wrap the bytes in a single-fork manifest, requiring /bzz
    // to fetch and a filename to address.
    let upload = client
        .file()
        .upload_data(&batch_id, Bytes::from(body), Some(&opts))
        .await?;
    let _ = name; // kept for forward-compat metadata when extending

    st.index.insert(
        name.into(),
        IndexEntry {
            encrypted_ref: upload.reference.to_hex(),
            size,
            ts: now_secs(),
        },
    );
    publish_index(client, &mut st, &batch_id, &signer, &topic).await?;
    println!("  vault entry: {name}\n  encrypted_ref: {}", upload.reference.to_hex());
    Ok(())
}

async fn cmd_get(client: &Client, name: &str, out: &str) -> Result<(), Error> {
    let st = load()?;
    let entry = st
        .index
        .get(name)
        .ok_or_else(|| Error::argument(format!("no such vault entry: {name}")))?;
    let r = Reference::from_hex(&entry.encrypted_ref)?;
    let body = client.file().download_data(&r, None).await?;
    fs::write(out, &body).map_err(|e| Error::argument(format!("write {out}: {e}")))?;
    println!("Wrote {} bytes to {out}", body.len());
    Ok(())
}

fn cmd_list() -> Result<(), Error> {
    let st = load()?;
    if st.index.is_empty() {
        println!("(empty vault)");
        return Ok(());
    }
    println!("vault {:?}  feed: {}", st.name, st.feed_manifest_ref);
    println!("\n{:<24}  {:>10}  {}", "name", "size", "encrypted_ref");
    for (name, e) in &st.index {
        println!("{name:<24}  {:>10}  {}", e.size, e.encrypted_ref);
    }
    Ok(())
}

async fn cmd_rm(client: &Client, name: &str) -> Result<(), Error> {
    let mut st = load()?;
    let batch_id = env_batch()?;
    let signer = env_signer()?;
    let topic = Topic::from_hex(&st.topic_hex)?;
    if st.index.remove(name).is_none() {
        return Err(Error::argument(format!("no such entry: {name}")));
    }
    publish_index(client, &mut st, &batch_id, &signer, &topic).await?;
    println!("Removed {name}");
    Ok(())
}

async fn publish_index(
    client: &Client,
    st: &mut VaultState,
    batch_id: &BatchId,
    signer: &PrivateKey,
    topic: &Topic,
) -> Result<(), Error> {
    let bytes = serde_json::to_vec_pretty(&st.index)?;
    let r = client
        .file()
        .upload_data(batch_id, Bytes::from(bytes), None)
        .await?;
    client
        .file()
        .update_feed_with_reference(batch_id, signer, topic, &r.reference, None)
        .await?;
    st.latest_index_ref = Some(r.reference.to_hex());
    save(st)?;
    Ok(())
}

fn env_batch() -> Result<BatchId, Error> {
    let h = env::var("BEE_BATCH_ID").map_err(|_| Error::argument("BEE_BATCH_ID is required"))?;
    BatchId::from_hex(&h)
}

fn env_signer() -> Result<PrivateKey, Error> {
    let h = env::var("BEE_SIGNER_HEX").map_err(|_| Error::argument("BEE_SIGNER_HEX is required"))?;
    PrivateKey::from_hex(&h)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn save(s: &VaultState) -> Result<(), Error> {
    let bytes = serde_json::to_vec_pretty(s)?;
    fs::write(STATE_FILE, bytes).map_err(|e| Error::argument(format!("write: {e}")))
}

fn load() -> Result<VaultState, Error> {
    let bytes = fs::read(STATE_FILE)
        .map_err(|_| Error::argument(format!("{STATE_FILE} not found — run `init` first")))?;
    Ok(serde_json::from_slice(&bytes)?)
}
