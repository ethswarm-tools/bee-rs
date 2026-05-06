//! v1.3.0 surface: ergonomics (`Client::ping`, `Client::with_token`,
//! tracing on `Inner::send`) and the new endpoints (`/timesettlements`,
//! `/rchash`, `/chunks/stream` WS upload).

use bee::swarm::BatchId;
use bee::{Client, Error};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio_tungstenite::tungstenite::protocol::Message;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn batch() -> BatchId {
    BatchId::new(&[0xab; 32]).unwrap()
}

// =====================================================================
// Phase A: ergonomics
// =====================================================================

#[tokio::test]
async fn ping_returns_a_non_zero_duration_on_success() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "ok",
            "version": "2.7.2-test",
            "apiVersion": "8.0.0"
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let elapsed = client.ping().await.expect("ping should succeed");
    // Loopback wiremock latency varies but is always non-zero; assert
    // shape only — concrete bounds would be flaky in CI.
    assert!(elapsed.as_nanos() > 0, "elapsed should advance");
}

#[tokio::test]
async fn ping_propagates_response_error_on_non_2xx() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(503).set_body_string("starting"))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let err = client.ping().await.expect_err("ping should fail on 5xx");
    match err {
        Error::Response { status, .. } => assert_eq!(status, 503),
        other => panic!("expected Error::Response, got {other:?}"),
    }
}

#[tokio::test]
async fn with_token_sends_bearer_authorization_on_every_request() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .and(header("Authorization", "Bearer secret-jwt-123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "ok",
            "version": "2.7.2-test",
            "apiVersion": "8.0.0"
        })))
        .mount(&server)
        .await;

    let client = Client::with_token(&server.uri(), "secret-jwt-123").unwrap();
    // health() goes through the same Inner::send as everything else;
    // if the header isn't attached, wiremock will return 404 and the
    // call fails.
    let h = client
        .debug()
        .health()
        .await
        .expect("auth header must be sent");
    assert_eq!(h.status, "ok");
}

#[tokio::test]
async fn with_token_rejects_token_with_invalid_header_chars() {
    // A newline cannot appear in an HTTP header value.
    let err = Client::with_token("http://localhost:1633", "bad\ntoken")
        .expect_err("invalid header chars must be rejected");
    match err {
        Error::Argument { message } => assert!(
            message.contains("invalid token"),
            "unexpected message: {message}"
        ),
        other => panic!("expected Error::Argument, got {other:?}"),
    }
}

// =====================================================================
// Phase B: /timesettlements + /rchash
// =====================================================================

#[tokio::test]
async fn time_settlements_parses_settlements_payload() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/timesettlements"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "totalReceived": "1000",
            "totalSent": "2500",
            "settlements": [
                {
                    "peer": "1234",
                    "received": "500",
                    "sent": "1500"
                },
                {
                    "peer": "5678",
                    "received": "500",
                    "sent": "1000"
                }
            ]
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let s = client
        .debug()
        .time_settlements()
        .await
        .expect("time_settlements should succeed");
    assert_eq!(s.total_received.as_ref().unwrap().to_string(), "1000");
    assert_eq!(s.total_sent.as_ref().unwrap().to_string(), "2500");
    assert_eq!(s.settlements.len(), 2);
}

