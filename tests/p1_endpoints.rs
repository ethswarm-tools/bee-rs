//! End-to-end tests for the P1 endpoint surface using wiremock.
//!
//! These tests exercise the full request path: URL construction,
//! header preparation, JSON parsing, error mapping. Each test stands
//! up a fresh `MockServer` and points a `Client` at it.

use bee::api::{DownloadOptions, RedundancyLevel, RedundantUploadOptions, UploadOptions};
use bee::postage::{
    PostageBatch, get_depth_for_size, get_stamp_cost, get_stamp_effective_bytes,
    get_stamp_theoretical_bytes,
};
use bee::swarm::{BatchId, Reference};
use bee::{Client, Error};
use num_bigint::BigInt;
use serde_json::json;
use wiremock::matchers::{header, header_exists, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn batch() -> BatchId {
    BatchId::new(&[0xab; 32]).unwrap()
}

fn reference32() -> Reference {
    Reference::new(&[0xee; 32]).unwrap()
}

// =====================================================================
// File / data
// =====================================================================

#[tokio::test]
async fn upload_data_sends_batch_header_and_returns_reference() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bytes"))
        .and(header("Swarm-Postage-Batch-Id", "ab".repeat(32).as_str()))
        .and(header("Content-Type", "application/octet-stream"))
        .respond_with(
            ResponseTemplate::new(201)
                .insert_header("Swarm-Tag", "42")
                .set_body_json(json!({ "reference": "ee".repeat(32) })),
        )
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let result = client
        .file()
        .upload_data(&batch(), b"hello swarm".to_vec(), None)
        .await
        .unwrap();
    assert_eq!(result.reference, reference32());
    assert_eq!(result.tag_uid, Some(42));
    assert_eq!(result.history_address, None);
}

#[tokio::test]
async fn upload_data_emits_redundancy_and_pin_headers() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bytes"))
        .and(header("Swarm-Pin", "true"))
        .and(header("Swarm-Redundancy-Level", "2"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "reference": "ee".repeat(32),
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let opts = RedundantUploadOptions {
        base: UploadOptions {
            pin: Some(true),
            ..Default::default()
        },
        redundancy_level: Some(RedundancyLevel::Strong),
    };
    let r = client
        .file()
        .upload_data(&batch(), b"x".to_vec(), Some(&opts))
        .await
        .unwrap();
    assert_eq!(r.reference, reference32());
}

#[tokio::test]
async fn upload_data_maps_4xx_to_response_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/bytes"))
        .respond_with(ResponseTemplate::new(422).set_body_string("invalid postage batch"))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let err = client
        .file()
        .upload_data(&batch(), b"x".to_vec(), None)
        .await
        .unwrap_err();
    match &err {
        Error::Response { status, body, .. } => {
            assert_eq!(*status, 422);
            assert_eq!(body, b"invalid postage batch");
        }
        _ => panic!("expected Response, got {err:?}"),
    }
    assert_eq!(err.status(), Some(422));
}

#[tokio::test]
async fn download_data_returns_body_bytes() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/bytes/{}", "ee".repeat(32))))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"hello".to_vec()))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let body = client
        .file()
        .download_data(&reference32(), None)
        .await
        .unwrap();
    assert_eq!(body.as_ref(), b"hello");
}

#[tokio::test]
async fn download_data_emits_redundancy_strategy_header() {
    use bee::api::RedundancyStrategy;
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/bytes/{}", "ee".repeat(32))))
        .and(header("Swarm-Redundancy-Strategy", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"ok".to_vec()))
        .mount(&server)
        .await;
    let client = Client::new(&server.uri()).unwrap();
    let opts = DownloadOptions {
        redundancy_strategy: Some(RedundancyStrategy::Data),
        ..Default::default()
    };
    let body = client
        .file()
        .download_data(&reference32(), Some(&opts))
        .await
        .unwrap();
    assert_eq!(body.as_ref(), b"ok");
}

#[tokio::test]
async fn probe_data_parses_content_length() {
    let server = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path(format!("/bytes/{}", "ee".repeat(32))))
        .respond_with(ResponseTemplate::new(200).insert_header("Content-Length", "1024"))
        .mount(&server)
        .await;
    let client = Client::new(&server.uri()).unwrap();
    let info = client.file().probe_data(&reference32()).await.unwrap();
    assert_eq!(info.content_length, 1024);
}

// =====================================================================
// File / chunk
// =====================================================================

#[tokio::test]
async fn upload_chunk_round_trips() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chunks"))
        .and(header_exists("Swarm-Postage-Batch-Id"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "reference": "ee".repeat(32),
        })))
        .mount(&server)
        .await;
    let client = Client::new(&server.uri()).unwrap();
    let r = client
        .file()
        .upload_chunk(&batch(), b"chunkbytes".to_vec(), None)
        .await
        .unwrap();
    assert_eq!(r.reference, reference32());
}

#[tokio::test]
async fn download_chunk_returns_bytes() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/chunks/{}", "ee".repeat(32))))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"chunk!".to_vec()))
        .mount(&server)
        .await;
    let client = Client::new(&server.uri()).unwrap();
    let body = client
        .file()
        .download_chunk(&reference32(), None)
        .await
        .unwrap();
    assert_eq!(body.as_ref(), b"chunk!");
}

// =====================================================================
// Postage
// =====================================================================

