//! status — operational snapshot + readiness flag.
//!
//! ```text
//! cargo run --example status
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

    println!("Checking node status...");
    let s = client.debug().status().await?;
    println!("Node Status Output:");
    println!("- Overlay: {}", s.overlay);
    println!("- Bee Mode: {}", s.bee_mode);
    println!("- Connected Peers: {}", s.connected_peers);
    println!("- Reserve Size: {}", s.reserve_size);
    println!("- Pullsync Rate: {:.4}", s.pullsync_rate);
    println!("- Is Reachable: {}", s.is_reachable);
    println!("- Is Warming Up: {}", s.is_warming_up);

    println!("\nChecking node readiness...");
    let ready = client.debug().readiness().await?;
    if ready {
        println!("Node Readiness: Ready");
    } else {
        println!("Node Readiness: Not Ready");
    }

    Ok(())
}
