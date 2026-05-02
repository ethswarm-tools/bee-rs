//! list-batches — list every postage batch owned by this Bee node.
//! Read-only.
//!
//! ```text
//! cargo run --example list-batches
//! ```
//!
//! Environment overrides:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).

use std::env;
use std::process::ExitCode;

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
    let client = Client::new(&url)?;

    let batches = client.postage().get_postage_batches().await?;
    if batches.is_empty() {
        println!("No postage batches owned by this node.");
        return Ok(());
    }

    println!("{} batch(es):", batches.len());
    println!();
    println!(
        "{:<64}  {:>5}  {:>9}  {:>6}  {:>11}  {:>9}  {:<8}  label",
        "batch id", "depth", "amount", "usable", "ttl(s)", "util(%)", "immut"
    );
    println!("{}", "-".repeat(140));
    for b in &batches {
        let amount_str = b
            .amount
            .as_ref()
            .map(|a| a.to_string())
            .unwrap_or_else(|| "-".to_string());
        let util_pct = if b.depth > b.bucket_depth {
            let cap = 1u64 << (b.depth - b.bucket_depth);
            ((b.utilization as f64 / cap as f64) * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };
        println!(
            "{:<64}  {:>5}  {:>9}  {:>6}  {:>11}  {:>8.2}%  {:<8}  {}",
            b.batch_id.to_hex(),
            b.depth,
            amount_str,
            b.usable,
            b.batch_ttl,
            util_pct,
            b.immutable,
            b.label
        );
    }
    Ok(())
}
