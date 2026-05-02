//! stamp-cost-live — preview the BZZ cost of buying a batch using the
//! live `/chainstate` price. Read-only (no chain TX).
//!
//! ```text
//! cargo run --example stamp-cost-live -- [size] [duration] [network]
//! ```
//!
//! Defaults: size = "1GB", duration = "30d", network = "gnosis"
//! (5s blocks; pass "mainnet" for 15s).
//!
//! Environment overrides:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).

use std::env;
use std::process::ExitCode;

use bee::storage::get_storage_cost;
use bee::swarm::{BeeDuration, Bzz, Network, Size};
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
    let size_str = args.next().unwrap_or_else(|| "1GB".to_string());
    let dur_str = args.next().unwrap_or_else(|| "30d".to_string());
    let net_str = args.next().unwrap_or_else(|| "gnosis".to_string());

    let size = Size::parse(&size_str)?;
    let duration = BeeDuration::parse(&dur_str)?.to_std();
    let network = match net_str.to_ascii_lowercase().as_str() {
        "gnosis" => Network::Gnosis,
        "mainnet" => Network::Mainnet,
        other => {
            return Err(Error::argument(format!(
                "unknown network {other:?} — expected gnosis or mainnet"
            )));
        }
    };

    let client = Client::new(&url)?;
    let chain = client.debug().chain_state().await?;
    let cost = get_storage_cost(&client, size, duration, network).await?;

    let total_bzz = Bzz::from_base_units(cost.total_cost.clone());

    println!("Live stamp cost preview");
    println!("=======================");
    println!("Bee URL:              {url}");
    println!("Size:                 {size_str} ({} bytes)", size.to_bytes());
    println!("Duration:             {dur_str}");
    println!("Network:              {net_str} ({}s blocks)", network.block_time_seconds());
    println!();
    println!("Live chain price:     {} PLUR/chunk/block", chain.current_price);
    println!("Stamp depth:          {}", cost.depth);
    println!("Blocks for duration:  {}", cost.blocks);
    println!("Per-chunk amount:     {} PLUR", cost.amount_per_chunk);
    println!("Total cost:           {} PLUR", cost.total_cost);
    println!("                      {} BZZ", total_bzz.to_decimal_string());
    Ok(())
}
