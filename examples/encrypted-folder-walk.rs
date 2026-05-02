//! encrypted-folder-walk — upload a small set of files as a
//! collection, then download the manifest's root chunk and walk the
//! fork tree recursively, fetching each child chunk and stitching the
//! file paths back together.
//!
//! Bee builds Mantaray manifests as a tree of chunks: the root only
//! holds the first byte of each file path, and longer prefixes spill
//! into child chunks. To reach a leaf you have to follow each fork's
//! child address until you hit a node whose `target_address` is set.
//!
//! Note on the name: an earlier version used `encrypt: true`, but
//! Bee's `/chunks` endpoint only accepts 32-byte addresses, so there
//! is no clean way to download an *encrypted manifest chunk* without
//! client-side AES-CTR decryption. We keep the filename for
//! continuity but demonstrate the cleaner unencrypted variant; see
//! [`encrypted-upload.rs`](encrypted-upload.rs) for the encryption
//! round-trip.
//!
//! ```text
//! cargo run --example encrypted-folder-walk
//! ```
//!
//! Environment:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BATCH_ID` — usable postage batch (required).

use std::env;
use std::process::ExitCode;

use bee::file::CollectionEntry;
use bee::manifest::unmarshal;
use bee::swarm::{BatchId, Reference};
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

    let client = Client::new(&url)?;

    // 1. Upload a few short files as a collection. Bee builds the
    //    manifest as a chunk tree where each filename is split across
    //    one or more chunks.
    let entries = vec![
        CollectionEntry::new("readme.txt", b"hello from the manifest walker".to_vec()),
        CollectionEntry::new("notes.md", b"# notes\n- one\n- two\n".to_vec()),
    ];
    let result = client
        .file()
        .upload_collection_entries(&batch_id, &entries, None)
        .await?;
    let manifest_ref = result.reference.clone();
    println!(
        "Manifest reference: {} ({} bytes)",
        manifest_ref.to_hex(),
        manifest_ref.len()
    );

    // 2. Walk the manifest tree recursively. Each node we parse may
    //    expose a `target_address` (a leaf — done) and/or more forks
    //    pointing at child chunks we need to fetch.
    let mut leaves: Vec<(String, Reference)> = Vec::new();
    walk(&client, &manifest_ref, Vec::new(), &mut leaves).await?;

    if leaves.is_empty() {
        return Err(Error::argument("manifest had no leaves"));
    }
    println!("\nManifest entries:");
    for (path, leaf) in &leaves {
        println!("  - {path:<24} → {} ({} bytes)", leaf.to_hex(), leaf.len());
    }

    // 3. Download each leaf via /bytes (manifest-resolved by Bee).
    for (path, leaf) in &leaves {
        let body = client.file().download_data(leaf, None).await?;
        match std::str::from_utf8(&body) {
            Ok(s) => println!("  fetched {path}: {s:?}"),
            Err(_) => println!("  fetched {path}: {} bytes (binary)", body.len()),
        }
    }
    Ok(())
}

// Boxed future is needed because async fns cannot recurse directly.
fn walk<'a>(
    client: &'a Client,
    chunk_ref: &'a Reference,
    path_so_far: Vec<u8>,
    out: &'a mut Vec<(String, Reference)>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), Error>> + Send + 'a>> {
    Box::pin(async move {
        let raw = client.file().download_chunk(chunk_ref, None).await?;
        let body = if raw.len() >= 8 { &raw[8..] } else { &raw[..] };
        let node = unmarshal(body, chunk_ref.as_bytes())?;

        if !node.is_null_target() {
            let leaf = Reference::new(&node.target_address)?;
            out.push((String::from_utf8_lossy(&path_so_far).into_owned(), leaf));
        }
        for fork in node.forks.values() {
            let Some(child_addr) = fork.node.self_address else {
                continue;
            };
            let child_ref = Reference::new(&child_addr)?;
            let mut next_path = path_so_far.clone();
            next_path.extend_from_slice(&fork.prefix);
            walk(client, &child_ref, next_path, out).await?;
        }
        Ok(())
    })
}
