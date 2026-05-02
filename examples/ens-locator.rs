//! ens-locator — `ResourceLocator` demo. Pure offline. Shows how to
//! treat hex references and ENS names uniformly when assembling Bee
//! URLs, plus offline path resolution within a manifest.
//!
//! ```text
//! cargo run --example ens-locator
//! ```
//!
//! No node required. ENS resolution itself happens server-side when the
//! locator is used as the `{ref}` segment of `/bzz/{ref}` or
//! `/bytes/{ref}`.

use std::process::ExitCode;

use bee::manifest::{MantarayNode, ResourceLocator, resolve_path};
use bee::swarm::{Error, Reference};

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
    let base_url = "http://localhost:1633";

    println!("ResourceLocator constructions");
    println!("-----------------------------");

    let hex_ref = Reference::from_hex(&"ab".repeat(32))?;
    let from_ref = ResourceLocator::from_reference(hex_ref.clone());
    println!("from_reference:   {from_ref}");
    println!("  url:            {base_url}/bzz/{from_ref}/");

    let from_ens = ResourceLocator::from_ens("hello.eth")?;
    println!("from_ens:         {from_ens}");
    println!("  url:            {base_url}/bzz/{from_ens}/");

    let parsed_ref = ResourceLocator::parse(&"cd".repeat(32))?;
    let parsed_ens = ResourceLocator::parse("docs.eth")?;
    println!("parse(hex):       {parsed_ref}");
    println!("parse(ens):       {parsed_ens}");

    println!("\nOffline manifest path resolution");
    println!("--------------------------------");
    let mut manifest = MantarayNode::new();
    let index_html = Reference::from_hex(&"11".repeat(32))?;
    let logo_png = Reference::from_hex(&"22".repeat(32))?;
    manifest.add_fork(b"index.html", Some(&index_html), None);
    manifest.add_fork(b"assets/logo.png", Some(&logo_png), None);

    println!("manifest entries:");
    for (path, _) in manifest.collect() {
        println!("  - {}", String::from_utf8_lossy(&path));
    }

    let resolved_index = resolve_path(&manifest, "/index.html")?;
    let resolved_logo = resolve_path(&manifest, "assets/logo.png")?;
    println!("resolve(/index.html)        → {}", resolved_index.to_hex());
    println!("resolve(assets/logo.png)    → {}", resolved_logo.to_hex());

    Ok(())
}
