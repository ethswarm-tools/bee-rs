//! chain-state — print the chain state Bee currently sees. Read-only.
//!
//! ```text
//! cargo run --example chain-state
//! ```
//!
//! Environment overrides:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).

use std::env;
use std::process::ExitCode;

use bee::swarm::Bzz;
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
    let s = client.debug().chain_state().await?;

    let total_amount_bzz = Bzz::from_base_units(s.total_amount.clone());

    println!("Chain state");
    println!("===========");
    println!("Settled block:    {}", s.block);
    println!("Chain tip:        {}", s.chain_tip);
    println!(
        "Lag:              {} block(s)",
        s.chain_tip.saturating_sub(s.block)
    );
    println!("Current price:    {} PLUR/chunk/block", s.current_price);
    println!("Total amount:     {} PLUR", s.total_amount);
    println!(
        "                  {} BZZ",
        total_amount_bzz.to_decimal_string()
    );
    Ok(())
}
