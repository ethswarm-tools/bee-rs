//! encrypted-upload — upload bytes with `encrypt: true`, observe the
//! 64-byte reference (32 bytes content address + 32 bytes encryption
//! key), and round-trip the data.
//!
//! ```text
//! cargo run --example encrypted-upload
//! ```
//!
//! Environment:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BATCH_ID` — usable postage batch (required).

use std::env;
use std::process::ExitCode;

use bee::api::{RedundantUploadOptions, UploadOptions};
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

    // 1. Plain upload — for comparison. Plain references are 32 bytes (64 hex chars).
    let payload = Bytes::from_static(b"some sensitive payload");
    let plain = client.file().upload_data(&batch_id, payload.clone(), None).await?;
    println!("plain reference:     {} ({} bytes)", plain.reference.to_hex(), plain.reference.len());

    // 2. Encrypted upload — Bee encrypts chunks and bakes the
    //    decryption key into the returned reference. Encrypted refs
    //    are 64 bytes (128 hex chars). Anyone given the reference can
    //    decrypt; without it, the data is opaque.
    let opts = RedundantUploadOptions {
        base: UploadOptions {
            encrypt: Some(true),
            ..Default::default()
        },
        ..Default::default()
    };
    let enc = client.file().upload_data(&batch_id, payload.clone(), Some(&opts)).await?;
    println!(
        "encrypted reference: {} ({} bytes, encrypted={})",
        enc.reference.to_hex(),
        enc.reference.len(),
        enc.reference.is_encrypted()
    );

    // 3. Round-trip — Bee transparently decrypts when given the full
    //    64-byte reference.
    let body = client.file().download_data(&enc.reference, None).await?;
    if body == payload {
        println!("round-trip ok ({} bytes match)", body.len());
    } else {
        return Err(Error::argument("round-trip mismatch"));
    }
    Ok(())
}
