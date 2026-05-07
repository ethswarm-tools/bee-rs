//! v1.4.0 surface: extended `/topology` parse — per-bin `BinInfo`
//! map, `reachability` + `networkAvailability` strings, light-node
//! bin, and per-peer `MetricSnapshotView`.

use bee::Client;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Build a representative `/topology` body with a sparse set of bins
/// populated, the rest defaulting to empty. Mirrors the shape Bee emits
/// for a node that has only completed peer discovery in the lower
/// proximity orders.
fn topology_body() -> serde_json::Value {
    let mut bins = serde_json::Map::with_capacity(32);
    // Empty/zero bins for 0..=31 by default; selectively overwrite.
    for i in 0u8..32 {
        bins.insert(
            format!("bin_{i}"),
            json!({
                "population": 0,
                "connected": 0,
                "disconnectedPeers": null,
                "connectedPeers": null,
            }),
        );
    }
    bins.insert(
        "bin_4".into(),
        json!({
            "population": 5,
            "connected": 3,
            "disconnectedPeers": [
                { "address": "00".repeat(32) }
            ],
            "connectedPeers": [
                {
                    "address": "11".repeat(32),
                    "metrics": {
                        "lastSeenTimestamp": 1_700_000_000,
                        "sessionConnectionRetry": 0,
                        "connectionTotalDuration": 12.5,
                        "sessionConnectionDuration": 4.2,
                        "sessionConnectionDirection": "outbound",
                        "latencyEWMA": 8_400_000,
                        "reachability": "Public",
                        "healthy": true
                    }
                },
                { "address": "22".repeat(32) }
            ]
        }),
    );
    bins.insert(
        "bin_8".into(),
        json!({
            "population": 18,
            "connected": 12,
            "disconnectedPeers": [],
            "connectedPeers": []
        }),
    );

    json!({
        "baseAddr": "ab".repeat(32),
        "population": 23,
        "connected": 15,
        "timestamp": "2024-01-01T00:00:00Z",
        "nnLowWatermark": 4,
        "depth": 8,
        "reachability": "Public",
        "networkAvailability": "Available",
        "bins": serde_json::Value::Object(bins),
        "lightNodes": {
            "population": 2,
            "connected": 1,
            "disconnectedPeers": [],
            "connectedPeers": [
                { "address": "33".repeat(32) }
            ]
        }
    })
}

#[tokio::test]
async fn topology_parses_bins_and_reachability() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/topology"))
        .respond_with(ResponseTemplate::new(200).set_body_json(topology_body()))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let t = client.debug().topology().await.unwrap();

    assert_eq!(t.depth, 8);
    assert_eq!(t.population, 23);
    assert_eq!(t.connected, 15);
    assert_eq!(t.reachability, "Public");
    assert_eq!(t.network_availability, "Available");

    // 32 bins always present; sparse population.
    assert_eq!(t.bins.len(), 32);
    assert_eq!(t.bins[0].population, 0);
    assert_eq!(t.bins[0].connected, 0);
    assert!(t.bins[0].connected_peers.is_empty());

    // Bin 4 carries the rich peer entry with full metrics.
    let b4 = &t.bins[4];
    assert_eq!(b4.population, 5);
    assert_eq!(b4.connected, 3);
    assert_eq!(b4.connected_peers.len(), 2);
    assert_eq!(b4.disconnected_peers.len(), 1);
    let p0 = &b4.connected_peers[0];
    assert_eq!(p0.address, "11".repeat(32));
    let metrics = p0.metrics.as_ref().expect("metrics present");
    assert_eq!(metrics.reachability, "Public");
    assert!(metrics.healthy);
    assert_eq!(metrics.session_connection_direction, "outbound");
    assert_eq!(metrics.latency_ewma, 8_400_000);

    // Peer with no metrics still parses.
    assert!(b4.connected_peers[1].metrics.is_none());

    // Bin 8 has counts but empty peer lists.
    assert_eq!(t.bins[8].population, 18);
    assert_eq!(t.bins[8].connected, 12);
    assert!(t.bins[8].connected_peers.is_empty());

    // Light-node bin sits beside the regular 32.
    assert_eq!(t.light_nodes.population, 2);
    assert_eq!(t.light_nodes.connected, 1);
    assert_eq!(t.light_nodes.connected_peers.len(), 1);
}

#[tokio::test]
async fn topology_handles_missing_extension_fields() {
    // Stripped-down body — no bins, no reachability strings. Older
    // Bee builds and dev mocks should still parse cleanly.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/topology"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "baseAddr": "ab".repeat(32),
            "population": 0,
            "connected": 0,
            "timestamp": "",
            "nnLowWatermark": 0,
            "depth": 0,
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let t = client.debug().topology().await.unwrap();

    assert_eq!(t.depth, 0);
    assert_eq!(t.reachability, "");
    assert_eq!(t.network_availability, "");
    // Default bins => 32 empty entries.
    assert_eq!(t.bins.len(), 32);
    for b in &t.bins {
        assert_eq!(b.population, 0);
        assert_eq!(b.connected, 0);
    }
    // Default light-node bin.
    assert_eq!(t.light_nodes.population, 0);
}

#[tokio::test]
async fn topology_partial_bins_padded_with_defaults() {
    // Body containing only a couple of bin keys — the deserializer
    // pads the rest to default. Defends against stripped-down dev
    // servers and future Bee builds that might not emit every bin.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/topology"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "baseAddr": "cd".repeat(32),
            "population": 5,
            "connected": 3,
            "timestamp": "",
            "nnLowWatermark": 4,
            "depth": 4,
            "reachability": "Private",
            "networkAvailability": "Available",
            "bins": {
                "bin_4": {
                    "population": 5,
                    "connected": 3,
                    "connectedPeers": null,
                    "disconnectedPeers": null
                }
            }
        })))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri()).unwrap();
    let t = client.debug().topology().await.unwrap();

    assert_eq!(t.bins.len(), 32);
    assert_eq!(t.bins[4].population, 5);
    assert_eq!(t.bins[4].connected, 3);
    // Untouched bins default to zero.
    assert_eq!(t.bins[0].population, 0);
    assert_eq!(t.bins[31].population, 0);
}
