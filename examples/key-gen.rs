//! key-gen — generate a fresh secp256k1 keypair and print all the
//! useful forms (private hex, compressed public hex, EIP-55 address).
//! Pure offline.
//!
//! ```text
//! cargo run --example key-gen
//! ```
//!
//! Save the printed private key with the same care you'd give an
//! Ethereum private key — it controls every feed, SOC, GSOC and ACT
//! identity built on top of it.

use std::process::ExitCode;

use bee::swarm::{Error, PrivateKey};
use rand::RngCore;

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
    // 32 bytes from the OS CSPRNG.
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);

    let signer = PrivateKey::new(&bytes)?;
    let public = signer.public_key()?;
    let address = public.address();

    println!("Generated secp256k1 keypair");
    println!("===========================");
    println!("Private key:           0x{}", signer.to_hex());
    println!("Public key (uncomp):   0x{}", public.to_hex());
    println!("Public key (comp):     0x{}", public.compressed_hex()?);
    println!("Ethereum address:      {}", address.to_checksum());
    println!();
    println!("Usage hints:");
    println!(
        "- Set BEE_SIGNER_HEX={} to drive feed-update / pss / soc examples",
        signer.to_hex()
    );
    println!("- The address is what readers need to follow your feeds");
    Ok(())
}
