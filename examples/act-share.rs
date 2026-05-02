//! act-share — upload a file under an Access Control Trie (ACT),
//! manage the grantee list (create / get / patch), and download as
//! the publisher using the resulting history root. ACT lets the owner
//! of an upload share encrypted content with a set of public keys
//! without re-uploading.
//!
//! ```text
//! cargo run --example act-share
//! ```
//!
//! Environment:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BATCH_ID` — usable postage batch (required).
//!
//! The publisher is the local Bee node — Bee signs ACT operations
//! with the node's identity and uses its `publicKey` from
//! `GET /addresses` for download authorisation.

use std::env;
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bee::api::{DownloadOptions, FileUploadOptions, UploadOptions};
use bee::swarm::{BatchId, PrivateKey, PublicKey, Reference};
use bee::{Client, Error};
use rand::RngCore;

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

    // 1. Read the local node's compressed pubkey — used as the ACT
    //    publisher when downloading.
    let addresses = client.debug().addresses().await?;
    let publisher_pk = PublicKey::from_hex(&addresses.public_key)?;
    println!("Publisher (node) pubkey: {}", addresses.public_key);

    // 2. Generate three random compressed grantee pubkeys.
    let g1 = random_compressed_pubkey()?;
    let g2 = random_compressed_pubkey()?;
    let g3 = random_compressed_pubkey()?;
    println!("Generated grantees:\n  {g1}\n  {g2}\n  {g3}\n");

    // 3. Upload a file with `act: true`. Bee returns an encrypted
    //    reference plus a `Swarm-Act-History-Address` header.
    let payload = bytes::Bytes::from_static(b"hello act grantees!");
    let upload_opts = FileUploadOptions {
        base: UploadOptions {
            act: Some(true),
            ..Default::default()
        },
        content_type: Some("text/plain".into()),
        ..Default::default()
    };
    let upload = client
        .file()
        .upload_file(
            &batch_id,
            payload.clone(),
            "act-secret.txt",
            "text/plain",
            Some(&upload_opts),
        )
        .await?;
    let history = upload
        .history_address
        .clone()
        .ok_or_else(|| Error::argument("upload did not return ACT history address"))?;
    println!("Uploaded:");
    println!("  reference:       {}", upload.reference.to_hex());
    println!("  history_address: {}\n", history.to_hex());

    // 4. Create an initial grantee list with three keys.
    let grantees = vec![g1.clone(), g2.clone(), g3.clone()];
    let created = client
        .api()
        .create_grantees(&batch_id, &grantees)
        .await?;
    let created_ref = Reference::from_hex(&created.reference)?;
    println!("Created grantee list:");
    println!("  ref:       {}", created.reference);
    println!("  historyref:{}", created.history_reference);

    let listed = client.api().get_grantees(&created_ref).await?;
    println!("  members ({}): {listed:?}\n", listed.len());

    // 5. Patch — keep only g1, revoke g2 and g3. Patch is anchored at
    //    the *upload's* history address (that's the access policy
    //    being mutated). Bee needs a moment to settle the grantee
    //    chunk after `create_grantees`; bee-js's integration test
    //    waits 5s, 2s suffices for a local node.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let patched = client
        .api()
        .patch_grantees(
            &batch_id,
            &created_ref,
            &history,
            std::slice::from_ref(&g1),
            &[g2.clone(), g3.clone()],
        )
        .await?;
    let patched_ref = Reference::from_hex(&patched.reference)?;
    let patched_after = client.api().get_grantees(&patched_ref).await?;
    println!("After patch (add 1, revoke 2):");
    println!("  ref:       {}", patched.reference);
    println!("  historyref:{}", patched.history_reference);
    println!("  members ({}): {patched_after:?}\n", patched_after.len());

    // 6. Download the original upload as the publisher. We pass the
    //    publisher's pubkey + history address + a current timestamp;
    //    Bee resolves "is the caller's identity authorised at ts?"
    //    Since the local node is the publisher it always passes.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let download_opts = DownloadOptions {
        act_publisher: Some(publisher_pk),
        act_history_address: Some(history.clone()),
        act_timestamp: Some(now),
        ..Default::default()
    };
    let (body, headers) = client
        .file()
        .download_file(&upload.reference, Some(&download_opts))
        .await?;
    println!(
        "Downloaded {} bytes (filename={:?}, ct={:?})",
        body.len(),
        headers.name,
        headers.content_type
    );
    match std::str::from_utf8(&body) {
        Ok(s) => println!("  payload: {s:?}"),
        Err(_) => println!("  payload: ({} bytes binary)", body.len()),
    }
    if body != payload {
        return Err(Error::argument("ACT round-trip payload mismatch"));
    }
    println!("\nRound-trip OK: ACT-protected upload decrypted via publisher identity.");
    Ok(())
}

fn random_compressed_pubkey() -> Result<String, Error> {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let pk = PrivateKey::new(&bytes)?;
    pk.public_key()?.compressed_hex()
}
