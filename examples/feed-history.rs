//! feed-history — write three feed updates, then walk indexes 0..N
//! and print the timeline.
//!
//! ```text
//! cargo run --example feed-history -- <topic-string>
//! ```
//!
//! Environment:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BATCH_ID` — usable postage batch (required).
//! - `BEE_SIGNER_HEX` — 32-byte hex private key (required).

use std::env;
use std::process::ExitCode;
use std::time::Duration;

use bee::file::make_feed_identifier;
use bee::swarm::{BatchId, PrivateKey, Topic};
use bee::{Client, Error};
use tokio::time::sleep;

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
    let topic_str = env::args()
        .nth(1)
        .ok_or_else(|| Error::argument("usage: feed-history <topic-string>"))?;
    let batch_hex =
        env::var("BEE_BATCH_ID").map_err(|_| Error::argument("BEE_BATCH_ID is required"))?;
    let batch_id = BatchId::from_hex(&batch_hex)?;
    let signer_hex =
        env::var("BEE_SIGNER_HEX").map_err(|_| Error::argument("BEE_SIGNER_HEX is required"))?;
    let signer = PrivateKey::from_hex(&signer_hex)?;
    let owner = signer.public_key()?.address();
    let topic = Topic::from_string(&topic_str);

    let client = Client::new(&url)?;

    // 1. Determine the next index for this feed (0 if empty).
    let start = client.file().find_next_index(&owner, &topic).await?;
    println!("Starting feed write at index {start} (existing latest+1).");

    // 2. Write three updates, sleeping briefly so timestamps differ.
    for i in 0..3u64 {
        let idx = start + i;
        let payload = format!("entry #{i} at index {idx}");
        client
            .file()
            .update_feed_with_index(&batch_id, &signer, &topic, idx, payload.as_bytes())
            .await?;
        println!("  wrote index {idx}: {payload:?}");
        sleep(Duration::from_secs(1)).await;
    }
    let last = start + 2;

    // 3. Walk every index from 0 to `last` and pretty-print.
    println!("\nFeed history (indexes 0..={last}):");
    let reader = client.file().make_soc_reader(owner);
    for i in 0..=last {
        let id = make_feed_identifier(&topic, i);
        match reader.download(&id).await {
            Ok(soc) => {
                if soc.payload.len() < 8 {
                    println!("  index {i:>3}: <bad payload>");
                    continue;
                }
                let mut ts_bytes = [0u8; 8];
                ts_bytes.copy_from_slice(&soc.payload[..8]);
                let ts = u64::from_be_bytes(ts_bytes);
                let body = &soc.payload[8..];
                match std::str::from_utf8(body) {
                    Ok(s) => println!("  index {i:>3}  ts={ts}  {s:?}"),
                    Err(_) => println!("  index {i:>3}  ts={ts}  ({} bytes binary)", body.len()),
                }
            }
            Err(e) => println!("  index {i:>3}: missing ({e})"),
        }
    }
    Ok(())
}
