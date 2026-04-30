//! End-to-end tests for the round-3 P1 surface: bzz file/collection
//! uploads, debug peers/transactions/accounting/stake, grantee +
//! envelope, top-level storage helpers.

use std::time::Duration;

use bee::Client;
use bee::file::CollectionEntry;
use bee::storage::{StorageOptions, buy_storage, extend_storage_size, get_storage_cost};
use bee::swarm::{BatchId, Network, Reference, Size};
use num_bigint::BigInt;
use serde_json::json;
use wiremock::matchers::{body_partial_json, header, header_exists, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn batch() -> BatchId {
    BatchId::new(&[0xab; 32]).unwrap()
}

fn reference() -> Reference {
    Reference::from_hex(&"cd".repeat(32)).unwrap()
}

// =====================================================================
// /bzz file upload + download
// =====================================================================

#[tokio::test]
async fn upload_file_sends_name_query_and_content_type() {
    let server = MockServer::start().await;
    let expected_ref = "11".repeat(32);

    Mock::given(method("POST"))
        .and(path("/bzz"))
        .and(query_param("name", "hello.txt"))
        .and(header("Content-Type", "text/plain"))
        .and(header_exists("Swarm-Postage-Batch-Id"))
        .respond_with(
            ResponseTemplate::new(201)
                .set_body_json(json!({ "reference": expected_ref.clone() }))
                .insert_header("Swarm-Tag", "9"),
        )
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let r = client
        .file()
        .upload_file(&batch(), b"hi".to_vec(), "hello.txt", "text/plain", None)
        .await
        .unwrap();
    assert_eq!(r.reference.to_hex(), expected_ref);
    assert_eq!(r.tag_uid, Some(9));
}

#[tokio::test]
async fn upload_file_defaults_octet_stream_when_no_content_type() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bzz"))
        .and(header("Content-Type", "application/octet-stream"))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(json!({ "reference": "00".repeat(32) })),
        )
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    client
        .file()
        .upload_file(&batch(), b"x".to_vec(), "", "", None)
        .await
        .unwrap();
}

#[tokio::test]
async fn download_file_returns_body_and_parses_headers() {
    let server = MockServer::start().await;
    let r = reference();
    Mock::given(method("GET"))
        .and(path(format!("/bzz/{}", r.to_hex())))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "text/plain")
                .insert_header("Content-Disposition", "attachment; filename=\"a.txt\"")
                .insert_header("Swarm-Tag-Uid", "11")
                .set_body_bytes(b"file body".to_vec()),
        )
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let (body, headers) = client.file().download_file(&r, None).await.unwrap();
    assert_eq!(body.as_ref(), b"file body");
    assert_eq!(headers.name.as_deref(), Some("a.txt"));
    assert_eq!(headers.content_type.as_deref(), Some("text/plain"));
    assert_eq!(headers.tag_uid, Some(11));
}

#[tokio::test]
async fn download_file_path_serves_collection_entry() {
    let server = MockServer::start().await;
    let r = reference();
    Mock::given(method("GET"))
        .and(path(format!("/bzz/{}/nested/file.bin", r.to_hex())))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "application/octet-stream")
                .set_body_bytes(vec![0xaa, 0xbb, 0xcc]),
        )
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let (body, _) = client
        .file()
        .download_file_path(&r, "nested/file.bin", None)
        .await
        .unwrap();
    assert_eq!(body.as_ref(), &[0xaa, 0xbb, 0xcc]);
}

#[tokio::test]
async fn upload_collection_entries_sends_tar_and_collection_header() {
    let server = MockServer::start().await;
    let expected_ref = "22".repeat(32);
    Mock::given(method("POST"))
        .and(path("/bzz"))
        .and(header("Content-Type", "application/x-tar"))
        .and(header("Swarm-Collection", "true"))
        .and(header_exists("Swarm-Postage-Batch-Id"))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(json!({ "reference": expected_ref.clone() })),
        )
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let entries = vec![
        CollectionEntry::new("index.html", b"<html/>".to_vec()),
        CollectionEntry::new("nested/data.bin", b"\x00\x01\x02".to_vec()),
    ];
    let r = client
        .file()
        .upload_collection_entries(&batch(), &entries, None)
        .await
        .unwrap();
    assert_eq!(r.reference.to_hex(), expected_ref);
}

// =====================================================================
// Grantee + envelope
// =====================================================================

#[tokio::test]
async fn get_grantees_returns_list() {
    // Live Bee returns a bare JSON array, not a `{ "grantees": [...] }`
    // wrapper. (Caught by the P4 soak — see CHANGELOG.)
    let server = MockServer::start().await;
    let r = reference();
    Mock::given(method("GET"))
        .and(path(format!("/grantee/{}", r.to_hex())))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!(["02aa", "03bb"])))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let g = client.api().get_grantees(&r).await.unwrap();
    assert_eq!(g, vec!["02aa".to_string(), "03bb".to_string()]);
}

