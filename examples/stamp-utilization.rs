//! stamp-utilization — per-bucket fill analysis for one batch. Predicts
//! when the batch will be full. Read-only.
//!
//! ```text
//! cargo run --example stamp-utilization -- <batch-id>
//! ```
//!
//! Environment overrides:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).

use std::env;
use std::process::ExitCode;

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
    let batch_hex = env::args()
        .nth(1)
        .ok_or_else(|| Error::argument("usage: stamp-utilization <batch-id>"))?;
    let batch_id = BatchId::from_hex(&batch_hex)?;

    let client = Client::new(&url)?;
    let buckets = client
        .postage()
        .get_postage_batch_buckets(&batch_id)
        .await?;

    let total_buckets = buckets.buckets.len();
    let mut max_fill = 0u32;
    let mut filled_buckets = 0usize;
    let mut total_chunks = 0u64;
    for b in &buckets.buckets {
        if b.collisions > 0 {
            filled_buckets += 1;
        }
        if b.collisions > max_fill {
            max_fill = b.collisions;
        }
        total_chunks += b.collisions as u64;
    }
    let cap = buckets.bucket_upper_bound;
    let pct_full_buckets = if total_buckets > 0 {
        (filled_buckets as f64 / total_buckets as f64) * 100.0
    } else {
        0.0
    };
    let max_fill_pct = if cap > 0 {
        (max_fill as f64 / cap as f64) * 100.0
    } else {
        0.0
    };

    println!("Stamp utilization for batch {}", batch_id.to_hex());
    println!("=================================================================");
    println!("Depth:                 {}", buckets.depth);
    println!("Bucket depth:          {}", buckets.bucket_depth);
    println!("Per-bucket cap:        {}", cap);
    println!("Total buckets:         {}", total_buckets);
    println!("Buckets used:          {filled_buckets} ({pct_full_buckets:.2}%)");
    println!("Hottest bucket fill:   {max_fill} / {cap} ({max_fill_pct:.2}%)");
    println!("Total chunks stamped:  {total_chunks}");
    println!();
    println!("The hottest bucket determines when the batch becomes full —");
    println!("Bee rejects writes once any bucket hits the cap. A high");
    println!("imbalance is a sign you should dilute (deepen) the batch.");
    Ok(())
}
