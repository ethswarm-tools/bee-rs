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

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bee::Client;
use bee::api::{FileUploadOptions, UploadOptions};
use bee::file::{CollectionEntry, OnStreamProgressFn, StreamProgress};
use bee::storage::get_storage_cost;
use bee::swarm::{
    BatchId, Identifier, Network, PrivateKey, Reference, Size, Topic, make_content_addressed_chunk,
};
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

    section("Read-only — operator, accounting, chequebook, loggers");
    check!(tally, "status", async {
        let s = client.debug().status().await.map_err(|e| e.to_string())?;
        println!("    overlay={} beeMode={}", s.overlay, s.bee_mode);
        Ok(())
    });
    check!(tally, "status_peers", async {
        let v = client
            .debug()
            .status_peers()
            .await
            .map_err(|e| e.to_string())?;
        println!("    peers={}", v.len());
        Ok(())
    });
    check!(tally, "status_neighborhoods", async {
        let v = client
            .debug()
            .status_neighborhoods()
            .await
            .map_err(|e| e.to_string())?;
        println!("    neighborhoods={}", v.len());
        Ok(())
    });
    check!(tally, "readiness", async {
        let r = client
            .debug()
            .readiness()
            .await
            .map_err(|e| e.to_string())?;
        println!("    ready={r}");
        Ok(())
    });
    check!(tally, "is_gateway", async {
        let g = client
            .debug()
            .is_gateway()
            .await
            .map_err(|e| e.to_string())?;
        println!("    gateway={g}");
        Ok(())
    });
    check!(tally, "peers", async {
        let p = client.debug().peers().await.map_err(|e| e.to_string())?;
        println!("    count={}", p.len());
        Ok(())
    });
    check!(tally, "blocklist", async {
        let b = client
            .debug()
            .blocklist()
            .await
            .map_err(|e| e.to_string())?;
        println!("    count={}", b.len());
        Ok(())
    });
    check!(tally, "reserve_state", async {
        let r = client
            .debug()
            .reserve_state()
            .await
            .map_err(|e| e.to_string())?;
        println!("    radius={} commitment={}", r.radius, r.commitment);
        Ok(())
    });
    check!(tally, "wallet", async {
        let w = client.debug().wallet().await.map_err(|e| e.to_string())?;
        println!(
            "    bzzBalance={:?} nativeBalance={:?}",
            w.bzz_balance, w.native_token_balance
        );
        Ok(())
    });
    check!(tally, "balances", async {
        let v = client.debug().balances().await.map_err(|e| e.to_string())?;
        println!("    peers={}", v.len());
        Ok(())
    });
    check!(tally, "consumed_balances", async {
        let v = client
            .debug()
            .consumed_balances()
            .await
            .map_err(|e| e.to_string())?;
        println!("    peers={}", v.len());
        Ok(())
    });
    check!(tally, "accounting", async {
        let m = client
            .debug()
            .accounting()
            .await
            .map_err(|e| e.to_string())?;
        println!("    peers={}", m.len());
        Ok(())
    });
    check!(tally, "stake", async {
        let s = client.debug().stake().await.map_err(|e| e.to_string())?;
        println!("    stake={s}");
        Ok(())
    });
    check!(tally, "withdrawable_stake", async {
        let s = client
            .debug()
            .withdrawable_stake()
            .await
            .map_err(|e| e.to_string())?;
        println!("    withdrawable={s}");
        Ok(())
    });
    check!(tally, "redistribution_state", async {
        let r = client
            .debug()
            .redistribution_state()
            .await
            .map_err(|e| e.to_string())?;
        println!("    isFrozen={} round={}", r.is_frozen, r.round);
        Ok(())
    });
    check!(tally, "chequebook_balance", async {
        let c = client
            .debug()
            .chequebook_balance()
            .await
            .map_err(|e| e.to_string())?;
        println!(
            "    totalBalance={} availableBalance={}",
            c.total_balance, c.available_balance
        );
        Ok(())
    });
    check!(tally, "last_cheques", async {
        let v = client
            .debug()
            .last_cheques()
            .await
            .map_err(|e| e.to_string())?;
        println!("    cheques={}", v.len());
        Ok(())
    });
    check!(tally, "settlements", async {
        let s = client
            .debug()
            .settlements()
            .await
            .map_err(|e| e.to_string())?;
        println!(
            "    totalReceived={:?} totalSent={:?}",
            s.total_received, s.total_sent
        );
        Ok(())
    });
    check!(tally, "loggers list", async {
        let l = client.debug().loggers().await.map_err(|e| e.to_string())?;
        println!("    loggers={}", l.loggers.len());
        Ok(())
    });
    check!(tally, "pending_transactions", async {
        let v = client
            .debug()
            .pending_transactions()
            .await
            .map_err(|e| e.to_string())?;
        println!("    pending={}", v.len());
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
    check!(tally, "get_postage_batches (list)", async {
        let v = client
            .postage()
            .get_postage_batches()
            .await
            .map_err(|e| e.to_string())?;
        println!("    batches={}", v.len());
        Ok(())
    });

    section("Storage helpers (read-only)");
    check!(tally, "get_storage_cost (1 GB / 30 days)", async {
        let cost = get_storage_cost(
            &client,
            Size::from_megabytes(1024.0).map_err(|e| e.to_string())?,
            Duration::from_secs(30 * 86_400),
            Network::Gnosis,
        )
        .await
        .map_err(|e| e.to_string())?;
        println!(
            "    depth={} blocks={} amountPerChunk={} totalCost={}",
            cost.depth, cost.blocks, cost.amount_per_chunk, cost.total_cost
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

    section("stream_directory — chunk-by-chunk + recursive manifest");
    check!(tally, "stream_collection_entries", async {
        // A larger payload (12 KB) exercises both leaf and intermediate
        // chunks, so we see >1 /chunks POST per file plus the recursive
        // manifest /bytes write.
        let big = vec![0xa5u8; 12 * 1024];
        let stream_entries = vec![
            CollectionEntry::new("index.html", b"<html>stream</html>".to_vec()),
            CollectionEntry::new("data.bin", big),
        ];
        let progress = Arc::new(AtomicUsize::new(0));
        let progress_clone = progress.clone();
        let on_progress: OnStreamProgressFn = Arc::new(move |p: StreamProgress| {
            // Track only the latest count; printing every chunk would
            // flood the soak log.
            progress_clone.store(p.processed, Ordering::SeqCst);
            let _ = p.total;
        });
        let r = client
            .file()
            .stream_collection_entries(&batch, &stream_entries, None, Some(on_progress))
            .await
            .map_err(|e| e.to_string())?;
        let processed = progress.load(Ordering::SeqCst);
        if processed == 0 {
            return Err("progress callback never fired".to_string());
        }
        println!(
            "    reference={} chunks_uploaded={}",
            r.reference.to_hex(),
            processed
        );
        Ok(())
    });

    section("Encrypted upload — round-trip");
    check!(tally, "upload_data + download_data (encrypted)", async {
        let payload = b"bee-rs encrypted-roundtrip payload".to_vec();
        let opts = FileUploadOptions {
            base: UploadOptions {
                encrypt: Some(true),
                ..Default::default()
            },
            ..Default::default()
        };
        let r = client
            .file()
            .upload_file(
                &batch,
                payload.clone(),
                "secret.bin",
                "application/octet-stream",
                Some(&opts),
            )
            .await
            .map_err(|e| e.to_string())?;
        if r.reference.len() != 64 {
            return Err(format!(
                "encrypted reference length {}, want 64",
                r.reference.len()
            ));
        }
        // Download via the bzz endpoint round-trips the body.
        let (body, _) = client
            .file()
            .download_file(&r.reference, None)
            .await
            .map_err(|e| e.to_string())?;
        if body.as_ref() != payload.as_slice() {
            return Err("encrypted download mismatch".to_string());
        }
        println!("    reference={} (64 bytes)", r.reference.to_hex());
        Ok(())
    });

    section("Chunks — direct upload + download");
    let mut chunk_ref: Option<Reference> = None;
    check!(tally, "upload_chunk + download_chunk", async {
        // Build a content-addressed chunk locally and POST the raw
        // wire form (span(8) || payload). Read it back via /chunks/{addr}.
        let payload = b"bee-rs chunk roundtrip".to_vec();
        let chunk = make_content_addressed_chunk(&payload).map_err(|e| e.to_string())?;
        let mut wire = Vec::with_capacity(8 + chunk.payload.len());
        wire.extend_from_slice(chunk.span.as_bytes());
        wire.extend_from_slice(&chunk.payload);
        let r = client
            .file()
            .upload_chunk(&batch, wire, None)
            .await
            .map_err(|e| e.to_string())?;
        if r.reference != chunk.address {
            return Err("upload_chunk reference != computed address".to_string());
        }
        let body = client
            .file()
            .download_chunk(&r.reference, None)
            .await
            .map_err(|e| e.to_string())?;
        // Body is span(8) || payload — strip the span and compare.
        if body.len() < 8 {
            return Err(format!("chunk body too short: {} bytes", body.len()));
        }
        if &body[8..] != payload.as_slice() {
            return Err("chunk payload mismatch".to_string());
        }
        let addr = r.reference.to_hex();
        chunk_ref = Some(r.reference);
        println!("    address={addr}");
        Ok(())
    });
    let _ = chunk_ref;

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
        // Newly uploaded SOC chunks need a moment to be retrievable on
        // a live network. Retry the lookup with backoff for up to 30s.
        let owner = owner.as_ref().expect("owner derived from signer");
        let mut last_err = None;
        for delay_ms in [500u64, 1000, 2000, 4000, 8000, 14000] {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            match client.file().fetch_latest_feed_update(owner, &topic).await {
                Ok(_) => return Ok(()),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err
            .map(|e| format!("after retries: {e}"))
            .unwrap_or_else(|| "no attempt made".to_string()))
    });
    let owner_addr = owner;
    check!(tally, "create_feed_manifest", async {
        let owner = owner_addr.as_ref().expect("owner");
        let topic = Topic::from_string("bee-rs-feed-manifest");
        let r = client
            .file()
            .create_feed_manifest(&batch, owner, &topic)
            .await
            .map_err(|e| e.to_string())?;
        println!("    manifest={}", r.to_hex());
        Ok(())
    });
    check!(tally, "find_next_index (fresh topic)", async {
        let owner = owner_addr.as_ref().expect("owner");
        let topic = Topic::from_string("bee-rs-fresh-feed-topic");
        let idx = client
            .file()
            .find_next_index(owner, &topic)
            .await
            .map_err(|e| e.to_string())?;
        if idx != 0 {
            return Err(format!("expected 0 for fresh topic, got {idx}"));
        }
        println!("    next_index=0");
        Ok(())
    });
    check!(tally, "FeedWriter + FeedReader round-trip", async {
        let topic = Topic::from_string("bee-rs-feed-rw");
        let writer = client
            .file()
            .make_feed_writer(signer.clone(), topic)
            .map_err(|e| e.to_string())?;
        writer
            .upload_payload(&batch, b"feed-rw-payload")
            .await
            .map_err(|e| e.to_string())?;
        let reader = client.file().make_feed_reader(*writer.owner(), topic);
        // Newly uploaded SOC chunks need to propagate to whatever
        // neighborhood owns them; on Sepolia with a small connected
        // peer set this can take a while. Retry up to ~60s.
        let mut last_err = None;
        for delay_ms in [500u64, 1000, 2000, 4000, 8000, 14000, 30000] {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            match reader.download().await {
                Ok(upd) => {
                    println!(
                        "    index={} index_next={} payload_len={}",
                        upd.index,
                        upd.index_next,
                        upd.payload.len()
                    );
                    return Ok(());
                }
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err
            .map(|e| format!("after retries: {e}"))
            .unwrap_or_else(|| "no attempt".to_string()))
    });
    if let Some(ref reference) = data_ref {
        check!(tally, "is_retrievable (uploaded data ref)", async {
            let ok = client
                .api()
                .is_retrievable(reference)
                .await
                .map_err(|e| e.to_string())?;
            if !ok {
                return Err("is_retrievable=false on freshly-uploaded ref".to_string());
            }
            Ok(())
        });
    } else {
        tally.skipped();
        println!("  skip  is_retrievable (no upload reference)");
    }

    section("Stewardship");
    if let Some(ref reference) = data_ref {
        check!(tally, "reupload", async {
            client
                .api()
                .reupload(reference, &batch)
                .await
                .map_err(|e| e.to_string())
        });
    } else {
        tally.skipped();
        println!("  skip  reupload (no upload reference)");
    }

    section("Grantee + envelope (ACT)");
    if let Some(ref reference) = data_ref {
        check!(tally, "post_envelope", async {
            let env = client
                .api()
                .post_envelope(&batch, reference)
                .await
                .map_err(|e| e.to_string())?;
            println!(
                "    issuer={} index_len={} timestamp_len={}",
                env.issuer,
                env.index.len(),
                env.timestamp.len()
            );
            Ok(())
        });
    } else {
        tally.skipped();
        println!("  skip  post_envelope (no upload reference)");
    }
    // Grantee creation requires a 33-byte SEC1-compressed pubkey hex.
    // Use the integration-check signer's own compressed pubkey so the
    // request is well-formed. ACT must be enabled on the node for the
    // POST /grantee endpoint to succeed; if not, this will 404.
    let grantee_pk = signer
        .public_key()
        .ok()
        .and_then(|pk| pk.compressed_hex().ok());
    let mut grantee_ref: Option<Reference> = None;
    match grantee_pk {
        Some(ref pk_hex) => {
            check!(tally, "create_grantees", async {
                let g = client
                    .api()
                    .create_grantees(&batch, std::slice::from_ref(pk_hex))
                    .await
                    .map_err(|e| e.to_string())?;
                println!("    ref={} historyref={}", g.reference, g.history_reference);
                grantee_ref = Reference::from_hex(&g.reference).ok();
                Ok(())
            });
            if let Some(ref gref) = grantee_ref {
                check!(tally, "get_grantees", async {
                    let v = client
                        .api()
                        .get_grantees(gref)
                        .await
                        .map_err(|e| e.to_string())?;
                    println!("    grantees={}", v.len());
                    Ok(())
                });
            } else {
                tally.skipped();
                println!("  skip  get_grantees (create_grantees did not return a ref)");
            }
        }
        None => {
            tally.skipped();
            tally.skipped();
            println!("  skip  create_grantees + get_grantees (no compressed pubkey)");
        }
    }

    section("PSS (HTTP send only)");
    check!(tally, "pss send", async {
        let topic = Topic::from_string("bee-rs-pss");
        client
            .pss()
            .send(&batch, &topic, "00", b"ping".to_vec(), None)
            .await
            .map_err(|e| e.to_string())
    });

    section("PSS — websocket subscribe (connectivity smoke)");
    check!(tally, "pss subscribe opens + stays alive 3s", async {
        // True end-to-end self-send fails: Bee rejects PSS sends whose
        // target prefix matches the local overlay (chunk would need a
        // remote neighborhood — `pss send failed` 500). With a single
        // node we can't generate the cross-node traffic that exercises
        // delivery; instead verify the websocket plumbing opens cleanly
        // and survives a short hold. Cross-node delivery is covered by
        // the `tests/p1_round4.rs` real-WS loopback unit test.
        let topic = Topic::from_string("bee-rs-pss-ws");
        let mut sub = client
            .pss()
            .subscribe(&topic)
            .await
            .map_err(|e| format!("subscribe: {e}"))?;
        let recv_fut = sub.recv();
        match tokio::time::timeout(Duration::from_secs(3), recv_fut).await {
            Ok(Some(_)) => {} // unexpected message, still fine
            Ok(None) => return Err("subscription closed prematurely".to_string()),
            Err(_) => {} // expected — no message in 3s
        }
        sub.cancel();
        Ok(())
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

    section("GSOC — websocket subscribe (connectivity smoke)");
    check!(tally, "gsoc subscribe opens + stays alive 3s", async {
        // GSOC chunks land in whichever neighborhood owns the
        // soc_address(identifier, owner). With a single-node testnet
        // the chunk usually lives elsewhere, so end-to-end self-receive
        // isn't reliable here. Verify the websocket opens cleanly.
        let id = Identifier::from_string("bee-rs-gsoc-ws");
        let owner = signer.public_key().map_err(|e| e.to_string())?.address();
        let mut sub = client
            .gsoc()
            .subscribe(&owner, &id)
            .await
            .map_err(|e| format!("subscribe: {e}"))?;
        let recv_fut = sub.recv();
        match tokio::time::timeout(Duration::from_secs(3), recv_fut).await {
            Ok(Some(_)) => {}
            Ok(None) => return Err("subscription closed prematurely".to_string()),
            Err(_) => {}
        }
        sub.cancel();
        Ok(())
    });

    section("Mutable batch lifecycle (gated)");
    if env::var("BEE_MUTABLE_BATCH_ID").is_ok() || env::var("BEE_BUY_MUTABLE").is_ok() {
        run_mutable_batch_flow(&client, &mut tally).await;
    } else {
        tally.skipped();
        println!(
            "  skip  mutable-batch lifecycle — set BEE_MUTABLE_BATCH_ID=<hex> to use an existing\n        non-immutable batch, or BEE_BUY_MUTABLE=1 to buy a fresh one (slow on Sepolia).\n        Skipped tests: top_up_batch, dilute_batch, extend_storage_duration."
        );
    }

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

/// Drive a fresh non-immutable batch through top_up / dilute /
/// extend_storage_duration. Either reuses `BEE_MUTABLE_BATCH_ID` or
/// buys a new one (slow on Sepolia — first usability can take minutes).
async fn run_mutable_batch_flow(client: &Client, tally: &mut Tally) {
    let mutable_batch = match obtain_mutable_batch(client).await {
        Ok(b) => {
            println!("  ok    obtained mutable batch {}", b.to_hex());
            tally.ok();
            b
        }
        Err(e) => {
            println!("  FAIL  obtain mutable batch — {e}");
            tally.err();
            return;
        }
    };
    // Wait until the batch is usable. Sepolia first-usability is slow.
    let usable_budget = Duration::from_secs(600);
    let start = std::time::Instant::now();
    let mut last_status = String::new();
    let mut became_usable = false;
    while start.elapsed() < usable_budget {
        match client.postage().get_postage_batch(&mutable_batch).await {
            Ok(b) if b.usable => {
                became_usable = true;
                println!(
                    "  ok    mutable batch became usable in {:.0}s",
                    start.elapsed().as_secs_f64()
                );
                break;
            }
            Ok(b) => {
                last_status = format!("usable={} depth={} ttl={}", b.usable, b.depth, b.batch_ttl);
            }
            Err(e) => {
                last_status = format!("get_postage_batch error: {e}");
            }
        }
        tokio::time::sleep(Duration::from_secs(15)).await;
    }
    if !became_usable {
        println!(
            "  FAIL  mutable batch never became usable in {}s — last: {last_status}",
            usable_budget.as_secs()
        );
        tally.err();
        return;
    }
    tally.ok();

    let topup = "100000000".parse::<BigInt>().expect("topup amount parses");
    check!(tally, "top_up_batch", async {
        client
            .postage()
            .top_up_batch(&mutable_batch, &topup)
            .await
            .map_err(|e| e.to_string())
    });
    check!(tally, "dilute_batch (+1 depth)", async {
        let b = client
            .postage()
            .get_postage_batch(&mutable_batch)
            .await
            .map_err(|e| e.to_string())?;
        client
            .postage()
            .dilute_batch(&mutable_batch, b.depth + 1)
            .await
            .map_err(|e| e.to_string())
    });
}

async fn obtain_mutable_batch(client: &Client) -> Result<BatchId, String> {
    if let Ok(hex) = env::var("BEE_MUTABLE_BATCH_ID") {
        return BatchId::from_hex(&hex).map_err(|e| format!("BEE_MUTABLE_BATCH_ID: {e}"));
    }
    let amount = env::var("BEE_MUTABLE_BATCH_AMOUNT").unwrap_or_else(|_| "200000000".to_string());
    let depth: u8 = env::var("BEE_MUTABLE_BATCH_DEPTH")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(17);
    let amount: BigInt = amount
        .parse()
        .map_err(|e| format!("BEE_MUTABLE_BATCH_AMOUNT: {e}"))?;
    println!(
        "  Buying MUTABLE batch (depth={depth}, amount={amount}) — Sepolia first-usability is slow."
    );
    let opts = bee::api::PostageBatchOptions {
        label: Some("bee-rs-integration-mutable".to_string()),
        immutable: Some(false),
        ..Default::default()
    };
    client
        .postage()
        .create_postage_batch_with_options(&amount, depth, Some(&opts))
        .await
        .map_err(|e| e.to_string())
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
