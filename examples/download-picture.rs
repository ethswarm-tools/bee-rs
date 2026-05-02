//! download-picture — download a file by its swarm reference.
//!
//! ```text
//! cargo run --example download-picture -- <reference> [output-filename]
//! ```
//!
//! Default output filename: `downloaded.png`.
//!
//! Environment overrides:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).

use std::env;
use std::process::ExitCode;

use bee::swarm::Reference;
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
    let ref_hex = args.next().ok_or_else(|| {
        Error::argument(
            "usage: download-picture <reference> [output-filename]\n  example: download-picture 4a2... downloaded.png",
        )
    })?;
    let reference = Reference::from_hex(&ref_hex)?;
    let output = args.next().unwrap_or_else(|| "downloaded.png".to_string());

    println!("Downloading reference {ref_hex}...");

    let client = Client::new(&url)?;
    let (body, headers) = client.file().download_file(&reference, None).await?;
    let ct = headers.content_type.as_deref().unwrap_or("(none)");
    println!("File found! Content-Type: {ct}");

    std::fs::write(&output, &body)
        .map_err(|e| Error::argument(format!("failed to write {output}: {e}")))?;
    println!(
        "Successfully downloaded and saved to {output} ({} bytes)",
        body.len()
    );
    Ok(())
}
