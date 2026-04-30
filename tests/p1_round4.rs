//! End-to-end tests for the round-4 P1 surface: collection upload
//! from disk + offline hashing, chequebook / settlements / wallet /
//! loggers, PSS send + websocket receive, GSOC send, BeeDev wrapper,
//! storage extension previews.

use std::time::Duration;

use bee::Client;
use bee::dev::DevClient;
use bee::file::{CollectionEntry, hash_collection_entries, hash_directory};
use bee::storage::{
    calculate_top_up_for_bzz, get_duration_extension_cost, get_size_extension_cost,
};
use bee::swarm::{BatchId, Identifier, Network, PrivateKey, Size, Topic};
use futures_util::SinkExt;
use num_bigint::BigInt;
use serde_json::json;
use tokio_tungstenite::tungstenite::protocol::Message;
use wiremock::matchers::{header, header_exists, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn batch() -> BatchId {
    BatchId::new(&[0xab; 32]).unwrap()
}

// =====================================================================
// Filesystem-walked collection upload + offline hashing
// =====================================================================

#[tokio::test]
async fn upload_collection_walks_filesystem() {
    let server = MockServer::start().await;
    let expected_ref = "33".repeat(32);
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

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"hi").unwrap();
    std::fs::create_dir_all(dir.path().join("nested")).unwrap();
    std::fs::write(dir.path().join("nested/b.bin"), [0u8, 1, 2]).unwrap();

    let client = Client::new(&server.uri()).unwrap();
    let r = client
        .file()
        .upload_collection(&batch(), dir.path(), None)
        .await
        .unwrap();
    assert_eq!(r.reference.to_hex(), expected_ref);
}

#[test]
fn hash_directory_matches_in_memory_hash() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"alpha").unwrap();
    std::fs::create_dir_all(dir.path().join("nested")).unwrap();
    std::fs::write(dir.path().join("nested/b.bin"), [0u8, 1, 2]).unwrap();

    let from_disk = hash_directory(dir.path()).unwrap();
    let from_memory = hash_collection_entries(&[
        CollectionEntry::new("a.txt", b"alpha".to_vec()),
        CollectionEntry::new("nested/b.bin", vec![0u8, 1, 2]),
    ])
    .unwrap();

    assert_eq!(from_disk, from_memory);
}

// =====================================================================
// Chequebook / settlements / wallet / loggers
// =====================================================================

#[tokio::test]
async fn wallet_parses_bigint_balances() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wallet"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "bzzAddress": "0xbzz",
            "nativeAddress": "0xnat",
            "chequebook": "0xcb",
            "bzzBalance": "1000",
            "nativeTokenBalance": "2000",
            "chainID": 100,
            "walletAddress": "0xw",
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let w = client.debug().wallet().await.unwrap();
    assert_eq!(w.bzz_address, "0xbzz");
    assert_eq!(w.bzz_balance.unwrap(), BigInt::from(1000));
    assert_eq!(w.native_token_balance.unwrap(), BigInt::from(2000));
    assert_eq!(w.chain_id, 100);
}

#[tokio::test]
async fn chequebook_balance_and_deposit() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/chequebook/balance"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "totalBalance": "500",
            "availableBalance": "300",
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chequebook/deposit"))
        .and(query_param("amount", "100"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"transactionHash": "0xdep"})))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let bal = client.debug().chequebook_balance().await.unwrap();
    assert_eq!(bal.total_balance, BigInt::from(500));
    assert_eq!(bal.available_balance, BigInt::from(300));

    let h = client
        .debug()
        .chequebook_deposit(&BigInt::from(100))
        .await
        .unwrap();
    assert_eq!(h, "0xdep");
}

#[tokio::test]
async fn cashout_last_cheque_sends_gas_price_header() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chequebook/cashout/peer1"))
        .and(header("gas-price", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"transactionHash": "0xch"})))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let h = client
        .debug()
        .cashout_last_cheque("peer1", Some(&BigInt::from(5)))
        .await
        .unwrap();
    assert_eq!(h, "0xch");
}

