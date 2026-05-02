//! pss-send-receive — listen for PSS messages on a topic, or send one.
//!
//! ```text
//! # listen
//! cargo run --example pss-send-receive -- listen <topic>
//!
//! # send (separate process / different Bee node)
//! cargo run --example pss-send-receive -- send <topic> <target-prefix> <message>
//! ```
//!
//! `<topic>` is a UTF-8 string; it is hashed via keccak256 to a 32-byte
//! topic identifier (matching `Topic.fromString` semantics).
//!
//! `<target-prefix>` is a short hex string Bee uses as a routing
//! prefix (e.g. `"0001"`). PSS does not require knowing the recipient's
//! full overlay; any node whose overlay starts with the prefix will
//! deliver the message.
//!
//! Note: a Bee node does NOT receive its own PSS messages — for a real
//! demo, run the listener on one node and the sender on another. Two
//! nodes pointed at different `BEE_URL`s, same topic.
//!
//! Environment:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BATCH_ID` — required for `send` (any usable batch).

use std::env;
use std::process::ExitCode;

use bee::swarm::{BatchId, Topic};
use bee::{Client, Error};
use bytes::Bytes;

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
    let mode = args
        .next()
        .ok_or_else(|| Error::argument("usage: pss-send-receive <listen|send> ..."))?;
    let client = Client::new(&url)?;

    match mode.as_str() {
        "listen" => {
            let topic_str = args
                .next()
                .ok_or_else(|| Error::argument("usage: pss-send-receive listen <topic>"))?;
            let topic = Topic::from_string(&topic_str);
            println!(
                "Subscribing to topic {:?} ({}) on {url}...",
                topic_str,
                topic.to_hex()
            );
            println!("Press Ctrl+C to stop.\n");

            let mut sub = client.pss().subscribe(&topic).await?;
            while let Some(msg) = sub.recv().await {
                match std::str::from_utf8(&msg) {
                    Ok(s) => println!("[{} bytes] {s:?}", msg.len()),
                    Err(_) => println!("[{} bytes] (binary)", msg.len()),
                }
            }
            println!("Subscription closed.");
            Ok(())
        }
        "send" => {
            let topic_str = args.next().ok_or_else(|| {
                Error::argument("usage: pss-send-receive send <topic> <target-prefix> <message>")
            })?;
            let target = args
                .next()
                .ok_or_else(|| Error::argument("missing <target-prefix>"))?;
            let message = args
                .next()
                .ok_or_else(|| Error::argument("missing <message>"))?;
            let batch_hex = env::var("BEE_BATCH_ID").map_err(|_| {
                Error::argument("BEE_BATCH_ID is required for send (set to a usable batch hex id)")
            })?;
            let batch_id = BatchId::from_hex(&batch_hex)?;
            let topic = Topic::from_string(&topic_str);

            println!("Sending PSS message");
            println!("- URL:    {url}");
            println!("- Topic:  {} (from {topic_str:?})", topic.to_hex());
            println!("- Target: {target}");
            println!("- Batch:  {}", batch_id.to_hex());
            println!("- Body:   {message:?}\n");

            client
                .pss()
                .send(&batch_id, &topic, &target, Bytes::from(message), None)
                .await?;
            println!("Message sent.");
            Ok(())
        }
        other => Err(Error::argument(format!(
            "unknown mode {other:?} — expected listen or send"
        ))),
    }
}
