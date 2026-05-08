//! v1.6.1 security hardening pass: response body cap, query-string
//! redaction, CRLF rejection on collection upload options.

use bee::api::{CollectionUploadOptions, validate_collection_upload_options};
use bee::swarm::redact_url;
use url::Url;

#[test]
fn redact_url_strips_query_and_fragment() {
    let cases = [
        ("http://bee/path?token=secret", "http://bee/path"),
        ("http://bee/path?a=1&b=2#frag", "http://bee/path"),
        ("http://bee/path", "http://bee/path"),
        ("http://bee/", "http://bee/"),
    ];
    for (input, want) in cases {
        let u = Url::parse(input).unwrap();
        assert_eq!(redact_url(&u), want, "redact_url({input})");
    }
}

#[test]
fn validate_collection_upload_options_rejects_crlf() {
    for bad in [
        "foo\r\nX-Injected: bar",
        "foo\nbar",
        "foo\rbar",
        "foo\x00bar",
    ] {
        let opts = CollectionUploadOptions {
            index_document: Some(bad.to_owned()),
            ..Default::default()
        };
        assert!(
            validate_collection_upload_options(Some(&opts)).is_err(),
            "should reject index_document={bad:?}"
        );
        let opts = CollectionUploadOptions {
            error_document: Some(bad.to_owned()),
            ..Default::default()
        };
        assert!(
            validate_collection_upload_options(Some(&opts)).is_err(),
            "should reject error_document={bad:?}"
        );
    }
    // Sanity: clean values pass.
    let opts = CollectionUploadOptions {
        index_document: Some("index.html".to_owned()),
        error_document: Some("404.html".to_owned()),
        ..Default::default()
    };
    assert!(validate_collection_upload_options(Some(&opts)).is_ok());
    assert!(validate_collection_upload_options(None).is_ok());
}

// The streaming-cap behavior in `Inner::read_capped` is exercised by
// the unit tests in `client.rs` indirectly (every send_json call goes
// through it). A wiremock-driven Content-Length-mismatch test was
// considered but hyper rejects forged Content-Length / body length
// conflicts at the transport layer before the cap can fire.
