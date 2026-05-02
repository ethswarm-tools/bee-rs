//! soc-write-read — sign and upload three Single Owner Chunks at
//! distinct identifiers, then read them back via the SOC reader.
//!
//! A SOC's address is `keccak256(identifier || owner)`. Anyone who
//! knows (owner, identifier) can fetch and verify a chunk: the
//! signature is recovered server-side and matched against the
//! expected owner.
//!
//! ```text
//! cargo run --example soc-write-read
//! ```
//!
//! Environment:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BATCH_ID` — usable postage batch (required).
//! - `BEE_SIGNER_HEX` — 32-byte hex private key (required).

use std::env;
use std::process::ExitCode;

use bee::file::soc_address;
use bee::swarm::{BatchId, Identifier, PrivateKey};
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
    let signer_hex =
        env::var("BEE_SIGNER_HEX").map_err(|_| Error::argument("BEE_SIGNER_HEX is required"))?;
    let signer = PrivateKey::from_hex(&signer_hex)?;
    let owner = signer.public_key()?.address();

    let client = Client::new(&url)?;
    let writer = client.file().make_soc_writer(signer)?;
    println!("SOC owner: {}", owner.to_hex());

    let entries: [(&str, &[u8]); 3] = [
        ("greeting", b"hello, single-owner-chunk"),
        ("number", b"42"),
        ("payload", b"\x01\x02\x03 binary bytes"),
    ];

    println!("\nWriting {} SOCs...", entries.len());
    for (label, body) in &entries {
        let id = Identifier::from_string(label);
        let result = writer.upload(&batch_id, &id, body, None).await?;
        let addr = soc_address(&id, &owner)?;
        println!(
            "  {:<8} id={}  ref={}  uploaded={}",
            label,
            id.to_hex(),
            addr.to_hex(),
            result.reference.to_hex()
        );
    }

    println!("\nReading back via SOC reader...");
    let reader = client.file().make_soc_reader(owner);
    for (label, body) in &entries {
        let id = Identifier::from_string(label);
        let soc = reader.download(&id).await?;
        // unmarshal_single_owner_chunk (called inside reader.download)
        // already verified the signature and recovered the owner; we
        // just check it matches what we expect.
        let owner_ok = soc.owner == owner;
        let payload_ok = soc.payload.as_slice() == *body;
        match std::str::from_utf8(&soc.payload) {
            Ok(s) => println!(
                "  {:<8} payload={s:?}  owner_ok={owner_ok}  payload_ok={payload_ok}",
                label
            ),
            Err(_) => println!(
                "  {:<8} payload=({} bytes binary)  owner_ok={owner_ok}  payload_ok={payload_ok}",
                label,
                soc.payload.len()
            ),
        }
        if !owner_ok || !payload_ok {
            return Err(Error::argument("SOC verification failed"));
        }
    }

    println!("\nAll {} SOCs verified.", entries.len());
    Ok(())
}
