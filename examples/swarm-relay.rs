//! swarm-relay — single-batch upload gateway with persisted bucket
//! tracking. Foundation for any "free-tier" relay or shared-batch
//! service: hold one batch + signer, accept user uploads, refuse them
//! pre-emptively when the local Stamper says a bucket is full.
//!
//! Each `relay` invocation:
//!   1. Loads (or initialises) Stamper state from `.swarm-relay-state.json`.
//!   2. Hashes the input file via `FileChunker` to enumerate every
//!      chunk address that will land on Bee.
//!   3. For each address, calls `Stamper::stamp` — sign + bucket
//!      increment. Failure here means the batch can no longer cover
//!      this file; we abort *before* uploading.
//!   4. Uploads the file via `upload_data` (Bee re-stamps internally;
//!      the local stamps act as predictive bookkeeping).
//!   5. Persists the new bucket state.
//!
//! ```text
//! swarm-relay init                # initialise state
//! swarm-relay relay <local-file>  # ingest one file
//! swarm-relay stats               # batch utilization snapshot
//! ```
//!
//! Wrap this binary in any HTTP server (axum, hyper, …) to expose a
//! `POST /upload` endpoint — the relay logic is the interesting part.
//!
//! Environment:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BATCH_ID` — usable postage batch (required for `init`).
//! - `BEE_SIGNER_HEX` — 32-byte hex private key (required for `init`).

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use bee::postage::{NUM_BUCKETS, Stamper};
use bee::swarm::{BatchId, FileChunker, PrivateKey};
use bee::{Client, Error};
use bytes::Bytes;
use serde::{Deserialize, Serialize};

const STATE_FILE: &str = ".swarm-relay-state.json";

#[derive(Serialize, Deserialize, Debug)]
struct RelayState {
    batch_id: String,
    signer_hex: String,
    depth: u8,
    buckets: Vec<u32>,
    uploaded_files: u32,
    uploaded_bytes: u64,
    rejected_files: u32,
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
        .ok_or_else(|| Error::argument("usage: swarm-relay <init|relay|stats|reset>"))?;
    match cmd.as_str() {
        "init" => cmd_init(&url).await,
        "relay" => {
            let path = args
                .next()
                .ok_or_else(|| Error::argument("usage: swarm-relay relay <local-file>"))?;
            cmd_relay(&url, &path).await
        }
        "stats" => cmd_stats(),
        "reset" => cmd_reset(),
        other => Err(Error::argument(format!("unknown command: {other}"))),
    }
}

async fn cmd_init(url: &str) -> Result<(), Error> {
    if PathBuf::from(STATE_FILE).exists() {
        return Err(Error::argument(format!(
            "{STATE_FILE} already exists — use `reset` first"
        )));
    }
    let batch_hex =
        env::var("BEE_BATCH_ID").map_err(|_| Error::argument("BEE_BATCH_ID is required"))?;
    let batch_id = BatchId::from_hex(&batch_hex)?;
    let signer_hex =
        env::var("BEE_SIGNER_HEX").map_err(|_| Error::argument("BEE_SIGNER_HEX is required"))?;
    let _ = PrivateKey::from_hex(&signer_hex)?;

    let client = Client::new(url)?;
    let batch = client.postage().get_postage_batch(&batch_id).await?;
    let st = RelayState {
        batch_id: batch_hex,
        signer_hex,
        depth: batch.depth,
        buckets: vec![0u32; NUM_BUCKETS],
        uploaded_files: 0,
        uploaded_bytes: 0,
        rejected_files: 0,
    };
    save(&st)?;
    println!("Relay initialised:");
    println!("  batch:    {}", batch.batch_id.to_hex());
    println!("  depth:    {}", batch.depth);
    println!("  max_slot: {}", 1u32 << (batch.depth - 16));
    println!("\nReady. Use `swarm-relay relay <file>` to ingest.");
    Ok(())
}

