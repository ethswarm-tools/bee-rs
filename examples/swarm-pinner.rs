//! swarm-pinner — watch a directory, upload+pin new files, and
//! periodically check that pinned content is still retrievable.
//!
//! Polls every `--interval` seconds (default 5). Each new regular
//! file under `<watch-dir>` is uploaded with `pin: true`; existing
//! pinned items are re-checked with `is_retrievable`. State persists
//! in `.swarm-pinner.json` so restarts don't re-upload.
//!
//! ```text
//! swarm-pinner <watch-dir>          # run forever
//! swarm-pinner <watch-dir> --once   # one pass + exit
//! ```
//!
//! Environment:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BATCH_ID` — usable postage batch (required).

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bee::api::{FileUploadOptions, UploadOptions};
use bee::swarm::{BatchId, Reference};
use bee::{Client, Error};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use tokio::time::sleep;

const STATE_FILE: &str = ".swarm-pinner.json";

#[derive(Serialize, Deserialize, Debug, Default)]
struct State {
    pinned: BTreeMap<String, PinnedEntry>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct PinnedEntry {
    reference: String,
    size: u64,
    pinned_at: u64,
    last_check_ok: bool,
    last_check_at: u64,
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
    let batch_id = env_batch()?;

    let mut args = env::args().skip(1);
    let dir = args
        .next()
        .ok_or_else(|| Error::argument("usage: swarm-pinner <watch-dir> [--once] [--interval N]"))?;
    let mut once = false;
    let mut interval_secs: u64 = 5;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--once" => once = true,
            "--interval" => {
                interval_secs = args
                    .next()
                    .ok_or_else(|| Error::argument("--interval needs N"))?
                    .parse()
                    .map_err(|e| Error::argument(format!("invalid interval: {e}")))?;
            }
            other => return Err(Error::argument(format!("unknown flag: {other}"))),
        }
    }

    let dir_path = PathBuf::from(&dir);
    if !dir_path.is_dir() {
        return Err(Error::argument(format!("{dir} is not a directory")));
    }

    let client = Client::new(&url)?;
    println!("Watching {dir} (every {interval_secs}s)");

    loop {
        if let Err(e) = pass(&client, &batch_id, &dir_path).await {
            eprintln!("pass error: {e}");
        }
        if once {
            break;
        }
        sleep(Duration::from_secs(interval_secs)).await;
    }
    Ok(())
}

async fn pass(client: &Client, batch_id: &BatchId, dir: &Path) -> Result<(), Error> {
    let mut state = load();
    let now = now_secs();

    // Phase 1: upload + pin anything new.
    let entries = list_files(dir)?;
    for path in &entries {
        let key = path.display().to_string();
        if state.pinned.contains_key(&key) {
            continue;
        }
        let body = fs::read(path).map_err(|e| Error::argument(format!("read {key}: {e}")))?;
        let size = body.len() as u64;
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("file")
            .to_string();

        let opts = FileUploadOptions {
            base: UploadOptions {
                pin: Some(true),
                ..Default::default()
            },
            content_type: Some("application/octet-stream".into()),
            ..Default::default()
        };
        let result = client
            .file()
            .upload_file(
                batch_id,
                Bytes::from(body),
                &name,
                "application/octet-stream",
                Some(&opts),
            )
            .await?;
        println!(
            "[{now}] uploaded+pinned {key} ({size} bytes) → {}",
            result.reference.to_hex()
        );
        state.pinned.insert(
            key,
            PinnedEntry {
                reference: result.reference.to_hex(),
                size,
                pinned_at: now,
                last_check_ok: true,
                last_check_at: now,
            },
        );
    }

    // Phase 2: re-check retrievability.
    for (path, e) in state.pinned.iter_mut() {
        let r = match Reference::from_hex(&e.reference) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let ok = client.api().is_retrievable(&r).await.unwrap_or(false);
        e.last_check_ok = ok;
        e.last_check_at = now;
        if !ok {
            eprintln!("[{now}] WARN {path} → {} not retrievable", e.reference);
        }
    }

    save(&state)?;
    print_status(&state);
    Ok(())
}

fn list_files(dir: &Path) -> Result<Vec<PathBuf>, Error> {
    let mut out = vec![];
    walk(dir, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk(here: &Path, out: &mut Vec<PathBuf>) -> Result<(), Error> {
    for entry in fs::read_dir(here).map_err(|e| Error::argument(format!("read_dir: {e}")))? {
        let entry = entry.map_err(|e| Error::argument(format!("dir entry: {e}")))?;
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out)?;
        } else if path.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

fn print_status(state: &State) {
    let total = state.pinned.len();
    let ok = state.pinned.values().filter(|e| e.last_check_ok).count();
    println!("status: {ok}/{total} retrievable");
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

fn load() -> State {
    fs::read(STATE_FILE)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save(s: &State) -> Result<(), Error> {
    let bytes = serde_json::to_vec_pretty(s)?;
    fs::write(STATE_FILE, bytes).map_err(|e| Error::argument(format!("write: {e}")))
}
