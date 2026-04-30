//! End-to-end tests for the round-2 P1 surface: SOC upload, feeds,
//! pin / tag / stewardship.

use bee::api::Tag;
use bee::file::{feed_update_chunk_reference, make_feed_identifier};
use bee::swarm::{
    BatchId, EthAddress, Identifier, PrivateKey, Reference, Signature, Topic,
    calculate_single_owner_chunk_address,
};
use bee::{Client, Error};
use serde_json::json;
use wiremock::matchers::{header, header_exists, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn batch() -> BatchId {
    BatchId::new(&[0xab; 32]).unwrap()
}

fn signer() -> PrivateKey {
    PrivateKey::new(&[0x33; 32]).unwrap()
}

// =====================================================================
// SOC upload + reader/writer
// =====================================================================

#[tokio::test]
async fn upload_soc_uses_owner_id_path_and_sig_query() {
    let server = MockServer::start().await;
    let owner = EthAddress::new(&[0xee; 20]).unwrap();
    let id = Identifier::new(&[0x11; 32]).unwrap();
    let sig = Signature::new(&[0x42; 65]).unwrap();
    let expected_ref = "ee".repeat(32);

    Mock::given(method("POST"))
        .and(path(format!("/soc/{}/{}", owner.to_hex(), id.to_hex())))
        .and(query_param("sig", sig.to_hex().as_str()))
        .and(header_exists("Swarm-Postage-Batch-Id"))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(json!({ "reference": expected_ref })),
        )
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let r = client
        .file()
        .upload_soc(&batch(), &owner, &id, &sig, b"hello".to_vec(), None)
        .await
        .unwrap();
    assert_eq!(r.reference.to_hex(), expected_ref);
}

#[tokio::test]
async fn soc_writer_signs_and_uploads_at_computed_address() {
    let server = MockServer::start().await;
    let signer_key = signer();
    let owner = signer_key.public_key().unwrap().address();
    let id = Identifier::from_string("integration");

    // Bee accepts any /soc/{owner}/{id} POST and echoes back the SOC
    // address as the reference. Our SocWriter is responsible for
    // computing the right URL and signing the body — the wiremock
    // mock just confirms the URL.
    let expected_ref = calculate_single_owner_chunk_address(&id, &owner)
        .unwrap()
        .to_hex();
    Mock::given(method("POST"))
        .and(path(format!("/soc/{}/{}", owner.to_hex(), id.to_hex())))
        .and(query_param_exists("sig"))
        .respond_with(
            ResponseTemplate::new(201)
                .set_body_json(json!({ "reference": expected_ref.clone() })),
        )
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let writer = client.file().make_soc_writer(signer_key).unwrap();
    let r = writer.upload(&batch(), &id, b"signed", None).await.unwrap();
    assert_eq!(r.reference.to_hex(), expected_ref);
}

// `query_param_exists` isn't in wiremock's prelude — provide it inline.
fn query_param_exists(name: &'static str) -> impl wiremock::Match {
    struct Exists(&'static str);
    impl wiremock::Match for Exists {
        fn matches(&self, request: &wiremock::Request) -> bool {
            request.url.query_pairs().any(|(k, _)| k == self.0)
        }
    }
    Exists(name)
}

// =====================================================================
// Feeds
// =====================================================================

#[test]
fn feed_identifier_is_keccak_topic_index() {
    use bee::swarm::bmt::keccak256;
    let topic = Topic::from_string("my-feed");
    let id = make_feed_identifier(&topic, 7);

    let mut input = Vec::new();
    input.extend_from_slice(topic.as_bytes());
    input.extend_from_slice(&7u64.to_be_bytes());
    let want = keccak256(&input);
    assert_eq!(id.as_bytes(), &want);
}

#[test]
fn feed_chunk_reference_matches_soc_address() {
    let owner = EthAddress::new(&[0xaa; 20]).unwrap();
    let topic = Topic::from_string("ref-feed");
    let r = feed_update_chunk_reference(&owner, &topic, 0).unwrap();
    let id = make_feed_identifier(&topic, 0);
    let want = calculate_single_owner_chunk_address(&id, &owner).unwrap();
    assert_eq!(r, want);
}

#[tokio::test]
async fn fetch_latest_feed_update_parses_indexes_from_headers() {
    let server = MockServer::start().await;
    let owner = EthAddress::new(&[0xee; 20]).unwrap();
    let topic = Topic::from_string("feed");

    Mock::given(method("GET"))
        .and(path(format!("/feeds/{}/{}", owner.to_hex(), topic.to_hex())))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("swarm-feed-index", "0000000000000003")
                .insert_header("swarm-feed-index-next", "0000000000000004")
                .set_body_bytes(b"payload-bytes".to_vec()),
        )
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let upd = client
        .file()
        .fetch_latest_feed_update(&owner, &topic)
        .await
        .unwrap();
    assert_eq!(upd.index, 3);
    assert_eq!(upd.index_next, 4);
    assert_eq!(upd.payload.as_ref(), b"payload-bytes");
}

#[tokio::test]
async fn find_next_index_returns_zero_on_404() {
    let server = MockServer::start().await;
    let owner = EthAddress::new(&[0xee; 20]).unwrap();
    let topic = Topic::from_string("empty-feed");

    Mock::given(method("GET"))
        .and(path(format!("/feeds/{}/{}", owner.to_hex(), topic.to_hex())))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let idx = client.file().find_next_index(&owner, &topic).await.unwrap();
    assert_eq!(idx, 0);
}

