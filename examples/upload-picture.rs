//! upload-picture — upload a single file via `POST /bzz`.
//!
//! ```text
//! cargo run --example upload-picture -- <batch-id> [file-path]
//! ```
//!
//! Defaults: file-path = "image.png". Content-type is inferred from the
//! file extension (best effort).
//!
//! Environment overrides:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).

use std::env;
use std::path::Path;
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

    let mut args = env::args().skip(1);
    let batch_hex = args.next().ok_or_else(|| {
        Error::argument(
            "usage: upload-picture <batch-id> [file-path]\n  example: upload-picture 4a2... image.png",
        )
    })?;
    let batch_id = BatchId::from_hex(&batch_hex)?;
    let file_path = args.next().unwrap_or_else(|| "image.png".to_string());

    let data = std::fs::read(&file_path)
        .map_err(|e| Error::argument(format!("failed to open {file_path}: {e}")))?;
    let name = Path::new(&file_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&file_path);
    let content_type = guess_content_type(&file_path);

    println!(
        "Uploading {file_path} ({} bytes, {content_type}) using batch {}...",
        data.len(),
        batch_id.to_hex()
    );

    let client = Client::new(&url)?;
    let result = client
        .file()
        .upload_file(&batch_id, Bytes::from(data), name, content_type, None)
        .await?;

    println!("Upload successful!");
    println!("Reference: {}", result.reference.to_hex());
    let trimmed = url.trim_end_matches('/');
    println!(
        "\nYou can view your picture in the browser at:\n{trimmed}/bzz/{}/",
        result.reference.to_hex()
    );

    Ok(())
}

fn guess_content_type(path: &str) -> &'static str {
    let ext = Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "html" | "htm" => "text/html",
        "txt" => "text/plain",
        "json" => "application/json",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}
