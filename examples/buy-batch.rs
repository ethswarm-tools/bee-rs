//! buy-batch — buy a postage batch and poll until it becomes usable.
//!
//! ```text
//! cargo run --example buy-batch -- [amount] [depth] [label]
//! ```
//!
//! Defaults: amount = 10000000, depth = 20, label = "bee-rs-example-batch".
//!
//! Note: requires a node with a funded chequebook (BZZ + native gas).
//! On Sepolia first usability typically takes 2-3 minutes. Press
//! Ctrl+C to stop polling — the batch is yours regardless.
//!
//! Environment overrides:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).

use std::env;
use std::process::ExitCode;
use std::time::Duration;

use bee::{Client, Error};
use num_bigint::BigInt;
use tokio::io::AsyncWriteExt;
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

    let mut args = env::args().skip(1);
    let amount: BigInt = args
        .next()
        .as_deref()
        .unwrap_or("10000000")
        .parse()
        .map_err(|e| Error::argument(format!("invalid amount: {e}")))?;
    let depth: u8 = args
        .next()
        .as_deref()
        .unwrap_or("20")
        .parse()
        .map_err(|e| Error::argument(format!("invalid depth: {e}")))?;
    let label = args
        .next()
        .unwrap_or_else(|| "bee-rs-example-batch".to_string());

    println!("Buying a postage batch...");
    println!("- Amount: {amount}");
    println!("- Depth: {depth}");
    println!("- Label: {label}\n");

    let client = Client::new(&url)?;
    let batch_id = client
        .postage()
        .create_postage_batch(&amount, depth, Some(&label))
        .await?;

    println!("Successfully bought batch!");
    println!("Batch ID: {}", batch_id.to_hex());
    println!(
        "\nNote: it takes a few minutes for the chain to confirm and the batch to become usable."
    );

    println!("Polling until the batch is usable... (Ctrl+C to cancel)");
    let mut stdout = tokio::io::stdout();
    loop {
        if let Ok(b) = client.postage().get_postage_batch(&batch_id).await {
            if b.usable {
                println!("\nSuccess! Batch {} is now usable.", batch_id.to_hex());
                return Ok(());
            }
        }
        let _ = stdout.write_all(b".").await;
        let _ = stdout.flush().await;
        sleep(Duration::from_secs(5)).await;
    }
}
