//! pinning-workflow — full pin lifecycle: upload → pin → list →
//! is_retrievable → reupload → unpin → re-pin.
//!
//! ```text
//! cargo run --example pinning-workflow
//! ```
//!
//! Environment:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BATCH_ID` — usable postage batch (required).

use std::env;
use std::process::ExitCode;

use bee::swarm::BatchId;
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
    let batch_hex =
        env::var("BEE_BATCH_ID").map_err(|_| Error::argument("BEE_BATCH_ID is required"))?;
    let batch_id = BatchId::from_hex(&batch_hex)?;

    let client = Client::new(&url)?;

    // 1. Upload some bytes (no pin flag yet).
    let payload = Bytes::from_static(b"hello pinning workflow");
    let result = client.file().upload_data(&batch_id, payload, None).await?;
    let reference = result.reference;
    println!("1. uploaded → {}", reference.to_hex());

    // 2. Pin the reference.
    client.api().pin(&reference).await?;
    println!("2. pinned");

    // 3. Confirm via list_pins and get_pin.
    let pinned = client.api().get_pin(&reference).await?;
    let pins = client.api().list_pins().await?;
    println!(
        "3. get_pin = {pinned}; list_pins has {} pin(s) (this one included: {})",
        pins.len(),
        pins.iter().any(|r| r == &reference)
    );

    // 4. Stewardship check — is the reference retrievable from the network?
    let retrievable = client.api().is_retrievable(&reference).await?;
    println!("4. is_retrievable = {retrievable}");

    // 5. Re-upload pinned data — useful when chunks have fallen off
    //    other nodes and you want to push them back into the network.
    client.api().reupload(&reference, &batch_id).await?;
    println!("5. reuploaded pinned data");

    // 6. Unpin.
    client.api().unpin(&reference).await?;
    let after = client.api().get_pin(&reference).await?;
    println!("6. unpinned; get_pin now = {after}");

    // 7. Re-pin (idempotent — pin twice, unpin once removes).
    client.api().pin(&reference).await?;
    println!("7. re-pinned");
    Ok(())
}
