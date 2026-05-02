//! swarm-chat — terminal chat over PSS.
//!
//! Each message is a tiny JSON envelope `{user, ts, text}` sent to a
//! topic prefix. The chat client subscribes to the same topic and
//! prints incoming messages while reading stdin lines to send.
//!
//! Bee does NOT deliver a node's own PSS messages back to itself, so
//! a single process won't see what it sent. Run two instances against
//! different Bee URLs (or different `BEE_BATCH_ID`s) and they'll talk.
//!
//! ```text
//! BEE_URL=http://localhost:1633 BEE_BATCH_ID=… \
//!     cargo run --example swarm-chat -- --user alice --topic demo
//! ```
//!
//! Environment:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BATCH_ID` — usable postage batch (required).

use std::env;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use bee::swarm::{BatchId, Topic};
use bee::{Client, Error};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Serialize, Deserialize, Debug)]
struct Envelope {
    user: String,
    ts: u64,
    text: String,
}

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

    let mut user = env::var("USER").unwrap_or_else(|_| "anon".into());
    let mut topic_name = "swarm-chat-default".to_string();
    let mut target = "".to_string();
    let mut args = env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--user" => user = args.next().ok_or_else(|| Error::argument("--user value"))?,
            "--topic" => {
                topic_name = args
                    .next()
                    .ok_or_else(|| Error::argument("--topic value"))?
            }
            "--target" => {
                target = args
                    .next()
                    .ok_or_else(|| Error::argument("--target value"))?
            }
            "-h" | "--help" => {
                println!("swarm-chat [--user <name>] [--topic <name>] [--target <hex-prefix>]");
                return Ok(());
            }
            other => return Err(Error::argument(format!("unknown flag: {other}"))),
        }
    }

    let topic = Topic::from_string(&topic_name);
    let client = Client::new(&url)?;

    println!("Joining {topic_name:?} as {user:?}");
    println!("(target prefix: {target:?})");
    println!("Type a message and press enter to send. Ctrl+D to quit.\n");

    // Receive loop.
    let recv_client = client.clone();
    let recv_topic = topic;
    let recv_user = user.clone();
    tokio::spawn(async move {
        let mut sub = match recv_client.pss().subscribe(&recv_topic).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("subscribe failed: {e}");
                return;
            }
        };
        while let Some(msg) = sub.recv().await {
            match serde_json::from_slice::<Envelope>(&msg) {
                Ok(env) if env.user != recv_user => {
                    println!("[{}] {}", env.user, env.text);
                }
                Ok(_) => {} // own echo (rare; Bee usually drops these)
                Err(_) => println!("[?] ({} bytes binary)", msg.len()),
            }
        }
    });

    // Send loop.
    let mut stdin = BufReader::new(tokio::io::stdin()).lines();
    while let Ok(Some(line)) = stdin.next_line().await {
        if line.is_empty() {
            continue;
        }
        let env = Envelope {
            user: user.clone(),
            ts: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            text: line,
        };
        let body = serde_json::to_vec(&env)?;
        if let Err(e) = client
            .pss()
            .send(&batch_id, &topic, &target, Bytes::from(body), None)
            .await
        {
            eprintln!("send failed: {e}");
        }
    }
    println!("\n(stdin closed, exiting.)");
    Ok(())
}
