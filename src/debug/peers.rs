//! Peer/connectivity endpoints: peers list, blocklist, ping, connect,
//! topology, reserve state. Mirrors bee-go's
//! `pkg/debug/connectivity.go` plus the relevant pieces of `node.go`.

use std::collections::BTreeMap;

use reqwest::Method;
use serde::{Deserialize, Deserializer};

use crate::client::request;
use crate::swarm::Error;

use super::DebugApi;

/// One connected peer entry. Mirrors bee-go `Peer`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Peer {
    /// Peer overlay address (hex).
    pub address: String,
    /// True for full nodes, false for light nodes.
    #[serde(default, rename = "fullNode")]
    pub full_node: bool,
}

/// Node addresses payload — `GET /addresses`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Addresses {
    /// Overlay (DHT) address.
    pub overlay: String,
    /// Underlay multiaddrs.
    pub underlay: Vec<String>,
    /// Ethereum address.
    pub ethereum: String,
    /// Node libp2p public key (hex).
    pub public_key: String,
    /// PSS public key (hex).
    pub pss_public_key: String,
}

/// Per-peer connection metrics. Mirrors bee-go `MetricSnapshotView`.
#[derive(Clone, Debug, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricSnapshotView {
    /// Unix timestamp (seconds) of the most recent observation.
    #[serde(default)]
    pub last_seen_timestamp: i64,
    /// Connection retries within the current session.
    #[serde(default)]
    pub session_connection_retry: u64,
    /// Total time the peer has been connected over its lifetime, in
    /// fractional seconds.
    #[serde(default)]
    pub connection_total_duration: f64,
    /// Time the peer has been connected in the current session.
    #[serde(default)]
    pub session_connection_duration: f64,
    /// `"inbound"` / `"outbound"` for the current session.
    #[serde(default)]
    pub session_connection_direction: String,
    /// Exponentially-weighted moving average latency (nanoseconds, the
    /// raw value Bee writes — divide by 1e6 for ms).
    #[serde(default, rename = "latencyEWMA")]
    pub latency_ewma: i64,
    /// Per-peer reachability string (`"Public"`, `"Private"`,
    /// `"Unknown"`, etc.).
    #[serde(default)]
    pub reachability: String,
    /// Whether the peer is currently considered healthy by the
    /// node-health subsystem.
    #[serde(default)]
    pub healthy: bool,
}

/// One peer entry inside a [`BinInfo`]. Mirrors bee-go `PeerInfo`.
#[derive(Clone, Debug, PartialEq, Default, Deserialize)]
pub struct PeerInfo {
    /// Peer overlay (hex).
    pub address: String,
    /// Per-peer connection metrics. Bee omits the field for some
    /// disconnected peers.
    #[serde(default)]
    pub metrics: Option<MetricSnapshotView>,
}

/// Per-bin population summary. Mirrors bee-go `BinInfo`.
///
/// Bee marshals empty `connectedPeers` / `disconnectedPeers` slices as
/// JSON `null` (Go default for nil slices). We accept `null` and `[]`
/// interchangeably so the parse is robust across Bee builds.
#[derive(Clone, Debug, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinInfo {
    /// Total known peers in this bin (connected + disconnected).
    #[serde(default, rename = "population")]
    pub population: u64,
    /// Currently connected peers in this bin.
    #[serde(default, rename = "connected")]
    pub connected: u64,
    /// Connected peers, with metrics.
    #[serde(default, deserialize_with = "deserialize_null_or_seq")]
    pub connected_peers: Vec<PeerInfo>,
    /// Known-but-disconnected peers.
    #[serde(default, deserialize_with = "deserialize_null_or_seq")]
    pub disconnected_peers: Vec<PeerInfo>,
}

/// Treat a JSON `null` value as an empty `Vec<T>`. Matches Go's
/// `json.Marshal` behaviour for nil slices.
fn deserialize_null_or_seq<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
}

/// Topology snapshot — `GET /topology`.
///
/// Bee returns the per-bin breakdown as 32 flat keys
/// (`bin_0`..`bin_31`); we collapse them into [`Topology::bins`] for
/// indexable access. The light-node bin sits beside the regular bins
/// at [`Topology::light_nodes`].
#[derive(Clone, Debug, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Topology {
    /// Base address (overlay).
    pub base_addr: String,
    /// Population (peers known across all bins).
    pub population: i64,
    /// Currently connected peers.
    pub connected: i64,
    /// Snapshot timestamp (RFC 3339).
    pub timestamp: String,
    /// Lower watermark for the nearest neighbour bin.
    pub nn_low_watermark: i64,
    /// Kademlia depth.
    pub depth: u8,
    /// Aggregate peer reachability state (`"Public"`, `"Private"`,
    /// `"Unknown"`, etc.). Empty on Bee versions that pre-date the
    /// AutoNAT field.
    #[serde(default)]
    pub reachability: String,
    /// Network availability (`"Available"` / `"Unavailable"` /
    /// empty on older Bee versions).
    #[serde(default)]
    pub network_availability: String,
    /// 32 per-bin entries indexed by bin number `0..=31`. See
    /// `BinInfo` for the per-bin shape.
    #[serde(default = "default_bins", deserialize_with = "deserialize_bins")]
    pub bins: Vec<BinInfo>,
    /// Aggregated info for connected light nodes. Sits outside the
    /// regular bins because light nodes don't get a Kademlia bin.
    #[serde(default)]
    pub light_nodes: BinInfo,
}