#[tokio::test]
async fn create_grantees_posts_body_and_batch_header() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/grantee"))
        .and(header_exists("Swarm-Postage-Batch-Id"))
        .and(body_partial_json(json!({ "grantees": ["02aa"] })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "ref": "ab".repeat(32),
            "historyref": "cd".repeat(32),
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let r = client
        .api()
        .create_grantees(&batch(), &["02aa".to_string()])
        .await
        .unwrap();
    assert_eq!(r.reference, "ab".repeat(32));
    assert_eq!(r.history_reference, "cd".repeat(32));
}

#[tokio::test]
async fn patch_grantees_sends_history_and_act_headers() {
    let server = MockServer::start().await;
    let r = reference();
    let history = Reference::from_hex(&"ef".repeat(32)).unwrap();
    Mock::given(method("PATCH"))
        .and(path(format!("/grantee/{}", r.to_hex())))
        .and(header_exists("Swarm-Postage-Batch-Id"))
        .and(header(
            "Swarm-Act-History-Address",
            history.to_hex().as_str(),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ref": "11".repeat(32),
            "historyref": "22".repeat(32),
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let g = client
        .api()
        .patch_grantees(&batch(), &r, &history, &["02aa".to_string()], &[])
        .await
        .unwrap();
    assert_eq!(g.reference, "11".repeat(32));
}

#[tokio::test]
async fn post_envelope_returns_quadruple() {
    let server = MockServer::start().await;
    let r = reference();
    Mock::given(method("POST"))
        .and(path(format!("/envelope/{}", r.to_hex())))
        .and(header_exists("Swarm-Postage-Batch-Id"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "issuer": "0xabc",
            "index": "0x01",
            "timestamp": "0x99",
            "signature": "0xdeadbeef",
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let e = client.api().post_envelope(&batch(), &r).await.unwrap();
    assert_eq!(e.issuer, "0xabc");
    assert_eq!(e.signature, "0xdeadbeef");
}

// =====================================================================
// Debug — peers, status, transactions, accounting, stake
// =====================================================================

#[tokio::test]
async fn peers_returns_list() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/peers"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "peers": [
                {"address": "aa", "fullNode": true},
                {"address": "bb", "fullNode": false},
            ]
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let peers = client.debug().peers().await.unwrap();
    assert_eq!(peers.len(), 2);
    assert_eq!(peers[0].address, "aa");
    assert!(peers[0].full_node);
    assert!(!peers[1].full_node);
}

#[tokio::test]
async fn ping_peer_returns_rtt_string() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/pingpong/abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"rtt": "2.5ms"})))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let rtt = client.debug().ping_peer("abc").await.unwrap();
    assert_eq!(rtt, "2.5ms");
}

#[tokio::test]
async fn connect_peer_strips_leading_slash() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/connect/dns/bee.example.com"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"address": "ovr"})))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let addr = client
        .debug()
        .connect_peer("/dns/bee.example.com")
        .await
        .unwrap();
    assert_eq!(addr, "ovr");
}

#[tokio::test]
async fn addresses_returns_overlay_underlay() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/addresses"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "overlay": "ovr",
            "underlay": ["/ip4/127.0.0.1/tcp/1634"],
            "ethereum": "0xeee",
            "publicKey": "0x02ab",
            "pssPublicKey": "0x02cd",
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let a = client.debug().addresses().await.unwrap();
    assert_eq!(a.overlay, "ovr");
    assert_eq!(a.underlay.len(), 1);
    assert_eq!(a.public_key, "0x02ab");
}

#[tokio::test]
async fn pending_transactions_lists_records() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/transactions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "pendingTransactions": [
                {
                    "transactionHash": "0xtx1",
                    "to": "0xeee",
                    "nonce": 17,
                    "gasPrice": "1000000000",
                    "gasLimit": 21000,
                    "gasTipBoost": 0,
                    "gasTipCap": "",
                    "gasFeeCap": "",
                    "data": "0x",
                    "created": "2026-04-30T00:00:00Z",
                    "description": "test",
                    "value": "0",
                }
            ]
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let txs = client.debug().pending_transactions().await.unwrap();
    assert_eq!(txs.len(), 1);
    assert_eq!(txs[0].transaction_hash, "0xtx1");
    assert_eq!(txs[0].nonce, 17);
    assert_eq!(
        txs[0].gas_price.as_ref().unwrap(),
        &BigInt::from(1_000_000_000u64)
    );
    assert!(txs[0].gas_tip_cap.is_none());
}

#[tokio::test]
async fn cancel_transaction_sends_gas_price_header() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/transactions/0xabc"))
        .and(header("gas-price", "5000000000"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"transactionHash": "0xnew"})))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let h = client
        .debug()
        .cancel_transaction("0xabc", Some(&BigInt::from(5_000_000_000u64)))
        .await
        .unwrap();
    assert_eq!(h, "0xnew");
}

#[tokio::test]
async fn balances_parses_bigint_strings() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/balances"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "balances": [
                {"peer": "p1", "balance": "1000000000000000000"},
                {"peer": "p2", "balance": "0"},
            ]
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let b = client.debug().balances().await.unwrap();
    assert_eq!(b.len(), 2);
    assert_eq!(b[0].peer, "p1");
    assert_eq!(
        b[0].balance,
        "1000000000000000000".parse::<BigInt>().unwrap()
    );
    assert_eq!(b[1].balance, BigInt::from(0));
}

