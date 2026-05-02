//! swarm-share — revocable file sharing on Swarm.
//!
//! `share <file> --to <pubkey>...` uploads the file under an Access
//! Control Trie (ACT), creates a grantee list with the provided
//! recipient public keys, and prints the references the recipients
//! need to download. `revoke` patches the grantee list to drop a key
//! without re-uploading the file.
//!
//! ```text
//! swarm-share share  <file>  --to <pubkey>...
//! swarm-share list
//! swarm-share revoke <id> --grantee <pubkey>
//! swarm-share grantees <id>
//! ```
//!
//! Environment:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BATCH_ID` — usable postage batch (required).

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use bee::api::{FileUploadOptions, UploadOptions};
use bee::swarm::{BatchId, Reference};
use bee::{Client, Error};
use bytes::Bytes;
use serde::{Deserialize, Serialize};

const STATE_FILE: &str = ".swarm-share.json";

#[derive(Serialize, Deserialize, Debug, Default)]
struct Shares {
    shares: Vec<Share>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Share {
    id: String,
    file: String,
    file_ref: String,
    history_address: String,
    grantee_ref: String,
    grantee_history: String,
    grantees: Vec<String>,
    ts: u64,
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
            "usage: swarm-share <share|list|revoke|grantees> ...",
        ));
    }
    let client = Client::new(&url)?;
    let cmd = args.remove(0);
    match cmd.as_str() {
        "share" => cmd_share(&client, &args).await,
        "list" => cmd_list(),
        "revoke" => cmd_revoke(&client, &args).await,
        "grantees" => cmd_grantees(&client, &args).await,
        other => Err(Error::argument(format!("unknown command: {other}"))),
    }
}

async fn cmd_share(client: &Client, args: &[String]) -> Result<(), Error> {
    if args.is_empty() {
        return Err(Error::argument(
            "usage: swarm-share share <file> --to <pubkey>...",
        ));
    }
    let file = &args[0];
    let mut grantees: Vec<String> = vec![];
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--to" => {
                i += 1;
                let pk = args
                    .get(i)
                    .ok_or_else(|| Error::argument("--to needs a value"))?;
                grantees.push(pk.clone());
                i += 1;
            }
            other => return Err(Error::argument(format!("unknown flag: {other}"))),
        }
    }
    if grantees.is_empty() {
        return Err(Error::argument("--to <pubkey> required at least once"));
    }
    let batch_id = env_batch()?;

    let body = fs::read(file).map_err(|e| Error::argument(format!("read {file}: {e}")))?;
    let name = PathBuf::from(file)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("share")
        .to_string();

    println!("Uploading {file} under ACT...");
    let opts = FileUploadOptions {
        base: UploadOptions {
            act: Some(true),
            ..Default::default()
        },
        content_type: Some("application/octet-stream".into()),
        ..Default::default()
    };
    let upload = client
        .file()
        .upload_file(
            &batch_id,
            Bytes::from(body),
            &name,
            "application/octet-stream",
            Some(&opts),
        )
        .await?;
    let history = upload
        .history_address
        .clone()
        .ok_or_else(|| Error::argument("upload did not return ACT history address"))?;
    println!("  file ref:        {}", upload.reference.to_hex());
    println!("  history_address: {}", history.to_hex());

    println!("Creating grantee list ({} keys)...", grantees.len());
    let created = client.api().create_grantees(&batch_id, &grantees).await?;
    println!("  grantee ref:     {}", created.reference);
    println!("  grantee history: {}", created.history_reference);

    let id = format!("{:08x}", now_secs() as u32);
    let mut shares = load();
    shares.shares.push(Share {
        id: id.clone(),
        file: name,
        file_ref: upload.reference.to_hex(),
        history_address: history.to_hex(),
        grantee_ref: created.reference.clone(),
        grantee_history: created.history_reference.clone(),
        grantees: grantees.clone(),
        ts: now_secs(),
    });
    save(&shares)?;

    println!("\nRecipient instructions for share {id}:");
    println!("  set BEE_URL to a node where the recipient is the publisher,");
    println!("  then download with these headers:");
    println!("    Swarm-Act:                 true");
    println!("    Swarm-Act-Publisher:       <publisher's compressed pubkey>");
    println!("    Swarm-Act-History-Address: {}", history.to_hex());
    println!("    Swarm-Act-Timestamp:       <current unix time>");
    println!("  on /bzz/{}/", upload.reference.to_hex());
    Ok(())
}