/// Default for the `Topology::bins` field. Always 32 empty entries —
/// kept invariant so consumers can index `bins[i]` without bounds
/// checks. Used by serde when the response omits the `bins` key
/// entirely (older Bee builds, dev/mock servers).
fn default_bins() -> Vec<BinInfo> {
    vec![BinInfo::default(); 32]
}

/// Decode the flat `{ "bin_0": …, …, "bin_31": … }` object into a
/// 32-element [`Vec<BinInfo>`]. Missing keys default to an empty
/// [`BinInfo`] so partial dev/mock servers don't blow up the parse.
fn deserialize_bins<'de, D>(d: D) -> Result<Vec<BinInfo>, D::Error>
where
    D: Deserializer<'de>,
{
    // Allow the field value to be `null` for forward compatibility.
    let map: Option<BTreeMap<String, BinInfo>> = Option::deserialize(d)?;
    let mut map = map.unwrap_or_default();
    let mut out = Vec::with_capacity(32);
    for i in 0..32u8 {
        let key = format!("bin_{i}");
        out.push(map.remove(&key).unwrap_or_default());
    }
    Ok(out)
}

/// Reserve state snapshot — `GET /reservestate`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReserveState {
    /// Network radius.
    pub radius: u8,
    /// Storage radius.
    pub storage_radius: u8,
    /// Batch commitment.
    pub commitment: i64,
}

impl DebugApi {
    /// `GET /peers` — list every peer this node is currently connected to.
    pub async fn peers(&self) -> Result<Vec<Peer>, Error> {
        let builder = request(&self.inner, Method::GET, "peers")?;
        #[derive(Deserialize)]
        struct Resp {
            peers: Vec<Peer>,
        }
        let r: Resp = self.inner.send_json(builder).await?;
        Ok(r.peers)
    }

    /// `GET /blocklist` — peers currently blocklisted by this node.
    pub async fn blocklist(&self) -> Result<Vec<Peer>, Error> {
        let builder = request(&self.inner, Method::GET, "blocklist")?;
        #[derive(Deserialize)]
        struct Resp {
            peers: Vec<Peer>,
        }
        let r: Resp = self.inner.send_json(builder).await?;
        Ok(r.peers)
    }

    /// `DELETE /peers/{address}` — disconnect and forget a peer.
    pub async fn remove_peer(&self, address: &str) -> Result<(), Error> {
        let path = format!("peers/{address}");
        let builder = request(&self.inner, Method::DELETE, &path)?;
        self.inner.send(builder).await?;
        Ok(())
    }

    /// `POST /pingpong/{address}` — round-trip ping a peer. Returns the
    /// reported RTT string (e.g. `"2.5ms"`).
    pub async fn ping_peer(&self, address: &str) -> Result<String, Error> {
        let path = format!("pingpong/{address}");
        let builder = request(&self.inner, Method::POST, &path)?;
        #[derive(Deserialize)]
        struct Resp {
            rtt: String,
        }
        let r: Resp = self.inner.send_json(builder).await?;
        Ok(r.rtt)
    }

    /// `POST /connect/{multiaddr}` — manually dial a peer at the given
    /// multiaddress (e.g. `"/dns/bee.example.com/tcp/1634/p2p/16Uiu…"`).
    /// Returns the resulting overlay address. A leading `/` in
    /// `multiaddr` is stripped.
    pub async fn connect_peer(&self, multiaddr: &str) -> Result<String, Error> {
        let trimmed = multiaddr.trim_start_matches('/');
        let path = format!("connect/{trimmed}");
        let builder = request(&self.inner, Method::POST, &path)?;
        #[derive(Deserialize)]
        struct Resp {
            address: String,
        }
        let r: Resp = self.inner.send_json(builder).await?;
        Ok(r.address)
    }

    /// `GET /addresses` — node overlay / underlay / ethereum / pubkeys.
    pub async fn addresses(&self) -> Result<Addresses, Error> {
        let builder = request(&self.inner, Method::GET, "addresses")?;
        self.inner.send_json(builder).await
    }

    /// `GET /topology` — Kademlia topology snapshot.
    pub async fn topology(&self) -> Result<Topology, Error> {
        let builder = request(&self.inner, Method::GET, "topology")?;
        self.inner.send_json(builder).await
    }

    /// `GET /reservestate` — current reserve radius/commitment.
    pub async fn reserve_state(&self) -> Result<ReserveState, Error> {
        let builder = request(&self.inner, Method::GET, "reservestate")?;
        self.inner.send_json(builder).await
    }

    /// `GET /welcome-message` — P2P welcome banner.
    pub async fn welcome_message(&self) -> Result<String, Error> {
        let builder = request(&self.inner, Method::GET, "welcome-message")?;
        #[derive(Deserialize)]
        struct Resp {
            #[serde(rename = "welcomeMessage")]
            welcome_message: String,
        }
        let r: Resp = self.inner.send_json(builder).await?;
        Ok(r.welcome_message)
    }

    /// `POST /welcome-message` — update the P2P welcome banner.
    pub async fn set_welcome_message(&self, message: &str) -> Result<(), Error> {
        #[derive(serde::Serialize)]
        struct Body<'a> {
            #[serde(rename = "welcomeMessage")]
            welcome_message: &'a str,
        }
        let body = serde_json::to_vec(&Body {
            welcome_message: message,
        })?;
        let builder = request(&self.inner, Method::POST, "welcome-message")?
            .header("Content-Type", "application/json")
            .body(bytes::Bytes::from(body));
        self.inner.send(builder).await?;
        Ok(())
    }
}