#[tokio::test]
async fn get_postage_batches_decodes_amount_as_bigint() {
    let server = MockServer::start().await;
    let body = json!({
        "stamps": [{
            "batchID": "ab".repeat(32),
            "amount": "12345678901234567890",
            "depth": 17,
            "bucketDepth": 16,
            "immutableFlag": false,
            "batchTTL": 3600,
            "utilization": 0,
            "usable": true,
            "exists": true,
        }],
    });
    Mock::given(method("GET"))
        .and(path("/stamps"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    let client = Client::new(&server.uri()).unwrap();
    let batches = client.postage().get_postage_batches().await.unwrap();
    assert_eq!(batches.len(), 1);
    let b: &PostageBatch = &batches[0];
    assert_eq!(b.batch_id, batch());
    assert_eq!(b.depth, 17);
    assert_eq!(
        b.amount.as_ref().unwrap(),
        &"12345678901234567890".parse::<BigInt>().unwrap()
    );
    assert!(b.usable);
}

#[tokio::test]
async fn get_postage_batch_decodes_immutable_flag_field_name() {
    // Bee returns the field as `immutableFlag`, not `immutable`. bee-go
    // got this wrong on first cut and the field was always false.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/stamps/{}", "ab".repeat(32))))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "batchID": "ab".repeat(32),
            "amount": "100",
            "depth": 17,
            "bucketDepth": 16,
            "immutableFlag": true,
            "batchTTL": 0,
        })))
        .mount(&server)
        .await;
    let client = Client::new(&server.uri()).unwrap();
    let b = client.postage().get_postage_batch(&batch()).await.unwrap();
    assert!(b.immutable);
}

#[tokio::test]
async fn create_postage_batch_uses_label_query() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/stamps/100/17"))
        .and(query_param("label", "my-batch"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "batchID": "ab".repeat(32),
        })))
        .mount(&server)
        .await;
    let client = Client::new(&server.uri()).unwrap();
    let id = client
        .postage()
        .create_postage_batch(&BigInt::from(100), 17, Some("my-batch"))
        .await
        .unwrap();
    assert_eq!(id, batch());
}

#[tokio::test]
async fn top_up_and_dilute_send_patch() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(format!("/stamps/topup/{}/500", "ab".repeat(32))))
        .respond_with(ResponseTemplate::new(202))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(format!("/stamps/dilute/{}/22", "ab".repeat(32))))
        .respond_with(ResponseTemplate::new(202))
        .mount(&server)
        .await;
    let client = Client::new(&server.uri()).unwrap();
    client
        .postage()
        .top_up_batch(&batch(), &BigInt::from(500))
        .await
        .unwrap();
    client.postage().dilute_batch(&batch(), 22).await.unwrap();
}

// =====================================================================
// Postage stamp math (no I/O)
// =====================================================================

#[test]
fn stamp_math_known_values() {
    assert_eq!(get_stamp_theoretical_bytes(17), 536_870_912);
    assert_eq!(get_stamp_cost(3, &BigInt::from(100)), BigInt::from(800));
    assert_eq!(get_stamp_effective_bytes(17), 40_890);
    assert_eq!(get_depth_for_size(1_000_000), 18);
}

// =====================================================================
// Debug
// =====================================================================

#[tokio::test]
async fn health_returns_versions() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "ok",
            "version": "2.7.1-61fab37b",
            "apiVersion": "7.4.1",
        })))
        .mount(&server)
        .await;
    let client = Client::new(&server.uri()).unwrap();
    let h = client.debug().health().await.unwrap();
    assert_eq!(h.status, "ok");
    assert_eq!(h.version, "2.7.1-61fab37b");
    assert_eq!(h.api_version, "7.4.1");
}

#[tokio::test]
async fn is_supported_api_version_compares_against_constant() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "ok",
            "version": "2.7.1-61fab37b",
            "apiVersion": "7.4.1",
        })))
        .mount(&server)
        .await;
    let client = Client::new(&server.uri()).unwrap();
    assert!(client.debug().is_supported_api_version().await.unwrap());
    assert!(client.debug().is_supported_exact_version().await.unwrap());
}

#[tokio::test]
async fn chain_state_decodes_bigint_strings() {
    // Bee returns currentPrice / totalAmount as decimal strings, not
    // numbers — bee-go's first cut tried to decode them as uint64 and
    // every chainstate call failed. Verify our custom deserializer.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/chainstate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "block": 12345,
            "chainTip": "0xabc",
            "currentPrice": "24000",
            "totalAmount": "999999999999999999999",
        })))
        .mount(&server)
        .await;
    let client = Client::new(&server.uri()).unwrap();
    let cs = client.debug().chain_state().await.unwrap();
    assert_eq!(cs.block, 12345);
    assert_eq!(cs.current_price, BigInt::from(24_000));
    assert_eq!(
        cs.total_amount,
        "999999999999999999999".parse::<BigInt>().unwrap()
    );
}

#[tokio::test]
async fn response_error_captures_method_url_status_body() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(503).set_body_string("not ready"))
        .mount(&server)
        .await;
    let client = Client::new(&server.uri()).unwrap();
    let err = client.debug().health().await.unwrap_err();
    match err {
        Error::Response {
            method,
            url,
            status,
            body,
            ..
        } => {
            assert_eq!(method, "GET");
            assert!(url.ends_with("/health"));
            assert_eq!(status, 503);
            assert_eq!(body, b"not ready");
        }
        _ => panic!("expected Response error"),
    }
}
