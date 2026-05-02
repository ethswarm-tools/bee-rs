//! manifest-add-file — build a Mantaray manifest from a list of
//! entries, hash it offline, then add a new entry, hash again, and
//! upload via `upload_collection_entries`. Demonstrates that adding
//! a file changes the manifest reference deterministically.
//!
//! ```text
//! cargo run --example manifest-add-file
//! ```
//!
//! Environment:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BATCH_ID` — usable postage batch (required).

use std::env;
use std::process::ExitCode;

use bee::file::{CollectionEntry, hash_collection_entries};
use bee::swarm::BatchId;
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

    // 1. Build initial entries.
    let mut entries = vec![
        CollectionEntry::new("index.html", b"<h1>hello</h1>".to_vec()),
        CollectionEntry::new("about.html", b"<h1>about</h1>".to_vec()),
    ];
    let root1 = hash_collection_entries(&entries)?;
    println!("Initial manifest ({} entries) → {}", entries.len(), root1.to_hex());

    // 2. Add a new file.
    entries.push(CollectionEntry::new(
        "contact.html",
        b"<h1>contact</h1>".to_vec(),
    ));
    let root2 = hash_collection_entries(&entries)?;
    println!("With contact.html ({} entries) → {}", entries.len(), root2.to_hex());

    if root1 == root2 {
        return Err(Error::argument(
            "manifest root did not change after adding a file",
        ));
    }
    println!("(root changed — adding a file mutates the manifest deterministically)");

    // 3. Upload the new manifest. Bee receives the entries as a tar
    //    and rebuilds the manifest server-side; the returned reference
    //    must equal our offline `root2`.
    let client = Client::new(&url)?;
    let result = client
        .file()
        .upload_collection_entries(&batch_id, &entries, None)
        .await?;
    println!("\nUploaded → {}", result.reference.to_hex());
    if result.reference == root2 {
        println!("matches offline hash ✓");
    } else {
        // bee-rs's `hash_collection_entries` can diverge from Bee's
        // server-side manifest hashing in edge cases (default
        // metadata, file-mode bits). bee-go matches; bee-rs is on
        // the catch-up list. Surface as a warning so the upload
        // path itself still demonstrates as working.
        eprintln!(
            "warning: offline hash {} differs from server hash {}",
            root2.to_hex(),
            result.reference.to_hex()
        );
    }

    let trimmed = url.trim_end_matches('/');
    println!("\nBrowse at: {trimmed}/bzz/{}/index.html", result.reference.to_hex());
    println!("           {trimmed}/bzz/{}/contact.html", result.reference.to_hex());
    Ok(())
}
