//! swarm-deploy — `git push`-style site deploy on Swarm.
//!
//! Each project gets a feed manifest (one stable `/bzz/<feedRef>/`
//! URL); each `push` re-uploads the directory, updates the feed
//! pointer, and appends a row to `.swarmdeploy/state.json` for
//! history. Rollback rewinds the feed to a past upload's reference.
//!
//! ```text
//! swarm-deploy init  <topic-name>            # one-time, creates feed manifest
//! swarm-deploy push  <local-dir>             # upload + update feed
//! swarm-deploy history                       # list past versions
//! swarm-deploy rollback <index>              # point feed back at version <index>
//! ```
//!
//! Environment:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BATCH_ID` — usable postage batch (required).
//! - `BEE_SIGNER_HEX` — 32-byte hex private key (required).

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use bee::swarm::{BatchId, PrivateKey, Reference, Topic};
use bee::{Client, Error};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
struct State {
    topic_name: String,
    topic_hex: String,
    owner_hex: String,
    feed_manifest_ref: String,
    history: Vec<HistoryEntry>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct HistoryEntry {
    timestamp: u64,
    site_ref: String,
    note: String,
}

const STATE_PATH: &str = ".swarmdeploy/state.json";

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
        .ok_or_else(|| Error::argument("usage: swarm-deploy <init|push|history|rollback> ..."))?;

    let client = Client::new(&url)?;
    match cmd.as_str() {
        "init" => {
            let topic_name = args
                .next()
                .ok_or_else(|| Error::argument("usage: swarm-deploy init <topic-name>"))?;
            cmd_init(&client, &url, &topic_name).await
        }
        "push" => {
            let dir = args
                .next()
                .ok_or_else(|| Error::argument("usage: swarm-deploy push <local-dir>"))?;
            let note = args.next().unwrap_or_default();
            cmd_push(&client, &url, &dir, &note).await
        }
        "history" => cmd_history(),
        "rollback" => {
            let idx_str = args
                .next()
                .ok_or_else(|| Error::argument("usage: swarm-deploy rollback <index>"))?;
            let idx: usize = idx_str
                .parse()
                .map_err(|e| Error::argument(format!("invalid index: {e}")))?;
            cmd_rollback(&client, &url, idx).await
        }
        other => Err(Error::argument(format!("unknown command: {other}"))),
    }
}

async fn cmd_init(client: &Client, url: &str, topic_name: &str) -> Result<(), Error> {
    if PathBuf::from(STATE_PATH).exists() {
        return Err(Error::argument(format!(
            "{STATE_PATH} already exists — already initialised"
        )));
    }
    let batch_id = env_batch()?;
    let signer = env_signer()?;
    let owner = signer.public_key()?.address();
    let topic = Topic::from_string(topic_name);

    let feed_manifest = client
        .file()
        .create_feed_manifest(&batch_id, &owner, &topic)
        .await?;

    let state = State {
        topic_name: topic_name.into(),
        topic_hex: topic.to_hex(),
        owner_hex: owner.to_hex(),
        feed_manifest_ref: feed_manifest.to_hex(),
        history: vec![],
    };
    save_state(&state)?;

    let trimmed = url.trim_end_matches('/');
    println!("Initialised swarm-deploy for {topic_name:?}");
    println!("  feed manifest: {}", feed_manifest.to_hex());
    println!("  stable URL:    {trimmed}/bzz/{}/", feed_manifest.to_hex());
    println!("\n(Empty until first `swarm-deploy push <dir>`.)");
    Ok(())
}

async fn cmd_push(client: &Client, url: &str, dir: &str, note: &str) -> Result<(), Error> {
    let mut state = load_state()?;
    let batch_id = env_batch()?;
    let signer = env_signer()?;
    let topic = Topic::from_hex(&state.topic_hex)?;

    println!("Uploading {dir}...");
    let result = client
        .file()
        .upload_collection(&batch_id, dir, None)
        .await?;
    let site_ref = result.reference.clone();
    println!("  site ref: {}", site_ref.to_hex());

    println!("Updating feed pointer...");
    client
        .file()
        .update_feed_with_reference(&batch_id, &signer, &topic, &site_ref, None)
        .await?;

    let ts = now_secs();
    state.history.push(HistoryEntry {
        timestamp: ts,
        site_ref: site_ref.to_hex(),
        note: note.into(),
    });
    save_state(&state)?;

    let trimmed = url.trim_end_matches('/');
    println!("\nDeployed v{}", state.history.len());
    println!("  stable URL: {trimmed}/bzz/{}/", state.feed_manifest_ref);
    println!("  this rev:   {trimmed}/bzz/{}/", site_ref.to_hex());
    Ok(())
}

fn cmd_history() -> Result<(), Error> {
    let state = load_state()?;
    if state.history.is_empty() {
        println!("(no deploys yet)");
        return Ok(());
    }
    println!("topic:         {}", state.topic_name);
    println!("feed manifest: {}", state.feed_manifest_ref);
    println!("\n#  {:<10}  {:<64}  note", "ts", "site_ref");
    for (i, e) in state.history.iter().enumerate() {
        println!("{:<2} {:<10}  {:<64}  {}", i, e.timestamp, e.site_ref, e.note);
    }
    Ok(())
}

async fn cmd_rollback(client: &Client, url: &str, idx: usize) -> Result<(), Error> {
    let mut state = load_state()?;
    let target = state
        .history
        .get(idx)
        .ok_or_else(|| Error::argument(format!("no version at index {idx}")))?
        .clone();
    let batch_id = env_batch()?;
    let signer = env_signer()?;
    let topic = Topic::from_hex(&state.topic_hex)?;
    let site_ref = Reference::from_hex(&target.site_ref)?;

    client
        .file()
        .update_feed_with_reference(&batch_id, &signer, &topic, &site_ref, None)
        .await?;
    state.history.push(HistoryEntry {
        timestamp: now_secs(),
        site_ref: target.site_ref.clone(),
        note: format!("rollback to #{idx}"),
    });
    save_state(&state)?;

    let trimmed = url.trim_end_matches('/');
    println!("Rolled back to #{idx}: {}", target.site_ref);
    println!("  stable URL: {trimmed}/bzz/{}/", state.feed_manifest_ref);
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

fn save_state(s: &State) -> Result<(), Error> {
    if let Some(parent) = PathBuf::from(STATE_PATH).parent() {
        fs::create_dir_all(parent).map_err(|e| Error::argument(format!("mkdir: {e}")))?;
    }
    let bytes = serde_json::to_vec_pretty(s)?;
    fs::write(STATE_PATH, bytes).map_err(|e| Error::argument(format!("write state: {e}")))?;
    Ok(())
}

fn load_state() -> Result<State, Error> {
    let bytes = fs::read(STATE_PATH)
        .map_err(|_| Error::argument(format!("{STATE_PATH} not found — run `init` first")))?;
    let s: State = serde_json::from_slice(&bytes)?;
    Ok(s)
}
