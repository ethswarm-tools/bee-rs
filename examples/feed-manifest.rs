//! feed-manifest — create a feed manifest, then publish a couple of
//! feed updates pointing at different content. Visiting
//! `/bzz/{feed-manifest-ref}/` always returns the latest update — the
//! URL stays stable across updates.
//!
//! ```text
//! cargo run --example feed-manifest -- <topic-string>
//! ```
//!
//! Environment:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BATCH_ID` — usable postage batch (required).
//! - `BEE_SIGNER_HEX` — 32-byte hex private key (required).

use std::env;
use std::process::ExitCode;
use std::time::Duration;

use bee::swarm::{BatchId, PrivateKey, Topic};
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
    let topic_str = env::args()
        .nth(1)
        .ok_or_else(|| Error::argument("usage: feed-manifest <topic-string>"))?;
    let batch_hex =
        env::var("BEE_BATCH_ID").map_err(|_| Error::argument("BEE_BATCH_ID is required"))?;
    let batch_id = BatchId::from_hex(&batch_hex)?;
    let signer_hex =
        env::var("BEE_SIGNER_HEX").map_err(|_| Error::argument("BEE_SIGNER_HEX is required"))?;
    let signer = PrivateKey::from_hex(&signer_hex)?;
    let owner = signer.public_key()?.address();
    let topic = Topic::from_string(&topic_str);

    let client = Client::new(&url)?;

    println!(
        "Creating feed manifest for (owner={}, topic={})...",
        owner.to_hex(),
        topic.to_hex()
    );
    let feed_manifest = client
        .file()
        .create_feed_manifest(&batch_id, &owner, &topic)
        .await?;
    println!("Feed manifest reference: {}", feed_manifest.to_hex());

    let trimmed = url.trim_end_matches('/');
    let stable_url = format!("{trimmed}/bzz/{}/", feed_manifest.to_hex());
    println!("Stable URL (always latest): {stable_url}\n");

    // Publish v1.
    let v1 = Bytes::from_static(b"version 1: hello world");
    let r1 = client
        .file()
        .upload_data(&batch_id, v1.clone(), None)
        .await?;
    let upd1 = client
        .file()
        .update_feed_with_reference(&batch_id, &signer, &topic, &r1.reference, None)
        .await?;
    println!("v1 content uploaded → {}", r1.reference.to_hex());
    println!("v1 feed update      → {}", upd1.reference.to_hex());

    // Publish v2.
    sleep(Duration::from_secs(1)).await;
    let v2 = Bytes::from_static(b"version 2: hello swarm");
    let r2 = client
        .file()
        .upload_data(&batch_id, v2.clone(), None)
        .await?;
    let upd2 = client
        .file()
        .update_feed_with_reference(&batch_id, &signer, &topic, &r2.reference, None)
        .await?;
    println!("\nv2 content uploaded → {}", r2.reference.to_hex());
    println!("v2 feed update      → {}", upd2.reference.to_hex());

    println!("\nAt {stable_url}, Bee resolves the feed pointer and serves");
    println!("the LATEST update's content. The URL is stable across updates;");
    println!("readers never need to know the per-update reference.");
    Ok(())
}
