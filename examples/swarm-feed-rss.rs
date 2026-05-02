//! swarm-feed-rss — read-only aggregator over N Swarm feeds.
//!
//! Configure feeds in `feeds.json`; the tool fetches the latest
//! update from each (and optionally walks recent history). No signer
//! required — feeds are public-by-default; anyone with the
//! `(owner, topic)` pair can read.
//!
//! ```text
//! swarm-feed-rss add  <name> <owner-eth-hex> <topic-string>
//! swarm-feed-rss list
//! swarm-feed-rss latest                       # latest from every feed
//! swarm-feed-rss walk <name> [--last N]       # last N indexes
//! ```
//!
//! Environment:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).

use std::env;
use std::fs;
use std::process::ExitCode;

use bee::file::make_feed_identifier;
use bee::swarm::{EthAddress, Topic};
use bee::{Client, Error};
use serde::{Deserialize, Serialize};

const FEEDS_FILE: &str = "feeds.json";

#[derive(Serialize, Deserialize, Debug, Default)]
struct Config {
    feeds: Vec<Feed>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Feed {
    name: String,
    owner_hex: String,
    topic_string: String,
    topic_hex: String,
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
    let mut args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        return Err(Error::argument(
            "usage: swarm-feed-rss <add|list|latest|walk> ...",
        ));
    }
    let client = Client::new(&url)?;
    let cmd = args.remove(0);
    match cmd.as_str() {
        "add" => cmd_add(&args),
        "list" => cmd_list(),
        "latest" => cmd_latest(&client).await,
        "walk" => cmd_walk(&client, &args).await,
        other => Err(Error::argument(format!("unknown command: {other}"))),
    }
}

fn cmd_add(args: &[String]) -> Result<(), Error> {
    if args.len() < 3 {
        return Err(Error::argument(
            "usage: swarm-feed-rss add <name> <owner-eth-hex> <topic-string>",
        ));
    }
    let name = &args[0];
    let owner_hex = &args[1];
    let topic_string = &args[2];
    let _ = EthAddress::from_hex(owner_hex)?;
    let topic = Topic::from_string(topic_string);

    let mut cfg = load();
    if cfg.feeds.iter().any(|f| f.name == *name) {
        return Err(Error::argument(format!("feed {name} already exists")));
    }
    cfg.feeds.push(Feed {
        name: name.clone(),
        owner_hex: owner_hex.to_lowercase(),
        topic_string: topic_string.clone(),
        topic_hex: topic.to_hex(),
    });
    save(&cfg)?;
    println!("Added feed {name}: owner={owner_hex} topic={topic_string:?}");
    Ok(())
}

fn cmd_list() -> Result<(), Error> {
    let cfg = load();
    if cfg.feeds.is_empty() {
        println!("(no feeds — `swarm-feed-rss add ...`)");
        return Ok(());
    }
    println!("{:<20}  {:<42}  topic", "name", "owner");
    for f in &cfg.feeds {
        println!("{:<20}  {:<42}  {:?}", f.name, f.owner_hex, f.topic_string);
    }
    Ok(())
}

async fn cmd_latest(client: &Client) -> Result<(), Error> {
    let cfg = load();
    if cfg.feeds.is_empty() {
        return Err(Error::argument("no feeds configured"));
    }
    for f in &cfg.feeds {
        println!("=== {} ===", f.name);
        let owner = EthAddress::from_hex(&f.owner_hex)?;
        let topic = Topic::from_hex(&f.topic_hex)?;
        match client.file().fetch_latest_feed_update(&owner, &topic).await {
            Ok(upd) => {
                let ts = decode_ts(&upd.payload);
                let body = body_after_ts(&upd.payload);
                println!(
                    "  index={} index_next={} ts={ts}",
                    upd.index, upd.index_next
                );
                print_payload("  ", body);
            }
            Err(e) => println!("  (no updates: {e})"),
        }
        println!();
    }
    Ok(())
}

async fn cmd_walk(client: &Client, args: &[String]) -> Result<(), Error> {
    if args.is_empty() {
        return Err(Error::argument(
            "usage: swarm-feed-rss walk <name> [--last N]",
        ));
    }
    let name = &args[0];
    let mut last_n: u64 = 5;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--last" => {
                i += 1;
                last_n = args
                    .get(i)
                    .ok_or_else(|| Error::argument("--last needs N"))?
                    .parse()
                    .map_err(|e| Error::argument(format!("invalid N: {e}")))?;
                i += 1;
            }
            other => return Err(Error::argument(format!("unknown flag: {other}"))),
        }
    }
    let cfg = load();
    let f = cfg
        .feeds
        .iter()
        .find(|f| f.name == *name)
        .ok_or_else(|| Error::argument(format!("no feed named {name}")))?;
    let owner = EthAddress::from_hex(&f.owner_hex)?;
    let topic = Topic::from_hex(&f.topic_hex)?;
    let next = client.file().find_next_index(&owner, &topic).await?;
    if next == 0 {
        println!("(empty feed)");
        return Ok(());
    }
    let last = next - 1;
    let from = last.saturating_sub(last_n.saturating_sub(1));
    println!("walking {name} indexes {from}..={last}");
    let reader = client.file().make_soc_reader(owner);
    for i in from..=last {
        let id = make_feed_identifier(&topic, i);
        match reader.download(&id).await {
            Ok(soc) => {
                let ts = decode_ts(&soc.payload);
                let body = body_after_ts(&soc.payload);
                let label = format!("  #{i:<4} ts={ts}");
                print_payload(&label, body);
            }
            Err(e) => println!("  #{i}: missing ({e})"),
        }
    }
    Ok(())
}

fn decode_ts(payload: &[u8]) -> u64 {
    if payload.len() < 8 {
        return 0;
    }
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&payload[..8]);
    u64::from_be_bytes(buf)
}

fn body_after_ts(payload: &[u8]) -> &[u8] {
    if payload.len() < 8 {
        payload
    } else {
        &payload[8..]
    }
}

fn print_payload(prefix: &str, body: &[u8]) {
    match std::str::from_utf8(body) {
        Ok(s) if s.len() <= 200 => println!("{prefix} {s:?}"),
        Ok(s) => println!("{prefix} {:?}…  ({} bytes)", &s[..200], s.len()),
        Err(_) => println!("{prefix} ({} bytes binary)", body.len()),
    }
}

fn load() -> Config {
    fs::read(FEEDS_FILE)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save(c: &Config) -> Result<(), Error> {
    let bytes = serde_json::to_vec_pretty(c)?;
    fs::write(FEEDS_FILE, bytes).map_err(|e| Error::argument(format!("write: {e}")))
}