#[tokio::test]
async fn settlements_total_and_per_peer() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/settlements"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "totalReceived": "100",
            "totalSent": "200",
            "settlements": [
                {"peer": "p1", "received": "50", "sent": "60"},
            ],
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let s = client.debug().settlements().await.unwrap();
    assert_eq!(s.total_received.unwrap(), BigInt::from(100));
    assert_eq!(s.total_sent.unwrap(), BigInt::from(200));
    assert_eq!(s.settlements.len(), 1);
    assert_eq!(
        s.settlements[0].received.as_ref().unwrap(),
        &BigInt::from(50)
    );
}

#[tokio::test]
async fn loggers_list_and_set_verbosity() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/loggers"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "tree": {},
            "loggers": [
                {"logger": "node/api", "verbosity": "info", "subsystem": "node", "id": "abc"}
            ]
        })))
        .mount(&server)
        .await;
    // base64("node/api") = "bm9kZS9hcGk="
    Mock::given(method("PUT"))
        .and(path("/loggers/bm9kZS9hcGk="))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/loggers/bm9kZS9hcGk="))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "tree": {},
            "loggers": []
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let listing = client.debug().loggers().await.unwrap();
    assert_eq!(listing.loggers.len(), 1);
    assert_eq!(listing.loggers[0].verbosity, "info");

    client
        .debug()
        .set_logger_verbosity("node/api")
        .await
        .unwrap();
    let by_expr = client
        .debug()
        .loggers_by_expression("node/api")
        .await
        .unwrap();
    assert!(by_expr.loggers.is_empty());
}

// =====================================================================
// PSS send (HTTP)
// =====================================================================

#[tokio::test]
async fn pss_send_includes_topic_target_and_batch_header() {
    let server = MockServer::start().await;
    let topic = Topic::from_string("greetings");
    Mock::given(method("POST"))
        .and(path(format!("/pss/send/{}/12ab", topic.to_hex())))
        .and(header_exists("Swarm-Postage-Batch-Id"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    client
        .pss()
        .send(&batch(), &topic, "12ab", b"hello".to_vec(), None)
        .await
        .unwrap();
}

// =====================================================================
// PSS subscribe (websocket loopback)
// =====================================================================

#[tokio::test]
async fn pss_subscribe_yields_messages_pushed_by_server() {
    use std::net::SocketAddr;
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let topic = Topic::from_string("greetings");
    let path_topic = topic.to_hex();

    let server_task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        // Validate the HTTP upgrade path and forward two messages.
        let path_topic = path_topic.clone();
        let cb =
            move |req: &tokio_tungstenite::tungstenite::handshake::server::Request,
                  resp: tokio_tungstenite::tungstenite::handshake::server::Response| {
                let want = format!("/pss/subscribe/{path_topic}");
                assert_eq!(req.uri().path(), want);
                Ok(resp)
            };
        let mut ws = tokio_tungstenite::accept_hdr_async(stream, cb)
            .await
            .unwrap();
        ws.send(Message::Binary(b"first".to_vec())).await.unwrap();
        ws.send(Message::Binary(b"second".to_vec())).await.unwrap();
        let _ = ws.close(None).await;
    });

    let client = Client::new(&format!("http://{addr}")).unwrap();
    let mut sub = client.pss().subscribe(&topic).await.unwrap();
    let m1 = sub.recv().await.unwrap();
    let m2 = sub.recv().await.unwrap();
    assert_eq!(m1.as_ref(), b"first");
    assert_eq!(m2.as_ref(), b"second");
    server_task.await.unwrap();
}

// =====================================================================
// GSOC send
// =====================================================================

#[tokio::test]
async fn gsoc_send_uploads_to_soc_endpoint_at_signer_address() {
    let server = MockServer::start().await;
    let signer = PrivateKey::new(&[0x55; 32]).unwrap();
    let owner = signer.public_key().unwrap().address();
    let id = Identifier::new(&[0x21; 32]).unwrap();
    let expected_ref = "44".repeat(32);

    Mock::given(method("POST"))
        .and(path(format!("/soc/{}/{}", owner.to_hex(), id.to_hex())))
        .and(header_exists("Swarm-Postage-Batch-Id"))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(json!({ "reference": expected_ref.clone() })),
        )
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let r = client
        .gsoc()
        .send(&batch(), &signer, &id, b"payload", None)
        .await
        .unwrap();
    assert_eq!(r.reference.to_hex(), expected_ref);

    // Pure helper: same address as crate::swarm::calculate_single_owner_chunk_address.
    let addr = client.gsoc().soc_address(&id, &owner).unwrap();
    assert_eq!(addr.as_bytes().len(), 32);
}

