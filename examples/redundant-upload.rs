//! redundant-upload — upload data with erasure-coded redundancy. The
//! upload pays for extra parity chunks so the data survives even when
//! some hosting nodes go offline.
//!
//! ```text
//! cargo run --example redundant-upload -- [level]
//! ```
//!
//! `level` is `off | medium | strong | insane | paranoid`. Defaults
//! to `medium`. Higher levels add more parity chunks (and stamp cost).
//!
//! Environment:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BATCH_ID` — usable postage batch (required).

use std::env;
use std::process::ExitCode;

use bee::api::{RedundancyLevel, RedundantUploadOptions};
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

    let level_str = env::args().nth(1).unwrap_or_else(|| "medium".to_string());
    let level = match level_str.to_ascii_lowercase().as_str() {
        "off" => RedundancyLevel::Off,
        "medium" => RedundancyLevel::Medium,
        "strong" => RedundancyLevel::Strong,
        "insane" => RedundancyLevel::Insane,
        "paranoid" => RedundancyLevel::Paranoid,
        other => {
            return Err(Error::argument(format!(
                "unknown level {other:?} — expected off|medium|strong|insane|paranoid"
            )));
        }
    };

    // ~256 KB payload to make the parity overhead visible.
    let payload = Bytes::from(vec![0xa5u8; 256 * 1024]);

    let client = Client::new(&url)?;

    let plain = client
        .file()
        .upload_data(&batch_id, payload.clone(), None)
        .await?;
    println!("Plain upload (off):       {}", plain.reference.to_hex());

    let opts = RedundantUploadOptions {
        redundancy_level: Some(level),
        ..Default::default()
    };
    let redundant = client
        .file()
        .upload_data(&batch_id, payload.clone(), Some(&opts))
        .await?;
    println!(
        "Redundant upload ({:>8}): {}",
        level_str,
        redundant.reference.to_hex()
    );

    println!();
    println!(
        "Payload size: {} bytes. Higher redundancy levels stamp more",
        payload.len()
    );
    println!("parity chunks; the reference is the same ‘visible’ root.");
    Ok(())
}
