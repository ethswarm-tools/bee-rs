//! integration-check exercises bee-rs against a live Bee node.
//!
//! Run against a node that has BZZ + native funds; the program will
//! buy a small postage batch (or reuse one via `BEE_BATCH_ID`) and a
//! few stamps will be consumed. It is safe to re-run.
//!
//! ```text
//! cargo run --example integration-check
//! ```
//!
//! Environment overrides:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BATCH_ID` — hex-encoded batch ID. When set, the batch is
//!   reused; when unset, a fresh batch is bought (slow on Sepolia).
//! - `BEE_BATCH_AMOUNT` — per-chunk amount when buying a batch
//!   (default: `"100000000"`).
//! - `BEE_BATCH_DEPTH` — depth when buying a batch (default: `17`).

use std::env;
use std::process::ExitCode;
use std::time::Duration;

use bee::Client;
use bee::file::CollectionEntry;
use bee::swarm::{BatchId, Identifier, PrivateKey, Topic};
use num_bigint::BigInt;

#[derive(Default)]
struct Tally {
    pass: u32,
    fail: u32,
    skip: u32,
}

impl Tally {
    fn ok(&mut self) {
        self.pass += 1;
    }
    fn err(&mut self) {
        self.fail += 1;
    }
    fn skipped(&mut self) {
        self.skip += 1;
    }
}

macro_rules! check {
    ($tally:expr, $name:literal, $body:expr) => {{
        let result: Result<(), String> = $body.await;
        match result {
            Ok(()) => {
                println!("  ok    {}", $name);
                $tally.ok();
            }
            Err(e) => {
                println!("  FAIL  {} — {e}", $name);
                $tally.err();
            }
        }
    }};
}

