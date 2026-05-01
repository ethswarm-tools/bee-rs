//! P5 surface: FeedReader / FeedWriter, CID round-trips, Stamper,
//! upload-collection progress callback.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bee::Client;
use bee::api::CollectionUploadOptions;
use bee::file::StreamProgress;
use bee::postage::Stamper;
use bee::swarm::{
    BatchId, CidType, EthAddress, PrivateKey, Reference, Topic, convert_cid_to_reference,
    convert_reference_to_cid,
};
use serde_json::json;
use wiremock::matchers::{header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn batch() -> BatchId {
    BatchId::new(&[0xab; 32]).unwrap()
}

fn signer() -> PrivateKey {
    PrivateKey::new(&[0x33; 32]).unwrap()
}

// =====================================================================
// FeedReader / FeedWriter
// =====================================================================

#[tokio::test]
async fn feed_reader_download_returns_latest_update() {
    let server = MockServer::start().await;
    let owner = EthAddress::new(&[0xee; 20]).unwrap();
    let topic = Topic::from_string("feed-reader-test");

    Mock::given(method("GET"))
        .and(path(format!(
            "/feeds/{}/{}",
            owner.to_hex(),
            topic.to_hex()
        )))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("swarm-feed-index", "0000000000000007")
                .insert_header("swarm-feed-index-next", "0000000000000008")
                .set_body_bytes(b"payload".to_vec()),
        )
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let reader = client.file().make_feed_reader(owner, topic);
    let upd = reader.download().await.unwrap();
    assert_eq!(upd.index, 7);
    assert_eq!(upd.index_next, 8);
    assert_eq!(upd.payload.as_ref(), b"payload");
    assert_eq!(reader.next_index().await.unwrap(), 8);
}

#[tokio::test]
async fn feed_writer_upload_payload_uses_signer_owner() {
    let server = MockServer::start().await;
    let topic = Topic::from_string("feed-writer-test");
    let pk = signer();
    let owner = pk.public_key().unwrap().address();

    // First, the GET /feeds/{owner}/{topic} returns 404 → next index 0.
    Mock::given(method("GET"))
        .and(path(format!(
            "/feeds/{}/{}",
            owner.to_hex(),
            topic.to_hex()
        )))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    // Then the POST /soc/{owner}/{id} for the signed update.
    let owner_hex = owner.to_hex();
    Mock::given(method("POST"))
        .and(header_exists("Swarm-Postage-Batch-Id"))
        .and(wiremock::matchers::path_regex(format!(
            r"^/soc/{}/[0-9a-f]+$",
            owner_hex
        )))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(json!({ "reference": "aa".repeat(32) })),
        )
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let writer = client.file().make_feed_writer(pk, topic).unwrap();
    let result = writer
        .upload_payload(&batch(), b"hello-feed")
        .await
        .unwrap();
    assert_eq!(result.reference.to_hex(), "aa".repeat(32));
    assert_eq!(writer.owner(), &owner);
}

#[tokio::test]
async fn feed_writer_upload_reference_with_explicit_index_skips_lookup() {
    let server = MockServer::start().await;
    let topic = Topic::from_string("feed-writer-explicit");
    let pk = signer();
    let owner = pk.public_key().unwrap().address();
    let target = Reference::from_hex(&"cd".repeat(32)).unwrap();

    // No GET /feeds mock — the explicit index path must skip it.
    Mock::given(method("POST"))
        .and(header_exists("Swarm-Postage-Batch-Id"))
        .and(wiremock::matchers::path_regex(format!(
            r"^/soc/{}/[0-9a-f]+$",
            owner.to_hex()
        )))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(json!({ "reference": "ef".repeat(32) })),
        )
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let writer = client.file().make_feed_writer(pk, topic).unwrap();
    let result = writer
        .upload_reference(&batch(), &target, Some(42))
        .await
        .unwrap();
    assert_eq!(result.reference.to_hex(), "ef".repeat(32));
}

// =====================================================================
// CID encode / decode round-trip
// =====================================================================

#[test]
fn cid_round_trip_feed_and_manifest() {
    let hex = "ca6357a08e317d15ec560fef34e4c45f8f19f01c75d6f20a7021602e9575a617";
    let r = Reference::from_hex(hex).unwrap();

    let feed_cid = convert_reference_to_cid(&r, CidType::Feed).unwrap();
    let dec = convert_cid_to_reference(&feed_cid).unwrap();
    assert_eq!(dec.kind, CidType::Feed);
    assert_eq!(dec.reference.to_hex(), hex);

    let manifest_cid = convert_reference_to_cid(&r, CidType::Manifest).unwrap();
    let dec = convert_cid_to_reference(&manifest_cid).unwrap();
    assert_eq!(dec.kind, CidType::Manifest);
    assert_eq!(dec.reference.to_hex(), hex);

    // The two CIDs must differ (different codec byte).
    assert_ne!(feed_cid, manifest_cid);
    assert!(feed_cid.starts_with('b'));
    assert!(manifest_cid.starts_with('b'));
}

