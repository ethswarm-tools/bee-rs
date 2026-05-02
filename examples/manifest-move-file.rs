//! manifest-move-file — move a file's path within a manifest by
//! removing the fork at the old path and adding it at the new path,
//! preserving the file's content reference. Demonstrates Mantaray's
//! offline `add_fork` / `remove_fork` primitives.
//!
//! ```text
//! cargo run --example manifest-move-file
//! ```
//!
//! Environment:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BATCH_ID` — usable postage batch (required).

use std::env;
use std::process::ExitCode;

use bee::file::{CollectionEntry, hash_collection_entries};
use bee::manifest::{MantarayNode, populate_self_addresses};
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

    // 1. Pretend each file is already content-addressed somewhere on
    //    Swarm — for the demo we just use deterministic addresses.
    let logo = Reference::from_hex(&"aa".repeat(32))?;
    let about = Reference::from_hex(&"bb".repeat(32))?;
    let style = Reference::from_hex(&"cc".repeat(32))?;

    // 2. Build manifest with a file at the old path.
    let mut node_v1 = MantarayNode::new();
    node_v1.add_fork(b"images/logo.png", Some(&logo), None);
    node_v1.add_fork(b"about.html", Some(&about), None);
    node_v1.add_fork(b"style.css", Some(&style), None);
    let root_v1 = populate_self_addresses(&mut node_v1)?;
    println!("v1 manifest:");
    for (path, _) in node_v1.collect() {
        println!("  - {}", String::from_utf8_lossy(&path));
    }
    println!("  root: {}", hex(&root_v1));

    // 3. Move logo.png from images/ to assets/.
    let mut node_v2 = MantarayNode::new();
    // Easier than mutating in-place: rebuild with the new layout.
    node_v2.add_fork(b"assets/logo.png", Some(&logo), None);
    node_v2.add_fork(b"about.html", Some(&about), None);
    node_v2.add_fork(b"style.css", Some(&style), None);
    let root_v2 = populate_self_addresses(&mut node_v2)?;
    println!("\nv2 manifest (logo.png moved):");
    for (path, _) in node_v2.collect() {
        println!("  - {}", String::from_utf8_lossy(&path));
    }
    println!("  root: {}", hex(&root_v2));

    // Demonstrate remove_fork on a clone: surgical mutation.
    let mut surgical = node_v1.clone();
    surgical.remove_fork(b"images/logo.png")?;
    surgical.add_fork(b"assets/logo.png", Some(&logo), None);
    let surgical_root = populate_self_addresses(&mut surgical)?;
    println!(
        "\nsurgical (remove_fork + add_fork) → same root as rebuild? {}",
        surgical_root == root_v2
    );

    // 4. To make the moved manifest live on Bee, upload via
    //    upload_collection_entries with the new layout. Note: this
    //    re-uploads the underlying file bytes too — bee-rs and bee-go
    //    don't yet expose `save_recursively` for in-place mutation
    //    without re-uploading leaves.
    let entries = vec![
        CollectionEntry::new("assets/logo.png", b"<png bytes>".to_vec()),
        CollectionEntry::new("about.html", b"<h1>about</h1>".to_vec()),
        CollectionEntry::new("style.css", b"body { color: red }".to_vec()),
    ];
    let offline_root = hash_collection_entries(&entries)?;
    println!("\nOffline hash for upload entries: {}", offline_root.to_hex());

    let client = Client::new(&url)?;
    let result = client
        .file()
        .upload_collection_entries(&batch_id, &entries, None)
        .await?;
    let trimmed = url.trim_end_matches('/');
    println!("Uploaded   → {}", result.reference.to_hex());
    println!("Browse at: {trimmed}/bzz/{}/assets/logo.png", result.reference.to_hex());
    Ok(())
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
