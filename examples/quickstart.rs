//! quickstart — minimal end-to-end against a live Bee node.
//!
//! Connects to a Bee node, checks health, buys (or reuses) a postage
//! batch, then round-trips a small payload through `/bytes`.
//!
//! ```text
//! cargo run --example quickstart
//! ```
//!
//! Environment overrides:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BATCH_ID` — hex batch ID. When set, the batch is reused;
//!   otherwise a fresh batch is bought (slow on Sepolia — multiple
//!   minutes for first usability).

use std::env;
use std::process::ExitCode;
use std::time::Duration;

use bee::storage::{StorageOptions, buy_storage};
use bee::swarm::{BatchId, Size};
use bee::{Client, Error};
use bytes::Bytes;

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
    let client = Client::new(&url)?;

    // 1. Health.
    let health = client.debug().health().await?;
    println!(
        "connected to bee {} (api {})",
        health.version, health.api_version
    );
    if health.status != "ok" {
        return Err(Error::argument(format!(
            "node is not healthy: status={}",
            health.status
        )));
    }

    // 2. Postage batch — reuse via env or buy fresh.
    let batch_id = match env::var("BEE_BATCH_ID").ok() {
        Some(hex) => {
            let id = BatchId::from_hex(&hex)
                .map_err(|e| Error::argument(format!("invalid BEE_BATCH_ID: {e}")))?;
            println!("reusing batch {}", id.to_hex());
            id
        }
        None => {
            println!("buying a small postage batch (this can take minutes on Sepolia)");
            let size = Size::from_megabytes(16.0)?;
            let duration = Duration::from_secs(24 * 60 * 60);
            buy_storage(&client, size, duration, &StorageOptions::default()).await?
        }
    };

    // 3. Upload + download round-trip.
    let payload = Bytes::from_static(b"Hello Swarm from bee-rs!");
    let result = client
        .file()
        .upload_data(&batch_id, payload.clone(), None)
        .await?;
    println!(
        "uploaded {} bytes -> {}",
        payload.len(),
        result.reference.to_hex()
    );

    let body = client.file().download_data(&result.reference, None).await?;
    if body == payload {
        println!("download matches upload ({} bytes)", body.len());
    } else {
        return Err(Error::argument(format!(
            "round-trip mismatch: uploaded {} bytes, got {}",
            payload.len(),
            body.len()
        )));
    }

    Ok(())
}
