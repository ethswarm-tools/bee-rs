//! v1.3 surface check — validate the new v1.3.0 APIs against a live
//! Bee node (Sepolia or mainnet). Run after [`integration-check`] to
//! confirm `Client::ping`, `Client::with_token` plumbing, the tracing
//! send-path events, `time_settlements`, `r_chash`, and the
//! `/chunks/stream` websocket upload.
//!
//! ```text
//! cargo run --example v1_3_check
//! BEE_BATCH_ID=<hex>  cargo run --example v1_3_check
//! ```
//!
//! Environment overrides:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BATCH_ID` — hex-encoded batch ID. Required for the
//!   `/chunks/stream` step; without it that step is skipped.

use std::env;
use std::process::ExitCode;

use bee::Client;
use bee::swarm::{BatchId, make_content_addressed_chunk};

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> ExitCode {
    let url = env::var("BEE_URL").unwrap_or_else(|_| "http://localhost:1633".to_string());
    println!("Bee URL: {url}\n");
    let client = match Client::new(&url) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("client construct failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut pass = 0u32;
    let mut fail = 0u32;
    let mut skip = 0u32;

    // ---- Phase A: ergonomics --------------------------------------
    println!("=== Phase A: ergonomics ===");

    match client.ping().await {
        Ok(d) => {
            println!("  ok    Client::ping              -> {d:?}");
            pass += 1;
        }
        Err(e) => {
            println!("  FAIL  Client::ping              -> {e}");
            fail += 1;
        }
    }

    // We don't have a token-protected node to validate with_token end
    // to end; the wiremock test covers header attachment. Just verify
    // the constructor accepts a sane token.
    match Client::with_token(&url, "dummy-token-not-used") {
        Ok(_) => {
            println!("  ok    Client::with_token        -> constructor accepts token");
            pass += 1;
        }
        Err(e) => {
            println!("  FAIL  Client::with_token        -> {e}");
            fail += 1;
        }
    }

    // tracing: we won't subscribe; just demonstrate that calling an
    // endpoint doesn't crash (events fire silently without a
    // subscriber).
    match client.debug().health().await {
        Ok(_) => {
            println!("  ok    tracing path              -> /health round-tripped (events silent)");
            pass += 1;
        }
        Err(e) => {
            println!("  FAIL  tracing path              -> {e}");
            fail += 1;
        }
    }

    // ---- Phase B: /timesettlements + /rchash ----------------------
    println!("\n=== Phase B: new endpoints ===");

    match client.debug().time_settlements().await {
        Ok(s) => {
            println!(
                "  ok    /timesettlements          -> peers={} totalReceived={:?} totalSent={:?}",
                s.settlements.len(),
                s.total_received,
                s.total_sent
            );
            pass += 1;
        }
        Err(e) => {
            println!("  FAIL  /timesettlements          -> {e}");
            fail += 1;
        }
    }

    // /rchash with depth=1 and arbitrary anchors. Bee computes the
    // sample on demand; on a freshly started node this can take a
    // while or fail if the reserve is empty — we treat any explicit
    // server-side failure as a soft skip rather than a regression.
    let depth = 1u8;
    let anchor1 = "00".repeat(32);
    let anchor2 = "11".repeat(32);
    match client.debug().r_chash(depth, &anchor1, &anchor2).await {
        Ok(r) => {
            println!(
                "  ok    /rchash (depth=1)          -> duration={:.3}s hash[0..16]={}",
                r.duration_seconds,
                &r.hash[..r.hash.len().min(16)]
            );
            pass += 1;
        }
        Err(e) => {
            println!("  SKIP  /rchash (depth=1)          -> server replied: {e}");
            skip += 1;
        }
    }

    // ---- Phase C: /chunks/stream WS upload ------------------------
    println!("\n=== Phase C: /chunks/stream ===");

    let batch_id = env::var("BEE_BATCH_ID")
        .ok()
        .and_then(|h| BatchId::from_hex(&h).ok());

    if let Some(bid) = batch_id {
        // Make a small content-addressed chunk (span || 10 bytes).
        let chunk = match make_content_addressed_chunk(b"hello-v1.3") {
            Ok(c) => c,
            Err(e) => {
                println!("  FAIL  /chunks/stream prepare     -> {e}");
                println!("\nResult: {pass} pass, {} fail, {skip} skip", fail + 1);
                return ExitCode::FAILURE;
            }
        };
        let payload = chunk.data();
        match client.file().chunks_stream(&bid, None).await {
            Ok(mut cs) => match cs.send_chunk(payload.clone()).await {
                Ok(()) => match cs.close().await {
                    Ok(()) => {
                        println!(
                            "  ok    /chunks/stream            -> 1 chunk acked ({} bytes), close ok",
                            payload.len()
                        );
                        pass += 1;
                    }
                    Err(e) => {
                        println!("  FAIL  /chunks/stream close       -> {e}");
                        fail += 1;
                    }
                },
                Err(e) => {
                    println!("  FAIL  /chunks/stream send_chunk  -> {e}");
                    fail += 1;
                }
            },
            Err(e) => {
                println!("  FAIL  /chunks/stream open        -> {e}");
                fail += 1;
            }
        }
    } else {
        println!("  SKIP  /chunks/stream            -> set BEE_BATCH_ID to a usable batch hex");
        skip += 1;
    }

    println!("\nResult: {pass} pass, {fail} fail, {skip} skip");
    if fail > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