fn cmd_list() -> Result<(), Error> {
    let shares = load();
    if shares.shares.is_empty() {
        println!("(no shares yet)");
        return Ok(());
    }
    println!("{:<10}  {:<20}  {:<10}  file_ref", "id", "file", "grantees");
    for s in &shares.shares {
        println!(
            "{:<10}  {:<20}  {:<10}  {}",
            s.id,
            truncate(&s.file, 20),
            s.grantees.len(),
            s.file_ref
        );
    }
    Ok(())
}

async fn cmd_revoke(client: &Client, args: &[String]) -> Result<(), Error> {
    if args.is_empty() {
        return Err(Error::argument(
            "usage: swarm-share revoke <id> --grantee <pubkey>",
        ));
    }
    let id = &args[0];
    let mut to_revoke: Vec<String> = vec![];
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--grantee" => {
                i += 1;
                to_revoke.push(
                    args.get(i)
                        .ok_or_else(|| Error::argument("--grantee needs a value"))?
                        .clone(),
                );
                i += 1;
            }
            other => return Err(Error::argument(format!("unknown flag: {other}"))),
        }
    }
    if to_revoke.is_empty() {
        return Err(Error::argument("--grantee <pubkey> required"));
    }

    let mut shares = load();
    let pos = shares
        .shares
        .iter()
        .position(|s| s.id == *id)
        .ok_or_else(|| Error::argument(format!("no share with id {id}")))?;
    let batch_id = env_batch()?;
    let s = &mut shares.shares[pos];

    let grantee_ref = Reference::from_hex(&s.grantee_ref)?;
    let history = Reference::from_hex(&s.history_address)?;

    let patched = client
        .api()
        .patch_grantees(&batch_id, &grantee_ref, &history, &[], &to_revoke)
        .await?;
    s.grantee_ref = patched.reference.clone();
    s.grantee_history = patched.history_reference.clone();
    s.grantees.retain(|g| !to_revoke.contains(g));
    save(&shares)?;

    println!("Revoked {} grantee(s) from share {id}", to_revoke.len());
    println!("  new grantee ref: {}", patched.reference);
    Ok(())
}

async fn cmd_grantees(client: &Client, args: &[String]) -> Result<(), Error> {
    let id = args
        .first()
        .ok_or_else(|| Error::argument("usage: swarm-share grantees <id>"))?;
    let shares = load();
    let s = shares
        .shares
        .iter()
        .find(|s| s.id == *id)
        .ok_or_else(|| Error::argument(format!("no share with id {id}")))?;
    let r = Reference::from_hex(&s.grantee_ref)?;
    let live = client.api().get_grantees(&r).await?;
    println!("share {id}: {}", s.file);
    println!("  cached: {} grantees", s.grantees.len());
    for g in &s.grantees {
        println!("    {g}");
    }
    println!("  live:   {} grantees", live.len());
    for g in &live {
        println!("    {g}");
    }
    Ok(())
}

fn env_batch() -> Result<BatchId, Error> {
    let h = env::var("BEE_BATCH_ID").map_err(|_| Error::argument("BEE_BATCH_ID is required"))?;
    BatchId::from_hex(&h)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.into()
    } else {
        format!("{}…", &s[..n - 1])
    }
}

fn load() -> Shares {
    fs::read(STATE_FILE)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save(s: &Shares) -> Result<(), Error> {
    let bytes = serde_json::to_vec_pretty(s)?;
    fs::write(STATE_FILE, bytes).map_err(|e| Error::argument(format!("write: {e}")))
}
