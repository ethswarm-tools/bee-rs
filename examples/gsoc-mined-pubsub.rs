//! gsoc-mined-pubsub — demonstrate Generic SOC pub/sub:
//!
//! 1. Read the local node's overlay address.
//! 2. Mine a signer (PoW-style) so the SOC address
//!    `keccak256(identifier || signer.address)` lands in that overlay's
//!    neighbourhood.
//! 3. Open a websocket subscription on that SOC address.
//! 4. Send three GSOC messages with the mined signer.
//! 5. Receive them on the subscription side.
//!
//! ```text
//! cargo run --example gsoc-mined-pubsub
//! ```
//!
//! Environment:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BATCH_ID` — usable postage batch (required).

use std::env;
use std::process::ExitCode;
use std::time::Duration;

use bee::swarm::{BatchId, Identifier, gsoc_mine};
use bee::{Client, Error};
use tokio::time::{sleep, timeout};

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

    let client = Client::new(&url)?;

    // 1. Local node's overlay (where chunks land).
    let addresses = client.debug().addresses().await?;
    let overlay_bytes = hex::decode(&addresses.overlay)
        .map_err(|e| Error::argument(format!("invalid overlay hex: {e}")))?;
    println!("Node overlay: {}", addresses.overlay);

    // 2. Pick an identifier and mine a signer with proximity 8 so the
    //    SOC address shares the first byte with the overlay. Mining
    //    is brute-force over a 2-byte counter; proximity 8 hits in
    //    a few hundred iterations on average.
    let identifier = Identifier::from_string("demo-channel");
    let proximity = 8u32;
    println!(
        "Mining GSOC signer for identifier {} at proximity {proximity}...",
        identifier.to_hex()
    );
    let signer = gsoc_mine(&overlay_bytes, &identifier, proximity)?;
    let owner = signer.public_key()?.address();
    let soc_addr = client.gsoc().soc_address(&identifier, &owner)?;
    println!("  signer.address: {}", owner.to_hex());
    println!("  soc_address:    {}\n", soc_addr.to_hex());

    // 3. Subscribe before sending so we don't miss anything.
    let mut sub = client.gsoc().subscribe(&owner, &identifier).await?;

    // 4. Send three messages from a background task.
    let messages = vec![
        b"hello gsoc".to_vec(),
        b"second message".to_vec(),
        b"third and last".to_vec(),
    ];
    let send_client = client.clone();
    let send_batch = batch_id.clone();
    let send_signer = signer.clone();
    let send_id = identifier.clone();
    let send_messages = messages.clone();
    let sender = tokio::spawn(async move {
        // Tiny stagger so the receiver is definitely subscribed.
        sleep(Duration::from_millis(500)).await;
        for (i, msg) in send_messages.iter().enumerate() {
            send_client
                .gsoc()
                .send(&send_batch, &send_signer, &send_id, msg, None)
                .await?;
            println!("  -> sent #{i}: {} bytes", msg.len());
            // Each send goes to the same SOC address (same
            // identifier + signer), so a new put *overwrites* the
            // previous one in Bee's local store. The websocket
            // notifier needs a moment to fire before the next put;
            // bursts under ~500ms get coalesced and the subscriber
            // sees only the last.
            sleep(Duration::from_millis(1500)).await;
        }
        Ok::<_, Error>(())
    });

    // 5. Receive.
    println!("Listening for {} messages...", messages.len());
    for i in 0..messages.len() {
        match timeout(Duration::from_secs(15), sub.recv()).await {
            Ok(Some(msg)) => {
                let expected = &messages[i];
                let ok = msg.as_ref() == expected.as_slice();
                match std::str::from_utf8(&msg) {
                    Ok(s) => println!("  <- recv #{i}: {s:?} (ok={ok})"),
                    Err(_) => println!("  <- recv #{i}: ({} bytes binary) (ok={ok})", msg.len()),
                }
                if !ok {
                    return Err(Error::argument(format!(
                        "message #{i} mismatch: got {} bytes, expected {}",
                        msg.len(),
                        expected.len()
                    )));
                }
            }
            Ok(None) => return Err(Error::argument("subscription closed early")),
            Err(_) => return Err(Error::argument(format!("timeout waiting for message #{i}"))),
        }
    }

    sender.await.map_err(|e| Error::argument(format!("sender task: {e}")))??;
    sub.cancel();
    println!("\nGSOC round-trip OK: {} messages received.", messages.len());
    Ok(())
}
