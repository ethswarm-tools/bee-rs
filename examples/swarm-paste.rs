//! swarm-paste — pipe stdin into a Bee node and print a sharable
//! `/bzz/<ref>` URL. Tiny pastebin-as-a-Swarm-app.
//!
//! ```text
//! echo "hello" | cargo run --example swarm-paste
//! cat report.md | cargo run --example swarm-paste -- --ct text/markdown
//! ```
//!
//! Environment:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BATCH_ID` — usable postage batch (required).

use std::env;
use std::io::Read;
use std::process::ExitCode;

use bee::api::{FileUploadOptions, UploadOptions};
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

    let mut content_type = "text/plain".to_string();
    let mut name = "paste.txt".to_string();
    let mut encrypt = false;
    let mut args = env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--ct" | "--content-type" => {
                content_type = args
                    .next()
                    .ok_or_else(|| Error::argument("--ct needs a value"))?;
            }
            "--name" => {
                name = args
                    .next()
                    .ok_or_else(|| Error::argument("--name needs a value"))?;
            }
            "--encrypt" => encrypt = true,
            "-h" | "--help" => {
                eprintln!(
                    "usage: swarm-paste [--ct <mime>] [--name <filename>] [--encrypt]\n\
                     reads stdin and uploads it as a file. prints the bzz URL on success."
                );
                return Ok(());
            }
            other => return Err(Error::argument(format!("unknown flag: {other}"))),
        }
    }

    let mut buf = Vec::new();
    std::io::stdin()
        .read_to_end(&mut buf)
        .map_err(|e| Error::argument(format!("read stdin: {e}")))?;
    if buf.is_empty() {
        return Err(Error::argument("nothing on stdin"));
    }

    let opts = FileUploadOptions {
        base: UploadOptions {
            encrypt: Some(encrypt),
            ..Default::default()
        },
        content_type: Some(content_type.clone()),
        ..Default::default()
    };
    let client = Client::new(&url)?;
    let result = client
        .file()
        .upload_file(
            &batch_id,
            Bytes::from(buf.clone()),
            &name,
            &content_type,
            Some(&opts),
        )
        .await?;

    let trimmed = url.trim_end_matches('/');
    println!("{} bytes uploaded ({content_type})", buf.len());
    println!("reference: {}", result.reference.to_hex());
    println!("url:       {trimmed}/bzz/{}/", result.reference.to_hex());
    if encrypt {
        println!("note: 64-byte reference contains the decryption key — keep the URL secret.");
    }
    Ok(())
}
