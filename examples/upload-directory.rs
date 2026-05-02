//! upload-directory — recursively upload a folder as a Swarm website.
//!
//! ```text
//! cargo run --example upload-directory -- <batch-id> <directory> [index-document]
//! ```
//!
//! Defaults: index-document = "index.html". An empty string ("") omits
//! the index document — the root path then returns 404 instead of
//! serving any file.
//!
//! Once uploaded, the site is served at `/bzz/<reference>/`. Individual
//! files are accessible at `/bzz/<reference>/<path>`.
//!
//! Environment overrides:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).

use std::env;
use std::process::ExitCode;

use bee::api::CollectionUploadOptions;
use bee::swarm::BatchId;
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
    let batch_hex = args.next().ok_or_else(|| {
        Error::argument(
            "usage: upload-directory <batch-id> <directory> [index-document]\n  example: upload-directory 4a2... ./public index.html",
        )
    })?;
    let batch_id = BatchId::from_hex(&batch_hex)?;
    let dir = args
        .next()
        .ok_or_else(|| Error::argument("missing <directory>"))?;
    let index = args.next().unwrap_or_else(|| "index.html".to_string());

    let opts = CollectionUploadOptions {
        index_document: if index.is_empty() { None } else { Some(index.clone()) },
        ..Default::default()
    }
    .with_on_entry(|p| {
        println!(
            "  [{:>3}/{:<3}] {} ({} bytes)",
            p.index + 1,
            p.total,
            p.path,
            p.size
        );
    });

    println!("Uploading directory {dir} using batch {}...", batch_id.to_hex());
    if !index.is_empty() {
        println!("Index document: {index}");
    }
    println!();

    let client = Client::new(&url)?;
    let result = client
        .file()
        .upload_collection(&batch_id, &dir, Some(&opts))
        .await?;

    let trimmed = url.trim_end_matches('/');
    println!("\nUpload successful!");
    println!("Reference: {}", result.reference.to_hex());
    println!("Browse at: {trimmed}/bzz/{}/", result.reference.to_hex());

    Ok(())
}
