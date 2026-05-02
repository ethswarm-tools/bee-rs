//! tag-upload-progress — track an upload's network-sync progress with
//! a Swarm tag. Demonstrates the `Swarm-Tag` header pattern.
//!
//! ```text
//! cargo run --example tag-upload-progress
//! ```
//!
//! Environment:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BATCH_ID` — usable postage batch (required).

use std::env;
use std::process::ExitCode;
use std::time::Duration;

use bee::api::{RedundantUploadOptions, UploadOptions};
use bee::swarm::BatchId;
use bee::{Client, Error};
use bytes::Bytes;
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
    let batch_hex =
        env::var("BEE_BATCH_ID").map_err(|_| Error::argument("BEE_BATCH_ID is required"))?;
    let batch_id = BatchId::from_hex(&batch_hex)?;

    let client = Client::new(&url)?;

    // 1. Create a tag.
    let tag = client.api().create_tag().await?;
    println!("created tag uid={}", tag.uid);

    // 2. Upload with the tag attached, deferred sync (so we can watch
    //    the tag advance after the upload returns).
    let payload = Bytes::from(vec![0xc4u8; 1024 * 1024]); // 1 MB
    let opts = RedundantUploadOptions {
        base: UploadOptions {
            tag: tag.uid,
            deferred: Some(true),
            ..Default::default()
        },
        ..Default::default()
    };
    let result = client
        .file()
        .upload_data(&batch_id, payload.clone(), Some(&opts))
        .await?;
    println!(
        "upload accepted ({} bytes) → {}",
        payload.len(),
        result.reference.to_hex()
    );

    // 3. Poll the tag a few times so we can see the counters move.
    println!("\npolling tag every 2s for 10s:");
    println!(
        "  {:>5}  {:>6}  {:>6}  {:>6}  {:>6}",
        "split", "seen", "stored", "sent", "synced"
    );
    for _ in 0..5 {
        let t = client.api().get_tag(tag.uid).await?;
        println!(
            "  {:>5}  {:>6}  {:>6}  {:>6}  {:>6}",
            t.split, t.seen, t.stored, t.sent, t.synced
        );
        sleep(Duration::from_secs(2)).await;
    }
    Ok(())
}
