//! feed-update — sign and publish a feed update, then read it back.
//!
//! A Swarm feed is a mutable pointer keyed on `(owner, topic)`. Each
//! update is signed by the owner and indexed sequentially; readers
//! ask for the latest update without knowing the index ahead of time.
//!
//! ```text
//! cargo run --example feed-update -- <topic-string> <message>
//! ```
//!
//! Environment:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BATCH_ID` — usable postage batch (required).
//! - `BEE_SIGNER_HEX` — 32-byte hex private key (required). To generate
//!   one, run `openssl rand -hex 32`. Re-using the same signer + topic
//!   updates the same feed; a new signer creates a new feed.

use std::env;
use std::process::ExitCode;

use bee::swarm::{BatchId, PrivateKey, Topic};
use bee::{Client, Error};

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
    let topic_str = args
        .next()
        .ok_or_else(|| Error::argument("usage: feed-update <topic-string> <message>"))?;
    let message = args
        .next()
        .ok_or_else(|| Error::argument("missing <message>"))?;

    let batch_hex = env::var("BEE_BATCH_ID")
        .map_err(|_| Error::argument("BEE_BATCH_ID is required (set to a usable batch hex id)"))?;
    let batch_id = BatchId::from_hex(&batch_hex)?;

    let signer_hex = env::var("BEE_SIGNER_HEX").map_err(|_| {
        Error::argument(
            "BEE_SIGNER_HEX is required (32-byte hex). Generate one with `openssl rand -hex 32`.",
        )
    })?;
    let signer = PrivateKey::from_hex(&signer_hex)?;
    let owner = signer.public_key()?.address();

    let topic = Topic::from_string(&topic_str);

    println!("Feed parameters:");
    println!("- Owner:  {}", owner.to_hex());
    println!("- Topic:  {} (from \"{topic_str}\")", topic.to_hex());
    println!("- Batch:  {}\n", batch_id.to_hex());

    let client = Client::new(&url)?;

    println!("Publishing feed update with payload {:?}...", message);
    let result = client
        .file()
        .update_feed(&batch_id, &signer, &topic, message.as_bytes())
        .await?;
    println!("  chunk reference: {}", result.reference.to_hex());

    println!("\nFetching latest feed update...");
    let update = client
        .file()
        .fetch_latest_feed_update(&owner, &topic)
        .await?;
    let payload = &update.payload;

    // Wire format: BE-uint64(timestamp) || data
    if payload.len() < 8 {
        return Err(Error::argument(format!(
            "unexpected feed payload length: {}",
            payload.len()
        )));
    }
    let mut ts_bytes = [0u8; 8];
    ts_bytes.copy_from_slice(&payload[..8]);
    let timestamp = u64::from_be_bytes(ts_bytes);
    let data = &payload[8..];

    println!("  index:      {}", update.index);
    println!("  index_next: {}", update.index_next);
    println!("  timestamp:  {timestamp} (unix seconds)");
    match std::str::from_utf8(data) {
        Ok(s) => println!("  payload:    {s:?}"),
        Err(_) => println!("  payload:    {} bytes (non-utf8)", data.len()),
    }

    Ok(())
}
