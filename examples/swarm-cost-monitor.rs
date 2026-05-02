//! swarm-cost-monitor — operator dashboard: batch TTLs counting
//! down, current chain price, projected refill cost.
//!
//! ```text
//! swarm-cost-monitor                 # one-shot snapshot
//! swarm-cost-monitor watch           # refresh every 30s
//! swarm-cost-monitor refill --days 30
//! ```
//!
//! Environment:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BLOCK_SECONDS` — chain block time (default: `5` Gnosis,
//!   set to `15` for mainnet-like chains).

use std::env;
use std::process::ExitCode;
use std::time::Duration;

use bee::postage::{PostageBatch, get_stamp_cost, get_stamp_usage};
use bee::swarm::Bzz;
use bee::{Client, Error};
use num_bigint::BigInt;
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
    let cmd = args.next().unwrap_or_else(|| "snapshot".into());
    let client = Client::new(&url)?;

    match cmd.as_str() {
        "snapshot" => snapshot(&client).await,
        "watch" => watch(&client).await,
        "refill" => {
            let mut days: f64 = 30.0;
            while let Some(a) = args.next() {
                match a.as_str() {
                    "--days" => {
                        days = args
                            .next()
                            .ok_or_else(|| Error::argument("--days needs N"))?
                            .parse()
                            .map_err(|e| Error::argument(format!("invalid days: {e}")))?;
                    }
                    other => return Err(Error::argument(format!("unknown flag: {other}"))),
                }
            }
            refill(&client, days).await
        }
        other => Err(Error::argument(format!("unknown command: {other}"))),
    }
}

async fn snapshot(client: &Client) -> Result<(), Error> {
    let batches = client.postage().get_postage_batches().await?;
    let chain = client.debug().chain_state().await?;
    let block_secs: u64 = env::var("BEE_BLOCK_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);

    println!(
        "chain: block={} tip={} price={} PLUR/chunk/block",
        chain.block, chain.chain_tip, chain.current_price
    );
    let lag = chain.chain_tip.saturating_sub(chain.block);
    if lag > 0 {
        println!("       (Bee is {lag} blocks behind tip)");
    }

    if batches.is_empty() {
        println!("\n(no batches owned by this node)");
        return Ok(());
    }
    println!(
        "\n{:<10}  {:<6}  {:<10}  {:<14}  {:<14}  label",
        "id8", "depth", "usable", "ttl", "fill"
    );
    for b in &batches {
        let id8 = &b.batch_id.to_hex()[..8];
        let usage = get_stamp_usage(b.utilization, b.depth, b.bucket_depth);
        let ttl = format_ttl(b.batch_ttl);
        println!(
            "{id8:<10}  {:<6}  {:<10}  {:<14}  {:<14.1}%  {}",
            b.depth,
            yes_no(b.usable),
            ttl,
            usage * 100.0,
            b.label
        );
    }
    println!();
    show_warnings(&batches, block_secs);
    Ok(())
}

async fn watch(client: &Client) -> Result<(), Error> {
    loop {
        print!("\x1b[2J\x1b[H");
        snapshot(client).await?;
        sleep(Duration::from_secs(30)).await;
    }
}

async fn refill(client: &Client, days: f64) -> Result<(), Error> {
    let chain = client.debug().chain_state().await?;
    let block_secs: u64 = env::var("BEE_BLOCK_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);
    let blocks = (days * 86_400.0 / block_secs as f64) as u64;
    let batches = client.postage().get_postage_batches().await?;

    println!(
        "Refill projection ({days:.1}d at {} PLUR/chunk/block, {block_secs}s blocks):",
        chain.current_price
    );
    if batches.is_empty() {
        println!("(no batches owned)");
        return Ok(());
    }
    println!(
        "\n{:<10}  {:<6}  {:<14}  {:<22}  refill cost (BZZ)",
        "id8", "depth", "current ttl", "topup amount/chunk"
    );
    let mut total = BigInt::from(0);
    for b in &batches {
        let id8 = &b.batch_id.to_hex()[..8];
        let topup_per_chunk = &chain.current_price * BigInt::from(blocks);
        let topup_total = get_stamp_cost(i32::from(b.depth), &topup_per_chunk);
        let bzz = Bzz::from_base_units(topup_total.clone());
        total += topup_total;
        println!(
            "{id8:<10}  {:<6}  {:<14}  {:<22}  {}",
            b.depth,
            format_ttl(b.batch_ttl),
            topup_per_chunk,
            bzz.to_significant_digits(4),
        );
    }
    let total_bzz = Bzz::from_base_units(total);
    println!(
        "\nTotal projected refill: {} BZZ",
        total_bzz.to_significant_digits(4)
    );
    Ok(())
}

fn show_warnings(batches: &[PostageBatch], block_secs: u64) {
    let mut warned = false;
    for b in batches {
        if b.batch_ttl > 0 && b.batch_ttl < (7 * 86_400) {
            warned = true;
            let days = b.batch_ttl as f64 / 86_400.0;
            println!(
                "WARN batch {} TTL {:.1}d — refill soon",
                &b.batch_id.to_hex()[..8],
                days
            );
        }
        let usage = get_stamp_usage(b.utilization, b.depth, b.bucket_depth);
        if usage > 0.85 {
            warned = true;
            println!(
                "WARN batch {} {:.1}% full — dilute soon",
                &b.batch_id.to_hex()[..8],
                usage * 100.0
            );
        }
    }
    if !warned {
        let _ = block_secs;
        println!("(all batches healthy)");
    }
}

fn format_ttl(secs: i64) -> String {
    if secs < 0 {
        return "n/a".into();
    }
    let s = secs as u64;
    let days = s / 86_400;
    let hours = (s % 86_400) / 3600;
    if days > 0 {
        format!("{days}d{hours:02}h")
    } else {
        format!("{}h{:02}m", s / 3600, (s % 3600) / 60)
    }
}

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}