#[tokio::test]
async fn is_feed_retrievable_returns_false_on_404() {
    let server = MockServer::start().await;
    let owner = EthAddress::new(&[0xee; 20]).unwrap();
    let topic = Topic::from_string("missing");

    Mock::given(method("GET"))
        .and(path(format!("/feeds/{}/{}", owner.to_hex(), topic.to_hex())))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let ok = client
        .file()
        .is_feed_retrievable(&owner, &topic, None, None)
        .await
        .unwrap();
    assert!(!ok);
}

#[tokio::test]
async fn create_feed_manifest_returns_reference() {
    let server = MockServer::start().await;
    let owner = EthAddress::new(&[0xee; 20]).unwrap();
    let topic = Topic::from_string("create-feed");

    Mock::given(method("POST"))
        .and(path(format!("/feeds/{}/{}", owner.to_hex(), topic.to_hex())))
        .and(header("Swarm-Postage-Batch-Id", "ab".repeat(32).as_str()))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(json!({ "reference": "ee".repeat(32) })),
        )
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let r = client
        .file()
        .create_feed_manifest(&batch(), &owner, &topic)
        .await
        .unwrap();
    assert_eq!(r, Reference::new(&[0xee; 32]).unwrap());
}

// =====================================================================
// Pins
// =====================================================================

#[tokio::test]
async fn pin_unpin_round_trip() {
    let server = MockServer::start().await;
    let r = Reference::new(&[0x55; 32]).unwrap();
    Mock::given(method("POST"))
        .and(path(format!("/pins/{}", r.to_hex())))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(format!("/pins/{}", r.to_hex())))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    client.api().pin(&r).await.unwrap();
    client.api().unpin(&r).await.unwrap();
}

#[tokio::test]
async fn get_pin_returns_true_on_200_and_false_on_404() {
    let server = MockServer::start().await;
    let r = Reference::new(&[0x55; 32]).unwrap();
    Mock::given(method("GET"))
        .and(path(format!("/pins/{}", r.to_hex())))
        .respond_with(ResponseTemplate::new(200))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/pins/{}", r.to_hex())))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    assert!(client.api().get_pin(&r).await.unwrap());
    assert!(!client.api().get_pin(&r).await.unwrap());
}

#[tokio::test]
async fn list_pins_decodes_references() {
    let server = MockServer::start().await;
    let r1 = Reference::new(&[0x11; 32]).unwrap();
    let r2 = Reference::new(&[0x22; 32]).unwrap();
    Mock::given(method("GET"))
        .and(path("/pins"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "references": [r1.to_hex(), r2.to_hex()],
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let pins = client.api().list_pins().await.unwrap();
    assert_eq!(pins, vec![r1, r2]);
}

// =====================================================================
// Tags
// =====================================================================

#[tokio::test]
async fn create_tag_decodes_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/tags"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "uid": 7,
            "name": "demo",
            "total": 0,
            "split": 0,
            "seen": 0,
            "stored": 0,
            "sent": 0,
            "synced": 0,
            "address": "",
            "startedAt": "2026-04-30T12:00:00Z",
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let tag = client.api().create_tag().await.unwrap();
    assert_eq!(tag.uid, 7);
    assert_eq!(tag.name, "demo");
    assert_eq!(tag.started_at, "2026-04-30T12:00:00Z");
}

#[tokio::test]
async fn get_tag_uses_uid_in_path() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/tags/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "uid": 42,
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let tag = client.api().get_tag(42).await.unwrap();
    assert_eq!(tag.uid, 42);
}

#[tokio::test]
async fn list_tags_passes_offset_limit_query() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/tags"))
        .and(query_param("offset", "10"))
        .and(query_param("limit", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "tags": [{"uid": 1}, {"uid": 2}],
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let tags = client
        .api()
        .list_tags(Some(10), Some(5))
        .await
        .unwrap();
    assert_eq!(tags.len(), 2);
    assert_eq!(tags[0].uid, 1);
}

#[tokio::test]
async fn delete_and_update_tag() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/tags/1"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/tags/2"))
        .and(header("Content-Type", "application/json"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    client.api().delete_tag(1).await.unwrap();
    let tag = Tag {
        uid: 2,
        name: "renamed".into(),
        ..Default::default()
    };
    client.api().update_tag(2, &tag).await.unwrap();
}

// =====================================================================
// Stewardship
// =====================================================================

#[tokio::test]
async fn reupload_sends_batch_id_and_put() {
    let server = MockServer::start().await;
    let r = Reference::new(&[0x99; 32]).unwrap();
    Mock::given(method("PUT"))
        .and(path(format!("/stewardship/{}", r.to_hex())))
        .and(header("swarm-postage-batch-id", "ab".repeat(32).as_str()))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    client.api().reupload(&r, &batch()).await.unwrap();
}

#[tokio::test]
async fn is_retrievable_decodes_camel_case_field() {
    let server = MockServer::start().await;
    let r = Reference::new(&[0x88; 32]).unwrap();
    Mock::given(method("GET"))
        .and(path(format!("/stewardship/{}", r.to_hex())))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "isRetrievable": true,
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    assert!(client.api().is_retrievable(&r).await.unwrap());
}

#[tokio::test]
async fn is_retrievable_propagates_5xx() {
    let server = MockServer::start().await;
    let r = Reference::new(&[0x77; 32]).unwrap();
    Mock::given(method("GET"))
        .and(path(format!("/stewardship/{}", r.to_hex())))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let err = client.api().is_retrievable(&r).await.unwrap_err();
    match err {
        Error::Response { status, .. } => assert_eq!(status, 503),
        _ => panic!("expected Response error"),
    }
}
