//! ref-to-cid — convert a Swarm reference to a CID and back. Pure
//! offline. Useful for IPFS-style addressing or interop with tooling
//! that consumes CIDs.
//!
//! ```text
//! cargo run --example ref-to-cid -- <reference> [manifest|feed]
//! ```
//!
//! Defaults: kind = "manifest". The same hex reference yields different
//! CIDs for `manifest` (codec 0xfa) and `feed` (codec 0xfb).

use std::process::ExitCode;

use bee::swarm::{CidType, Error, Reference, convert_cid_to_reference, convert_reference_to_cid};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Error> {
    let mut args = std::env::args().skip(1);
    let ref_hex = args
        .next()
        .ok_or_else(|| Error::argument("usage: ref-to-cid <reference> [manifest|feed]"))?;
    let kind_str = args.next().unwrap_or_else(|| "manifest".to_string());
    let kind = match kind_str.to_ascii_lowercase().as_str() {
        "manifest" => CidType::Manifest,
        "feed" => CidType::Feed,
        other => {
            return Err(Error::argument(format!(
                "unknown kind {other:?} — expected manifest or feed"
            )));
        }
    };

    let reference = Reference::from_hex(&ref_hex)?;
    let cid = convert_reference_to_cid(&reference, kind)?;
    println!("Reference → CID");
    println!("---------------");
    println!("Reference: {}", reference.to_hex());
    println!("Kind:      {kind_str}");
    println!("CID:       {cid}");

    println!("\nCID → Reference (round-trip)");
    println!("----------------------------");
    let decoded = convert_cid_to_reference(&cid)?;
    println!("Reference: {}", decoded.reference.to_hex());
    println!("Kind:      {:?}", decoded.kind);
    Ok(())
}