fn section(title: &str) {
    println!("\n=== {title} ===");
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> ExitCode {
    let url = env::var("BEE_URL").unwrap_or_else(|_| "http://localhost:1633".to_string());
    println!("Bee URL: {url}\n");

    let client = match Client::new(&url) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Client::new failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut tally = Tally::default();

    section("Read-only — connectivity & node info");
    check!(tally, "is_connected", async {
        if client.debug().is_connected().await {
            Ok(())
        } else {
            Err("not connected".to_string())
        }
    });
    check!(tally, "check_connection", async {
        client
            .debug()
            .check_connection()
            .await
            .map_err(|e| e.to_string())
    });
    check!(tally, "health", async {
        let h = client.debug().health().await.map_err(|e| e.to_string())?;
        println!(
            "    status={} version={} apiVersion={}",
            h.status, h.version, h.api_version
        );
        Ok(())
    });
    check!(tally, "versions", async {
        let v = client.debug().versions().await.map_err(|e| e.to_string())?;
        println!(
            "    bee={} api={} | supports bee={} api={}",
            v.bee_version,
            v.bee_api_version,
            v.supported_bee_version_exact,
            v.supported_api_version
        );
        Ok(())
    });
    check!(tally, "is_supported_api_version", async {
        let ok = client
            .debug()
            .is_supported_api_version()
            .await
            .map_err(|e| e.to_string())?;
        if ok {
            Ok(())
        } else {
            Err("API version mismatch".to_string())
        }
    });
    check!(tally, "node_info", async {
        let n = client
            .debug()
            .node_info()
            .await
            .map_err(|e| e.to_string())?;
        println!(
            "    beeMode={} chequebookEnabled={} swapEnabled={}",
            n.bee_mode, n.chequebook_enabled, n.swap_enabled
        );
        Ok(())
    });
    check!(tally, "addresses", async {
        let a = client
            .debug()
            .addresses()
            .await
            .map_err(|e| e.to_string())?;
        println!("    overlay={} ethereum={}", a.overlay, a.ethereum);
        Ok(())
    });
    check!(tally, "topology", async {
        let t = client.debug().topology().await.map_err(|e| e.to_string())?;
        println!(
            "    population={} connected={} depth={}",
            t.population, t.connected, t.depth
        );
        Ok(())
    });
    check!(tally, "chain_state", async {
        let c = client
            .debug()
            .chain_state()
            .await
            .map_err(|e| e.to_string())?;
        println!(
            "    block={} currentPrice={} totalAmount={}",
            c.block, c.current_price, c.total_amount
        );
        Ok(())
    });

    section("Postage — batch lifecycle");
    let batch = match obtain_batch(&client).await {
        Ok(b) => {
            println!("  ok    obtained batch {}", b.to_hex());
            tally.ok();
            b
        }
        Err(e) => {
            println!("  FAIL  obtain batch — {e}");
            tally.err();
            print_summary(&tally);
            return if tally.fail > 0 {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            };
        }
    };
    check!(tally, "get_postage_batch", async {
        let b = client
            .postage()
            .get_postage_batch(&batch)
            .await
            .map_err(|e| e.to_string())?;
        println!(
            "    depth={} usable={} batchTTL={}",
            b.depth, b.usable, b.batch_ttl
        );
        Ok(())
    });

    section("Bytes — upload, probe, download");
    let payload = b"bee-rs integration-check payload".to_vec();
    let mut data_ref = None;
    check!(tally, "upload_data", async {
        let r = client
            .file()
            .upload_data(&batch, payload.clone(), None)
            .await
            .map_err(|e| e.to_string())?;
        println!("    reference={}", r.reference.to_hex());
        data_ref = Some(r.reference);
        Ok(())
    });
    if let Some(ref reference) = data_ref {
        check!(tally, "probe_data", async {
            let info = client
                .file()
                .probe_data(reference)
                .await
                .map_err(|e| e.to_string())?;
            if info.content_length as usize != payload.len() {
                return Err(format!(
                    "content_length {} != payload {}",
                    info.content_length,
                    payload.len()
                ));
            }
            Ok(())
        });
        check!(tally, "download_data", async {
            let body = client
                .file()
                .download_data(reference, None)
                .await
                .map_err(|e| e.to_string())?;
            if body.as_ref() != payload.as_slice() {
                return Err("downloaded payload mismatch".to_string());
            }
            Ok(())
        });
    } else {
        tally.skipped();
        tally.skipped();
        println!("  skip  probe_data + download_data (no upload reference)");
    }

    section("Bzz — file + collection");
    let mut file_ref = None;
    check!(tally, "upload_file", async {
        let r = client
            .file()
            .upload_file(&batch, b"hello".to_vec(), "hello.txt", "text/plain", None)
            .await
            .map_err(|e| e.to_string())?;
        println!("    reference={}", r.reference.to_hex());
        file_ref = Some(r.reference);
        Ok(())
    });
    if let Some(ref reference) = file_ref {
        check!(tally, "download_file", async {
            let (body, h) = client
                .file()
                .download_file(reference, None)
                .await
                .map_err(|e| e.to_string())?;
            if body.as_ref() != b"hello" {
                return Err("file body mismatch".to_string());
            }
            println!("    name={:?} content_type={:?}", h.name, h.content_type);
            Ok(())
        });
    } else {
        tally.skipped();
    }
    let entries = vec![
        CollectionEntry::new("index.html", b"<html>hi</html>".to_vec()),
        CollectionEntry::new("about.txt", b"about".to_vec()),
    ];
    check!(tally, "upload_collection_entries", async {
        let r = client
            .file()
            .upload_collection_entries(&batch, &entries, None)
            .await
            .map_err(|e| e.to_string())?;
        println!("    reference={}", r.reference.to_hex());
        Ok(())
    });

    section("Pin / tag");
    if let Some(ref reference) = data_ref {
        check!(tally, "pin", async {
            client.api().pin(reference).await.map_err(|e| e.to_string())
        });
        check!(tally, "get_pin", async {
            let pinned = client
                .api()
                .get_pin(reference)
                .await
                .map_err(|e| e.to_string())?;
            if !pinned {
                return Err("get_pin returned false right after pin".to_string());
            }
            Ok(())
        });
        check!(tally, "unpin", async {
            client
                .api()
                .unpin(reference)
                .await
                .map_err(|e| e.to_string())
        });
    } else {
        tally.skipped();
        tally.skipped();
        tally.skipped();
        println!("  skip  pin/get_pin/unpin (no upload reference)");
    }
    check!(tally, "create_tag + delete_tag", async {
        let t = client.api().create_tag().await.map_err(|e| e.to_string())?;
        client
            .api()
            .delete_tag(t.uid)
            .await
            .map_err(|e| e.to_string())
    });

    section("Feeds + SOC");
    let signer = signer_from_env_or_random();
    let owner = signer
        .public_key()
        .map_err(|e| eprintln!("public_key: {e}"))
        .ok()
        .map(|pk| pk.address());
    check!(tally, "feed update + fetch", async {
        let topic = Topic::from_string("bee-rs-integration");
        client
            .file()
            .update_feed(&batch, &signer, &topic, b"feed-payload")
            .await
            .map_err(|e| e.to_string())?;
        let _ = client
            .file()
            .fetch_latest_feed_update(owner.as_ref().expect("owner derived from signer"), &topic)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    });

    section("PSS (HTTP send only)");
    check!(tally, "pss send", async {
        let topic = Topic::from_string("bee-rs-pss");
        client
            .pss()
            .send(&batch, &topic, "00", b"ping".to_vec(), None)
            .await
            .map_err(|e| e.to_string())
    });

    section("GSOC (send only)");
    let id = Identifier::from_string("bee-rs-gsoc");
    check!(tally, "gsoc send", async {
        client
            .gsoc()
            .send(&batch, &signer, &id, b"gsoc-payload", None)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    });

    print_summary(&tally);
    if tally.fail > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn print_summary(tally: &Tally) {
    println!(
        "\n--- summary: {} ok, {} fail, {} skipped ---",
        tally.pass, tally.fail, tally.skip
    );
}

async fn obtain_batch(client: &Client) -> Result<BatchId, String> {
    if let Ok(hex) = env::var("BEE_BATCH_ID") {
        return BatchId::from_hex(&hex).map_err(|e| format!("BEE_BATCH_ID: {e}"));
    }
    let amount = env::var("BEE_BATCH_AMOUNT").unwrap_or_else(|_| "100000000".to_string());
    let depth: u8 = env::var("BEE_BATCH_DEPTH")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(17);
    let amount: BigInt = amount
        .parse()
        .map_err(|e| format!("BEE_BATCH_AMOUNT: {e}"))?;
    println!(
        "  Buying batch (depth={depth}, amount={amount}) — first usability is slow on Sepolia."
    );
    let id = client
        .postage()
        .create_postage_batch(&amount, depth, Some("bee-rs-integration"))
        .await
        .map_err(|e| e.to_string())?;
    // Bee needs a few seconds before the batch is usable.
    tokio::time::sleep(Duration::from_secs(3)).await;
    Ok(id)
}

fn signer_from_env_or_random() -> PrivateKey {
    if let Ok(hex) = env::var("BEE_SIGNER") {
        if let Ok(pk) = PrivateKey::from_hex(&hex) {
            return pk;
        }
    }
    PrivateKey::new(&[0x42; 32]).expect("32-byte zero key always valid")
}