#[tokio::test]
async fn r_chash_parses_full_response() {
    let server = MockServer::start().await;
    let depth = 8u8;
    let anchor1 = "aa".repeat(32);
    let anchor2 = "bb".repeat(32);
    let path_str = format!("/rchash/{depth}/{anchor1}/{anchor2}");
    Mock::given(method("GET"))
        .and(path(path_str.as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "durationSeconds": 12.4,
            "hash": "ee".repeat(32),
            "proofs": {
                "proof1": {
                    "chunkSpan": 4096,
                    "postageProof": {
                        "index": "0001",
                        "postageId": "ab".repeat(32),
                        "signature": "ff".repeat(65),
                        "timeStamp": "1700000000"
                    },
                    "proofSegments": ["seg-a", "seg-b"],
                    "proofSegments2": null,
                    "proofSegments3": null,
                    "proveSegment": "12345",
                    "proveSegment2": "67890",
                    "socProof": null
                },
                "proof2": {
                    "chunkSpan": 4096,
                    "postageProof": {
                        "index": "0002",
                        "postageId": "ab".repeat(32),
                        "signature": "ff".repeat(65),
                        "timeStamp": "1700000001"
                    },
                    "proofSegments": ["seg-c"],
                    "proofSegments2": null,
                    "proofSegments3": null,
                    "proveSegment": "abcde",
                    "proveSegment2": "fedcb",
                    "socProof": [{
                        "chunkAddr": "11".repeat(32),
                        "identifier": "id-1",
                        "signature": "ff".repeat(65),
                        "signer": "22".repeat(20)
                    }]
                },
                "proofLast": {
                    "chunkSpan": 4096,
                    "postageProof": {
                        "index": "0003",
                        "postageId": "ab".repeat(32),
                        "signature": "ff".repeat(65),
                        "timeStamp": "1700000002"
                    },
                    "proofSegments": ["seg-x"],
                    "proofSegments2": null,
                    "proofSegments3": null,
                    "proveSegment": "deadbeef",
                    "proveSegment2": "cafebabe",
                    "socProof": null
                }
            }
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let r = client
        .debug()
        .r_chash(depth, &anchor1, &anchor2)
        .await
        .expect("r_chash should succeed");
    assert!((r.duration_seconds - 12.4).abs() < 1e-9);
    assert_eq!(r.hash.len(), 64);
    assert_eq!(r.proofs.proof1.chunk_span, 4096);
    assert_eq!(r.proofs.proof1.proof_segments.as_deref().unwrap().len(), 2);
    assert!(r.proofs.proof1.soc_proof.is_none());
    let soc = r
        .proofs
        .proof2
        .soc_proof
        .as_deref()
        .expect("proof2 has soc_proof");
    assert_eq!(soc.len(), 1);
    assert_eq!(soc[0].identifier, "id-1");
    assert_eq!(r.proofs.proof_last.prove_segment, "deadbeef");
}

// =====================================================================
// Phase C: /chunks/stream websocket upload
// =====================================================================

#[tokio::test]
async fn chunks_stream_sends_chunks_and_consumes_acks() {
    use std::net::SocketAddr;
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let bid = batch();
    let bid_hex = bid.to_hex();

    let server_task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let bid_hex_for_cb = bid_hex.clone();
        // Validate the upgrade path, query, and batch-id header.
        #[allow(clippy::result_large_err)]
        let cb =
            move |req: &tokio_tungstenite::tungstenite::handshake::server::Request,
                  resp: tokio_tungstenite::tungstenite::handshake::server::Response| {
                assert_eq!(req.uri().path(), "/chunks/stream");
                assert_eq!(req.uri().query(), Some("swarm-tag=42"));
                let got = req
                    .headers()
                    .get("swarm-postage-batch-id")
                    .expect("batch id header missing");
                assert_eq!(got.to_str().unwrap(), bid_hex_for_cb);
                Ok(resp)
            };
        let mut ws = tokio_tungstenite::accept_hdr_async(stream, cb)
            .await
            .unwrap();

        // Receive two chunks, ack each with a single 0 byte.
        for expected in [b"chunk-one".as_slice(), b"chunk-two".as_slice()] {
            match ws.next().await.unwrap().unwrap() {
                Message::Binary(b) => assert_eq!(b, expected),
                other => panic!("expected binary chunk, got {other:?}"),
            }
            ws.send(Message::Binary(vec![0u8])).await.unwrap();
        }
        // Wait for the client's close frame and reply.
        let _ = ws.next().await;
        let _ = ws.close(None).await;
    });

    let client = Client::new(&format!("http://{addr}")).unwrap();
    let mut cs = client
        .file()
        .chunks_stream(&bid, Some(42))
        .await
        .expect("chunks_stream open");
    cs.send_chunk(b"chunk-one".to_vec()).await.unwrap();
    cs.send_chunk(b"chunk-two".to_vec()).await.unwrap();
    cs.close().await.unwrap();
    server_task.await.unwrap();
}

#[tokio::test]
async fn chunks_stream_propagates_text_error_frame() {
    use std::net::SocketAddr;
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let bid = batch();

    let server_task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        // Drain one chunk, reply with a text error frame.
        let _ = ws.next().await;
        ws.send(Message::Text("postage stamp invalid".into()))
            .await
            .unwrap();
        let _ = ws.close(None).await;
    });

    let client = Client::new(&format!("http://{addr}")).unwrap();
    let mut cs = client.file().chunks_stream(&bid, None).await.unwrap();
    let err = cs
        .send_chunk(b"chunk".to_vec())
        .await
        .expect_err("text frame must surface as error");
    match err {
        Error::Argument { message } => assert!(
            message.contains("postage stamp invalid"),
            "unexpected message: {message}"
        ),
        other => panic!("expected Error::Argument, got {other:?}"),
    }
    server_task.await.unwrap();
}

#[tokio::test]
async fn r_chash_handles_minimal_response() {
    // Bee can return a sparse object when proofs are unavailable;
    // ensure deserialization still succeeds.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rchash/0/00/00"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "durationSeconds": 0.0,
            "hash": "00".repeat(32),
            "proofs": {}
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let r = client
        .debug()
        .r_chash(0, "00", "00")
        .await
        .expect("r_chash should accept sparse proofs");
    assert_eq!(r.duration_seconds, 0.0);
    assert_eq!(r.proofs.proof1.chunk_span, 0);
}
