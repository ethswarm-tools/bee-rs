//! stamper-client-side — produce postage stamps offline using a
//! client-side `Stamper`. Useful for batch-stamping pipelines, gas-free
//! re-stamping, or delegating stamp generation to a service that holds
//! the signing key. Bee-rs persists per-bucket state so a stamper can
//! be paused, persisted, and resumed without overspending a bucket.
//!
//! ```text
//! cargo run --example stamper-client-side
//! ```
//!
//! Environment:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BATCH_ID` — usable postage batch (required).
//! - `BEE_SIGNER_HEX` — 32-byte hex private key (required). Signs the
//!   stamp; in production this is the batch issuer's key.

use std::env;
use std::process::ExitCode;

use bee::postage::{Stamper, convert_envelope_to_marshaled_stamp};
use bee::swarm::{BatchId, PrivateKey, make_content_addressed_chunk};
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
    let batch_hex =
        env::var("BEE_BATCH_ID").map_err(|_| Error::argument("BEE_BATCH_ID is required"))?;
    let batch_id = BatchId::from_hex(&batch_hex)?;
    let signer_hex =
        env::var("BEE_SIGNER_HEX").map_err(|_| Error::argument("BEE_SIGNER_HEX is required"))?;
    let signer = PrivateKey::from_hex(&signer_hex)?;

    let client = Client::new(&url)?;

    // Look up the batch's depth so the stamper buckets match the
    // on-chain batch capacity.
    let batch = client.postage().get_postage_batch(&batch_id).await?;
    println!("Batch:");
    println!("  id:    {}", batch.batch_id.to_hex());
    println!("  depth: {}", batch.depth);
    println!("  usable:{}\n", batch.usable);

    // 1. Fresh stamper.
    let mut stamper = Stamper::from_blank(signer.clone(), batch_id, batch.depth)?;
    println!("Stamper:");
    println!("  depth:    {}", stamper.depth());
    println!("  max_slot: {}\n", stamper.max_slot());

    // 2. Stamp three CAC chunks of varying payloads.
    let payloads: [&[u8]; 3] = [b"alpha", b"beta", b"gamma"];
    for p in payloads {
        let chunk = make_content_addressed_chunk(p)?;
        let env = stamper.stamp(chunk.address.as_bytes())?;
        let wire = convert_envelope_to_marshaled_stamp(&env)?;
        println!("  payload {p:?}");
        println!("    chunk_addr: {}", chunk.address.to_hex());
        println!("    issuer:     {}", env.issuer.to_hex());
        println!("    index:      {}", hex::encode(env.index));
        println!("    timestamp:  {}", hex::encode(env.timestamp));
        println!("    stamp(113): {}\n", hex::encode(wire));
    }

    // 3. Demonstrate persist + resume. Counters survive across
    //    restarts so the resumed stamper does not overspend a bucket.
    let snapshot: Vec<u32> = stamper.state().to_vec();
    let used: u32 = snapshot.iter().sum();
    println!(
        "Persisted state: {} buckets, {} stamps total.",
        snapshot.len(),
        used
    );

    let mut resumed = Stamper::from_state(signer, batch_id, snapshot.clone(), batch.depth)?;
    let chunk = make_content_addressed_chunk(b"after resume")?;
    let env = resumed.stamp(chunk.address.as_bytes())?;
    println!("Resumed stamper produced fresh stamp:");
    println!("  chunk_addr: {}", chunk.address.to_hex());
    println!("  index:      {}", hex::encode(env.index));
    println!(
        "  total stamps now: {}",
        resumed.state().iter().sum::<u32>()
    );
    Ok(())
}
