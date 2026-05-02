//! swarm-keyring — passphrase-encrypted secp256k1 key store.
//!
//! Generate, import, list, export, and "use" Swarm signing keys
//! without keeping them in plaintext on disk. Each key is encrypted
//! with AES-256-GCM using a key derived from the user's passphrase
//! via scrypt, and stored in `keyring.json`.
//!
//! ```text
//! swarm-keyring new      <name>          # generate + store
//! swarm-keyring import   <name> <hex>    # store an existing key
//! swarm-keyring list                     # name → eth address
//! swarm-keyring export   <name>          # decrypt + print hex
//! swarm-keyring address  <name>          # print eth address only
//! ```
//!
//! Passphrase is read from `BEE_KEYRING_PASS`; for production CLI
//! use, swap to a TTY prompt (rpassword).

use std::env;
use std::fs;
use std::process::ExitCode;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use bee::Error;
use bee::swarm::PrivateKey;
use rand::RngCore;
use scrypt::Params as ScryptParams;
use serde::{Deserialize, Serialize};

const KEYRING_FILE: &str = "keyring.json";

#[derive(Serialize, Deserialize, Debug, Default)]
struct Keyring {
    keys: Vec<Encrypted>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Encrypted {
    name: String,
    address: String,
    salt_hex: String,
    nonce_hex: String,
    cipher_hex: String,
    log_n: u8,
    r: u32,
    p: u32,
}

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
    let mut args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        return Err(Error::argument(
            "usage: swarm-keyring <new|import|list|export|address> ...",
        ));
    }
    let cmd = args.remove(0);
    match cmd.as_str() {
        "new" => cmd_new(&args),
        "import" => cmd_import(&args),
        "list" => cmd_list(),
        "export" => cmd_export(&args),
        "address" => cmd_address(&args),
        other => Err(Error::argument(format!("unknown command: {other}"))),
    }
}

fn cmd_new(args: &[String]) -> Result<(), Error> {
    let name = args
        .first()
        .ok_or_else(|| Error::argument("usage: swarm-keyring new <name>"))?;
    let pass = passphrase()?;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let pk = PrivateKey::new(&bytes)?;
    insert(name, &pk, &pass)?;
    println!("created {name}: {}", pk.public_key()?.address().to_hex());
    Ok(())
}

fn cmd_import(args: &[String]) -> Result<(), Error> {
    if args.len() < 2 {
        return Err(Error::argument("usage: swarm-keyring import <name> <hex>"));
    }
    let name = &args[0];
    let pk = PrivateKey::from_hex(&args[1])?;
    let pass = passphrase()?;
    insert(name, &pk, &pass)?;
    println!("imported {name}: {}", pk.public_key()?.address().to_hex());
    Ok(())
}

fn cmd_list() -> Result<(), Error> {
    let kr = load();
    if kr.keys.is_empty() {
        println!("(empty keyring)");
        return Ok(());
    }
    println!("{:<20}  address", "name");
    for k in &kr.keys {
        println!("{:<20}  {}", k.name, k.address);
    }
    Ok(())
}

fn cmd_export(args: &[String]) -> Result<(), Error> {
    let name = args
        .first()
        .ok_or_else(|| Error::argument("usage: swarm-keyring export <name>"))?;
    let pass = passphrase()?;
    let pk = decrypt(name, &pass)?;
    println!("{}", pk.to_hex());
    Ok(())
}

fn cmd_address(args: &[String]) -> Result<(), Error> {
    let name = args
        .first()
        .ok_or_else(|| Error::argument("usage: swarm-keyring address <name>"))?;
    let kr = load();
    let entry = kr
        .keys
        .iter()
        .find(|k| k.name == *name)
        .ok_or_else(|| Error::argument(format!("no key named {name}")))?;
    println!("{}", entry.address);
    Ok(())
}

fn insert(name: &str, pk: &PrivateKey, pass: &str) -> Result<(), Error> {
    let mut kr = load();
    if kr.keys.iter().any(|k| k.name == name) {
        return Err(Error::argument(format!("key {name} already exists")));
    }
    let mut salt = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);
    let mut nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);

    let log_n: u8 = 14;
    let r: u32 = 8;
    let p: u32 = 1;
    let key = derive_key(pass.as_bytes(), &salt, log_n, r, p)?;
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| Error::argument(format!("aead: {e}")))?;
    // `Nonce::from_slice` is flagged as deprecated by clippy via the
    // generic-array transition, but aes-gcm 0.10 still uses 0.14 so
    // we suppress locally rather than chase the upgrade.
    #[allow(deprecated)]
    let nonce_ref = Nonce::from_slice(&nonce);
    let ciphertext = cipher
        .encrypt(nonce_ref, pk.as_bytes())
        .map_err(|e| Error::argument(format!("encrypt: {e}")))?;

    kr.keys.push(Encrypted {
        name: name.into(),
        address: pk.public_key()?.address().to_hex(),
        salt_hex: hex::encode(salt),
        nonce_hex: hex::encode(nonce),
        cipher_hex: hex::encode(&ciphertext),
        log_n,
        r,
        p,
    });
    save(&kr)
}

fn decrypt(name: &str, pass: &str) -> Result<PrivateKey, Error> {
    let kr = load();
    let entry = kr
        .keys
        .iter()
        .find(|k| k.name == name)
        .ok_or_else(|| Error::argument(format!("no key named {name}")))?;
    let salt = hex::decode(&entry.salt_hex).map_err(|e| Error::argument(format!("salt: {e}")))?;
    let nonce =
        hex::decode(&entry.nonce_hex).map_err(|e| Error::argument(format!("nonce: {e}")))?;
    let ct = hex::decode(&entry.cipher_hex).map_err(|e| Error::argument(format!("cipher: {e}")))?;
    let key = derive_key(pass.as_bytes(), &salt, entry.log_n, entry.r, entry.p)?;
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| Error::argument(format!("aead: {e}")))?;
    #[allow(deprecated)]
    let nonce_ref = Nonce::from_slice(&nonce);
    let pt = cipher
        .decrypt(nonce_ref, ct.as_ref())
        .map_err(|_| Error::argument("decryption failed (wrong passphrase?)"))?;
    PrivateKey::new(&pt)
}

fn derive_key(pass: &[u8], salt: &[u8], log_n: u8, r: u32, p: u32) -> Result<[u8; 32], Error> {
    let params = ScryptParams::new(log_n, r, p, 32)
        .map_err(|e| Error::argument(format!("scrypt params: {e}")))?;
    let mut out = [0u8; 32];
    scrypt::scrypt(pass, salt, &params, &mut out)
        .map_err(|e| Error::argument(format!("scrypt: {e}")))?;
    Ok(out)
}

fn passphrase() -> Result<String, Error> {
    env::var("BEE_KEYRING_PASS")
        .map_err(|_| Error::argument("BEE_KEYRING_PASS not set (32-char passphrase recommended)"))
}

fn load() -> Keyring {
    fs::read(KEYRING_FILE)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save(kr: &Keyring) -> Result<(), Error> {
    let bytes = serde_json::to_vec_pretty(kr)?;
    fs::write(KEYRING_FILE, bytes).map_err(|e| Error::argument(format!("write: {e}")))
}