// =====================================================================
// DevClient (thin wrapper)
// =====================================================================

#[tokio::test]
async fn dev_client_derefs_to_full_client() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "ok",
            "version": "2.7.1-dev",
            "apiVersion": "7.4.1",
        })))
        .mount(&server)
        .await;

    let dev = DevClient::new(&server.uri()).unwrap();
    let h = dev.debug().health().await.unwrap();
    assert_eq!(h.status, "ok");
}

// =====================================================================
// Storage extension cost previews
// =====================================================================

fn batch_response(batch: &BatchId, depth: u8, amount: &str) -> serde_json::Value {
    json!({
        "batchID": batch.to_hex(),
        "utilization": 0,
        "usable": true,
        "label": "",
        "depth": depth,
        "amount": amount,
        "bucketDepth": 16,
        "blockNumber": 0,
        "immutableFlag": false,
        "exists": true,
        "batchTTL": 100,
        "start": 0,
        "owner": "0x00",
    })
}

#[tokio::test]
async fn duration_extension_cost_combines_chain_state_and_batch() {
    let server = MockServer::start().await;
    let id = batch();
    Mock::given(method("GET"))
        .and(path(format!("/stamps/{}", id.to_hex())))
        .respond_with(ResponseTemplate::new(200).set_body_json(batch_response(&id, 18, "120")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/chainstate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "block": 1,
            "chainTip": "0x0",
            "currentPrice": "10",
            "totalAmount": "0",
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    // 60s on Gnosis = 12 blocks, per-chunk = 120, depth 18 → 2^18 * 120 = 31_457_280
    let cost = get_duration_extension_cost(&client, &id, Duration::from_secs(60), Network::Gnosis)
        .await
        .unwrap();
    assert_eq!(cost, BigInt::from(31_457_280u64));
}

#[tokio::test]
async fn size_extension_cost_zero_when_already_deep_enough() {
    let server = MockServer::start().await;
    let id = batch();
    Mock::given(method("GET"))
        .and(path(format!("/stamps/{}", id.to_hex())))
        .respond_with(ResponseTemplate::new(200).set_body_json(batch_response(&id, 22, "10")))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    // 1 MB needs depth 18; current is 22 → no dilution needed.
    let cost = get_size_extension_cost(&client, &id, Size::from_megabytes(1.0).unwrap())
        .await
        .unwrap();
    assert_eq!(cost, BigInt::from(0));
}

#[tokio::test]
async fn size_extension_cost_grows_with_depth_delta() {
    let server = MockServer::start().await;
    let id = batch();
    Mock::given(method("GET"))
        .and(path(format!("/stamps/{}", id.to_hex())))
        .respond_with(ResponseTemplate::new(200).set_body_json(batch_response(&id, 17, "10")))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    // 1 MB needs depth 18; current is 17 → (2^18 - 2^17) * 10 = 131072 * 10 = 1_310_720.
    let cost = get_size_extension_cost(&client, &id, Size::from_megabytes(1.0).unwrap())
        .await
        .unwrap();
    assert_eq!(cost, BigInt::from(1_310_720u64));
}

#[tokio::test]
async fn calculate_top_up_for_bzz_returns_difference() {
    let server = MockServer::start().await;
    let id = batch();
    Mock::given(method("GET"))
        .and(path(format!("/stamps/{}", id.to_hex())))
        .respond_with(ResponseTemplate::new(200).set_body_json(batch_response(&id, 18, "100")))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    assert_eq!(
        calculate_top_up_for_bzz(&client, &id, &BigInt::from(250))
            .await
            .unwrap(),
        BigInt::from(150)
    );
    assert_eq!(
        calculate_top_up_for_bzz(&client, &id, &BigInt::from(50))
            .await
            .unwrap(),
        BigInt::from(0)
    );
}
