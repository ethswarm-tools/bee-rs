//! basic-usage — health and node-info round-trip.
//!
//! ```text
//! cargo run --example basic-usage
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

    // 1. Health.
    println!("Checking node health...");
    let health = client.debug().health().await?;
    println!(
        "Node Health: status={} version={} api={}\n",
        health.status, health.version, health.api_version
    );

    // 2. Node info.
    println!("Fetching node information...");
    let info = client.debug().node_info().await?;
    println!("Node Information:");
    println!("- Bee Mode: {}", info.bee_mode);
    if info.swap_enabled {
        println!("- SWAP is enabled");
    } else {
        println!("- SWAP is not enabled");
    }
    if info.chequebook_enabled {
        println!("- Chequebook is enabled");
    } else {
        println!("- Chequebook is not enabled");
    }

    Ok(())
}