// =====================================================================
// Stamper smoke test (exercises public API beyond unit tests)
// =====================================================================

#[test]
fn stamper_persists_and_resumes_state() {
    let mut a = Stamper::from_blank(signer(), batch(), 18).unwrap();
    a.stamp(&[0u8; 32]).unwrap();
    a.stamp(&[1u8; 32]).unwrap();
    let snapshot = a.state().to_vec();

    let b = Stamper::from_state(signer(), batch(), snapshot, 18).unwrap();
    // Bucket 0 should have height 1, bucket 0x0101 should have height 1.
    assert_eq!(b.state()[0], 1);
    assert_eq!(b.state()[0x0101], 1);
    assert_eq!(b.depth(), 18);
    assert_eq!(b.max_slot(), 4);
}

// =====================================================================
// upload_collection progress callback
// =====================================================================

#[tokio::test]
async fn upload_collection_invokes_progress_callback() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bzz"))
        .and(header_exists("Swarm-Postage-Batch-Id"))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(json!({ "reference": "11".repeat(32) })),
        )
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"alpha").unwrap();
    std::fs::write(dir.path().join("b.txt"), b"beta").unwrap();

    let observed = Arc::new(AtomicUsize::new(0));
    let observed_clone = observed.clone();
    let names: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
    let names_clone = names.clone();

    let opts = CollectionUploadOptions::default().with_on_entry(move |entry| {
        observed_clone.fetch_add(1, Ordering::SeqCst);
        names_clone.lock().unwrap().push(entry.path.to_string());
    });

    let client = Client::new(&server.uri()).unwrap();
    let path: PathBuf = dir.path().to_path_buf();
    client
        .file()
        .upload_collection(&batch(), &path, Some(&opts))
        .await
        .unwrap();

    assert_eq!(observed.load(Ordering::SeqCst), 2);
    let mut got = names.lock().unwrap().clone();
    got.sort();
    assert_eq!(got, vec!["a.txt".to_string(), "b.txt".to_string()]);
}

// =====================================================================
// stream_directory: per-chunk uploads + recursive manifest persist
// =====================================================================

#[tokio::test]
async fn stream_directory_uploads_chunks_then_manifest() {
    let server = MockServer::start().await;

    // Per-chunk uploads to /chunks. Bee returns the address it
    // computed; for this test we just echo a stable hex — the
    // client doesn't re-verify the address against the body.
    let chunks_seen = Arc::new(AtomicUsize::new(0));
    let chunks_seen_clone = chunks_seen.clone();
    Mock::given(method("POST"))
        .and(path("/chunks"))
        .and(header_exists("Swarm-Postage-Batch-Id"))
        .respond_with(move |_: &wiremock::Request| {
            chunks_seen_clone.fetch_add(1, Ordering::SeqCst);
            ResponseTemplate::new(201).set_body_json(json!({ "reference": "cc".repeat(32) }))
        })
        .mount(&server)
        .await;

    // Manifest serialization goes through /bytes. There can be
    // multiple if the manifest fans out; for two short files it's
    // exactly one node.
    let bytes_seen = Arc::new(AtomicUsize::new(0));
    let bytes_seen_clone = bytes_seen.clone();
    Mock::given(method("POST"))
        .and(path("/bytes"))
        .and(header_exists("Swarm-Postage-Batch-Id"))
        .respond_with(move |_: &wiremock::Request| {
            bytes_seen_clone.fetch_add(1, Ordering::SeqCst);
            ResponseTemplate::new(201).set_body_json(json!({ "reference": "ee".repeat(32) }))
        })
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"alpha").unwrap();
    std::fs::write(dir.path().join("index.html"), b"<html/>").unwrap();

    let last: Arc<std::sync::Mutex<Option<StreamProgress>>> = Arc::new(std::sync::Mutex::new(None));
    let last_clone = last.clone();
    let on_progress: bee::file::OnStreamProgressFn = Arc::new(move |p: StreamProgress| {
        *last_clone.lock().unwrap() = Some(p);
    });

    let client = Client::new(&server.uri()).unwrap();
    let result = client
        .file()
        .stream_directory(&batch(), dir.path(), None, Some(on_progress))
        .await
        .unwrap();

    // Manifest reference flows back from the last /bytes response.
    assert_eq!(result.reference.to_hex(), "ee".repeat(32));

    // Two single-chunk files → 2 chunk uploads, no intermediates.
    assert_eq!(chunks_seen.load(Ordering::SeqCst), 2);
    // At least one manifest write (the root).
    assert!(bytes_seen.load(Ordering::SeqCst) >= 1);

    // Final progress callback should report processed == total == 2.
    let last = last.lock().unwrap().expect("progress callback fired");
    assert_eq!(last.total, 2);
    assert_eq!(last.processed, 2);
}