#[tokio::test]
async fn accounting_parses_per_peer_data() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/accounting"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "peerData": {
                "p1": {
                    "balance": "100",
                    "consumedBalance": "50",
                    "thresholdReceived": "200",
                    "thresholdGiven": "200",
                    "currentThresholdReceived": "150",
                    "currentThresholdGiven": "180",
                    "surplusBalance": "10",
                    "reservedBalance": "5",
                    "shadowReservedBalance": "0",
                    "ghostBalance": "1",
                }
            }
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let a = client.debug().accounting().await.unwrap();
    let p1 = a.get("p1").unwrap();
    assert_eq!(p1.balance.as_ref().unwrap(), &BigInt::from(100));
    assert_eq!(p1.surplus_balance.as_ref().unwrap(), &BigInt::from(10));
}

#[tokio::test]
async fn stake_get_and_deposit() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/stake"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"stakedAmount": "12345"})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/stake/777"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"txHash": "0xstk"})))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    assert_eq!(client.debug().stake().await.unwrap(), BigInt::from(12_345));
    let h = client
        .debug()
        .deposit_stake(&BigInt::from(777))
        .await
        .unwrap();
    assert_eq!(h, "0xstk");
}

#[tokio::test]
async fn node_info_and_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/node"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "beeMode": "full",
            "chequebookEnabled": true,
            "swapEnabled": true,
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "overlay": "ovr",
            "proximity": 2,
            "beeMode": "full",
            "reserveSize": 0,
            "reserveSizeWithinRadius": 0,
            "pullsyncRate": 1.5,
            "storageRadius": 4,
            "connectedPeers": 10,
            "neighborhoodSize": 5,
            "batchCommitment": 0,
            "isReachable": true,
            "lastSyncedBlock": 100,
            "committedDepth": 4,
            "isWarmingUp": false,
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let n = client.debug().node_info().await.unwrap();
    assert_eq!(n.bee_mode, "full");
    assert!(n.chequebook_enabled);

    let s = client.debug().status().await.unwrap();
    assert_eq!(s.overlay, "ovr");
    assert_eq!(s.connected_peers, 10);
    assert!((s.pullsync_rate - 1.5).abs() < f64::EPSILON);
}

// =====================================================================
// Top-level storage helpers
// =====================================================================

#[tokio::test]
async fn get_storage_cost_combines_chain_state_and_size() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/chainstate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "block": 1,
            "chainTip": 1,
            "currentPrice": "10",
            "totalAmount": "0",
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    // 1 MB at depth-18 (per stamp_math table). Duration 1 minute on
    // Gnosis = 12 blocks. Per-chunk amount = 10 * 12 = 120.
    let cost = get_storage_cost(
        &client,
        Size::from_megabytes(1.0).unwrap(),
        Duration::from_secs(60),
        Network::Gnosis,
    )
    .await
    .unwrap();
    assert_eq!(cost.depth, 18);
    assert_eq!(cost.blocks, 12);
    assert_eq!(cost.amount_per_chunk, BigInt::from(120));
    // total = 2^18 * 120 = 31_457_280
    assert_eq!(cost.total_cost, BigInt::from(31_457_280u64));
}

#[tokio::test]
async fn buy_storage_creates_postage_batch_with_computed_amount() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/chainstate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "block": 1,
            "chainTip": 1,
            "currentPrice": "10",
            "totalAmount": "0",
        })))
        .mount(&server)
        .await;
    // depth-18, amount 120 → /stamps/120/18
    Mock::given(method("POST"))
        .and(path("/stamps/120/18"))
        .and(query_param("label", "demo"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"batchID": "ab".repeat(32)})))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let id = buy_storage(
        &client,
        Size::from_megabytes(1.0).unwrap(),
        Duration::from_secs(60),
        &StorageOptions {
            network: Network::Gnosis,
            label: Some("demo".into()),
            immutable: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(id.to_hex(), "ab".repeat(32));
}

#[tokio::test]
async fn extend_storage_size_dilutes_when_target_deeper() {
    let server = MockServer::start().await;
    let id = batch();
    Mock::given(method("GET"))
        .and(path(format!("/stamps/{}", id.to_hex())))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "batchID": id.to_hex(),
            "utilization": 0,
            "usable": true,
            "label": "",
            "depth": 17,
            "amount": "1",
            "bucketDepth": 16,
            "blockNumber": 0,
            "immutableFlag": false,
            "exists": true,
            "batchTTL": 100,
        })))
        .mount(&server)
        .await;
    // 1 MB needs depth 18 → dilute from 17 to 18
    Mock::given(method("PATCH"))
        .and(path(format!("/stamps/dilute/{}/18", id.to_hex())))
        .respond_with(ResponseTemplate::new(202))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    extend_storage_size(&client, &id, Size::from_megabytes(1.0).unwrap())
        .await
        .unwrap();
}
