//! swarm-fs — filesystem-style staging for a Mantaray collection.
//!
//! Maintain a local "staging tree" in `.swarm-fs.json` mapping
//! `<path-in-manifest> → <local-file-on-disk>`. Mutate it like a
//! filesystem (`add`, `mv`, `rm`, `ls`); when you're happy, `publish`
//! reads each local file, packages them as a Mantaray collection, and
//! prints the resulting Swarm reference. The staging file is the
//! single source of truth — bee-rs/bee-go don't yet expose HTTP-aware
//! `load_recursively` for in-place manifest mutation, so we round-trip
//! through local state instead.
//!
//! ```text
//! swarm-fs init
//! swarm-fs add  <path-in-manifest>  <local-file>
//! swarm-fs mv   <old-path>          <new-path>
//! swarm-fs rm   <path>
//! swarm-fs ls
//! swarm-fs publish [--index <name>]
//! ```
//!
//! Environment:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BATCH_ID` — usable postage batch (required for `publish`).

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use bee::api::CollectionUploadOptions;
use bee::file::CollectionEntry;
use bee::swarm::BatchId;
use bee::{Client, Error};
use serde::{Deserialize, Serialize};

const STATE_PATH: &str = ".swarm-fs.json";

#[derive(Serialize, Deserialize, Debug, Default)]
struct Tree {
    entries: BTreeMap<String, String>,
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
    let mut args = env::args().skip(1);
    let cmd = args.next().ok_or_else(|| {
        Error::argument("usage: swarm-fs <init|add|mv|rm|ls|publish> ...")
    })?;
    match cmd.as_str() {
        "init" => cmd_init(),
        "add" => {
            let path = args
                .next()
                .ok_or_else(|| Error::argument("usage: swarm-fs add <path> <local-file>"))?;
            let local = args
                .next()
                .ok_or_else(|| Error::argument("usage: swarm-fs add <path> <local-file>"))?;
            cmd_add(&path, &local)
        }
        "mv" => {
            let old = args
                .next()
                .ok_or_else(|| Error::argument("usage: swarm-fs mv <old> <new>"))?;
            let new = args
                .next()
                .ok_or_else(|| Error::argument("usage: swarm-fs mv <old> <new>"))?;
            cmd_mv(&old, &new)
        }
        "rm" => {
            let path = args
                .next()
                .ok_or_else(|| Error::argument("usage: swarm-fs rm <path>"))?;
            cmd_rm(&path)
        }
        "ls" => cmd_ls(),
        "publish" => cmd_publish(args.collect()).await,
        other => Err(Error::argument(format!("unknown command: {other}"))),
    }
}

fn cmd_init() -> Result<(), Error> {
    if PathBuf::from(STATE_PATH).exists() {
        return Err(Error::argument(format!("{STATE_PATH} already exists")));
    }
    save(&Tree::default())?;
    println!("Initialised empty staging tree at {STATE_PATH}.");
    Ok(())
}

fn cmd_add(path: &str, local: &str) -> Result<(), Error> {
    let path = normalise(path)?;
    let local_path = PathBuf::from(local);
    if !local_path.is_file() {
        return Err(Error::argument(format!("{local} is not a regular file")));
    }
    let mut tree = load()?;
    tree.entries.insert(path.clone(), local.to_string());
    save(&tree)?;
    println!("staged {path} → {local}");
    Ok(())
}

fn cmd_mv(old: &str, new: &str) -> Result<(), Error> {
    let old = normalise(old)?;
    let new = normalise(new)?;
    let mut tree = load()?;
    let v = tree
        .entries
        .remove(&old)
        .ok_or_else(|| Error::argument(format!("no such path: {old}")))?;
    tree.entries.insert(new.clone(), v);
    save(&tree)?;
    println!("renamed {old} → {new}");
    Ok(())
}

fn cmd_rm(path: &str) -> Result<(), Error> {
    let path = normalise(path)?;
    let mut tree = load()?;
    tree.entries
        .remove(&path)
        .ok_or_else(|| Error::argument(format!("no such path: {path}")))?;
    save(&tree)?;
    println!("removed {path}");
    Ok(())
}

fn cmd_ls() -> Result<(), Error> {
    let tree = load()?;
    if tree.entries.is_empty() {
        println!("(empty)");
        return Ok(());
    }
    println!("{:<40}  {}", "path", "local-file");
    for (p, l) in &tree.entries {
        println!("{p:<40}  {l}");
    }
    Ok(())
}

async fn cmd_publish(extra: Vec<String>) -> Result<(), Error> {
    let url = env::var("BEE_URL").unwrap_or_else(|_| "http://localhost:1633".into());
    let batch_id = env_batch()?;
    let tree = load()?;
    if tree.entries.is_empty() {
        return Err(Error::argument("nothing to publish"));
    }

    let mut index_doc: Option<String> = None;
    let mut iter = extra.into_iter();
    while let Some(a) = iter.next() {
        match a.as_str() {
            "--index" => {
                index_doc = Some(
                    iter.next()
                        .ok_or_else(|| Error::argument("--index needs a value"))?,
                );
            }
            other => return Err(Error::argument(format!("unknown publish flag: {other}"))),
        }
    }

    let mut entries: Vec<CollectionEntry> = vec![];
    for (path, local) in &tree.entries {
        let data = fs::read(local).map_err(|e| Error::argument(format!("read {local}: {e}")))?;
        entries.push(CollectionEntry { path: path.clone(), data });
    }

    let opts = CollectionUploadOptions {
        index_document: index_doc.clone(),
        ..Default::default()
    };
    let client = Client::new(&url)?;
    let result = client
        .file()
        .upload_collection_entries(&batch_id, &entries, Some(&opts))
        .await?;

    let trimmed = url.trim_end_matches('/');
    println!("Uploaded {} entries.", entries.len());
    println!("  reference: {}", result.reference.to_hex());
    println!("  url:       {trimmed}/bzz/{}/", result.reference.to_hex());
    if let Some(idx) = index_doc {
        println!("  index_document: {idx}");
    }
    Ok(())
}

fn normalise(p: &str) -> Result<String, Error> {
    let p = p.trim_start_matches('/').trim().to_string();
    if p.is_empty() {
        return Err(Error::argument("empty path"));
    }
    Ok(p)
}

fn env_batch() -> Result<BatchId, Error> {
    let h = env::var("BEE_BATCH_ID").map_err(|_| Error::argument("BEE_BATCH_ID is required"))?;
    BatchId::from_hex(&h)
}

fn save(t: &Tree) -> Result<(), Error> {
    let bytes = serde_json::to_vec_pretty(t)?;
    fs::write(STATE_PATH, bytes).map_err(|e| Error::argument(format!("write: {e}")))
}

fn load() -> Result<Tree, Error> {
    let bytes = fs::read(STATE_PATH)
        .map_err(|_| Error::argument(format!("{STATE_PATH} not found — run `init` first")))?;
    Ok(serde_json::from_slice(&bytes)?)
}