async fn cmd_relay(url: &str, path: &str) -> Result<(), Error> {
    let mut st = load()?;
    let batch_id = BatchId::from_hex(&st.batch_id)?;
    let signer = PrivateKey::from_hex(&st.signer_hex)?;
    let body = fs::read(path).map_err(|e| Error::argument(format!("read {path}: {e}")))?;
    let size = body.len() as u64;

    // Build a Stamper from current state, then enumerate every chunk
    // address via FileChunker and pre-stamp each one. If any bucket
    // overflows, reject before uploading.
    let mut stamper = Stamper::from_state(signer, batch_id, st.buckets.clone(), st.depth)?;
    let stamper_arc = Arc::new(Mutex::new(stamper.clone()));
    let s = stamper_arc.clone();
    let mut chunker = FileChunker::with_callback(move |sealed| {
        let mut g = s.lock().unwrap();
        g.stamp(sealed.address.as_bytes())?;
        Ok(())
    });
    chunker.write(&body)?;
    let root = match chunker.finalize() {
        Ok(r) => r,
        Err(e) => {
            // Could be a bucket-full error from the callback.
            st.rejected_files += 1;
            save(&st)?;
            return Err(Error::argument(format!(
                "rejected (likely bucket full): {e}"
            )));
        }
    };

    println!(
        "Pre-stamped {} bytes → root {}",
        body.len(),
        root.address.to_hex()
    );

    // Update bucket state to reflect the pre-stamping.
    stamper = Arc::try_unwrap(stamper_arc)
        .map_err(|_| Error::argument("stamper still shared"))?
        .into_inner()
        .map_err(|_| Error::argument("stamper poisoned"))?;
    st.buckets = stamper.state().to_vec();

    // Now actually upload. Bee handles real stamping using the same
    // batch; the local Stamper's counters predict on-batch utilisation.
    let client = Client::new(url)?;
    let result = client
        .file()
        .upload_data(&batch_id, Bytes::from(body), None)
        .await?;
    if result.reference != root.address {
        eprintln!(
            "warning: server-side ref {} differs from offline {}",
            result.reference.to_hex(),
            root.address.to_hex()
        );
    }
    st.uploaded_files += 1;
    st.uploaded_bytes += size;
    save(&st)?;

    let trimmed = url.trim_end_matches('/');
    println!("Uploaded → {}", result.reference.to_hex());
    println!("  url: {trimmed}/bytes/{}", result.reference.to_hex());
    print_summary(&st);
    Ok(())
}

fn cmd_stats() -> Result<(), Error> {
    let st = load()?;
    print_summary(&st);
    let max_slot = 1u32 << (st.depth - 16);
    let total_capacity = (max_slot as u64) * (NUM_BUCKETS as u64);
    let total_used: u64 = st.buckets.iter().map(|c| *c as u64).sum();
    let max_height = st.buckets.iter().copied().max().unwrap_or(0);
    let hottest = st
        .buckets
        .iter()
        .enumerate()
        .max_by_key(|(_, c)| **c)
        .map(|(i, _)| i)
        .unwrap_or(0);
    println!("Capacity:      {total_used} / {total_capacity} chunks");
    println!("Hottest bucket: #{hottest:04x} ({max_height} / {max_slot})");
    Ok(())
}

fn cmd_reset() -> Result<(), Error> {
    if PathBuf::from(STATE_FILE).exists() {
        fs::remove_file(STATE_FILE).map_err(|e| Error::argument(format!("rm: {e}")))?;
        println!("Removed {STATE_FILE}");
    } else {
        println!("(nothing to reset)");
    }
    Ok(())
}

fn print_summary(st: &RelayState) {
    println!(
        "Relay: depth={} files_uploaded={} bytes={} files_rejected={}",
        st.depth, st.uploaded_files, st.uploaded_bytes, st.rejected_files
    );
}

fn save(s: &RelayState) -> Result<(), Error> {
    let bytes = serde_json::to_vec_pretty(s)?;
    fs::write(STATE_FILE, bytes).map_err(|e| Error::argument(format!("write: {e}")))
}

fn load() -> Result<RelayState, Error> {
    let bytes = fs::read(STATE_FILE)
        .map_err(|_| Error::argument(format!("{STATE_FILE} not found — run `init` first")))?;
    Ok(serde_json::from_slice(&bytes)?)
}
