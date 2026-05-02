//! stamp-cost — pure offline calculator: given a size, a duration, and a
//! per-chunk-per-block price, print the postage stamp depth and total
//! BZZ cost. **No Bee node required.**
//!
//! ```text
//! cargo run --example stamp-cost -- [size] [duration] [price-plur] [network]
//! ```
//!
//! Defaults: size = "1GB", duration = "30d", price = 24000 PLUR/chunk/block,
//! network = "gnosis" (5s blocks; use "mainnet" for 15s).
//!
//! For the live chain price, run a Bee node and use
//! `bee::storage::get_storage_cost` instead — that calls
//! `GET /chainstate` and uses the real per-block price.

use std::process::ExitCode;

use bee::postage::{get_depth_for_size, get_stamp_cost};
use bee::swarm::{BeeDuration, Bzz, Error, Network, Size};
use num_bigint::BigInt;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Error> {
    let mut args = std::env::args().skip(1);
    let size_str = args.next().unwrap_or_else(|| "1GB".to_string());
    let dur_str = args.next().unwrap_or_else(|| "30d".to_string());
    let price_str = args.next().unwrap_or_else(|| "24000".to_string());
    let net_str = args.next().unwrap_or_else(|| "gnosis".to_string());

    let size = Size::parse(&size_str)?;
    let duration = BeeDuration::parse(&dur_str)?;
    let price_per_block: BigInt = price_str
        .parse()
        .map_err(|e| Error::argument(format!("invalid price: {e}")))?;
    let network = match net_str.to_ascii_lowercase().as_str() {
        "gnosis" => Network::Gnosis,
        "mainnet" => Network::Mainnet,
        other => {
            return Err(Error::argument(format!(
                "unknown network {other:?} — use gnosis or mainnet"
            )));
        }
    };

    let depth = get_depth_for_size(size.to_bytes());
    let blocks = network.seconds_to_blocks(duration.to_seconds().max(0) as u64);
    let amount_per_chunk = &price_per_block * BigInt::from(blocks);
    let total_plur = get_stamp_cost(depth, &amount_per_chunk);
    let total_bzz = Bzz::from_base_units(total_plur.clone());

    println!("Stamp cost preview");
    println!("==================");
    println!(
        "Size:                  {size_str} ({} bytes)",
        size.to_bytes()
    );
    println!(
        "Duration:              {dur_str} ({} seconds, ~{:.2} days)",
        duration.to_seconds(),
        duration.to_days()
    );
    println!(
        "Network:               {net_str} ({}s blocks)",
        network.block_time_seconds()
    );
    println!("Price per chunk/block: {price_per_block} PLUR");
    println!();
    println!("Stamp depth:           {depth}");
    println!(
        "Chunks covered:        2^{depth} = {}",
        BigInt::from(2u32).pow(depth as u32)
    );
    println!("Blocks for duration:   {blocks}");
    println!("Per-chunk amount:      {amount_per_chunk} PLUR");
    println!("Total cost:            {total_plur} PLUR");
    println!(
        "                       {} BZZ",
        total_bzz.to_decimal_string()
    );

    Ok(())
}
