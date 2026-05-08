//! Health, versions, and chain-state endpoints.

use num_bigint::BigInt;
use reqwest::Method;
use serde::{Deserialize, Deserializer};

use crate::client::{Inner, MAX_JSON_RESPONSE_BYTES, request};
use crate::swarm::Error;

use super::DebugApi;

/// `GET /health` response. Mirrors bee-js `Health`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    /// Reported status (`"ok"` when ready).
    pub status: String,
    /// Bee build version (e.g. `"2.7.1-61fab37b"`).
    pub version: String,
    /// API version this Bee implements.
    pub api_version: String,
}

/// `GET /node` / `GET /addresses` plus health give the full picture.
/// `Versions` is the structured triple bee-js exposes from `/health`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BeeVersions {
    /// Full Bee version including git suffix.
    pub bee_version: String,
    /// Just the API version field.
    pub bee_api_version: String,
    /// Supported API version this client claims to follow.
    pub supported_api_version: String,
    /// Supported exact Bee version this client targets.
    pub supported_bee_version_exact: String,
}

/// `GET /chainstate` response. Mirrors bee-go's `ChainStateResponse`,
/// including the bigint-as-string custom decode for `currentPrice` /
/// `totalAmount` (one of the three live-Bee bug fixes bee-go hit).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainState {
    /// Latest block number Bee has settled to.
    pub block: u64,
    /// Highest block number Bee has observed (head of chain).
    pub chain_tip: u64,
    /// Per-chunk price in PLUR/block.
    #[serde(default, deserialize_with = "deserialize_bigint_string")]
    pub current_price: BigInt,
    /// Total accumulated price (PLUR).
    #[serde(default, deserialize_with = "deserialize_bigint_string")]
    pub total_amount: BigInt,
}

fn deserialize_bigint_string<'de, D>(d: D) -> Result<BigInt, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(d)?;
    if s.is_empty() {
        return Ok(BigInt::from(0));
    }
    s.parse::<BigInt>().map_err(serde::de::Error::custom)
}

/// Supported API version this client claims compatibility with.
pub const SUPPORTED_API_VERSION: &str = "8.0.0";
/// Supported exact Bee version this client targets.
pub const SUPPORTED_BEE_VERSION_EXACT: &str = "2.7.2-rc1-83612d37";

/// `GET /node` response — operator-mode flags. Mirrors bee-go `NodeInfo`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeInfo {
    /// Bee mode (`"full"`, `"light"`, `"ultra-light"`, `"dev"`).
    pub bee_mode: String,
    /// Whether the chequebook subsystem is enabled.
    pub chequebook_enabled: bool,
    /// Whether SWAP settlement is enabled.
    pub swap_enabled: bool,
}

/// `GET /status` response — operational status snapshot. Mirrors
/// bee-go `StatusResponse`.
#[derive(Clone, Debug, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// Overlay address.
    pub overlay: String,
    /// Proximity order vs. the network root.
    pub proximity: i64,
    /// Bee mode.
    pub bee_mode: String,
    /// Reserve size (chunks).
    pub reserve_size: i64,
    /// Reserve size within radius.
    pub reserve_size_within_radius: i64,
    /// Pull-sync rate (chunks/sec).
    pub pullsync_rate: f64,
    /// Storage radius.
    pub storage_radius: i64,
    /// Currently connected peers.
    pub connected_peers: i64,
    /// Neighbourhood size.
    pub neighborhood_size: i64,
    /// Batch commitment.
    pub batch_commitment: i64,
    /// Reachability flag.
    pub is_reachable: bool,
    /// Last block synced.
    pub last_synced_block: i64,
    /// Committed depth.
    pub committed_depth: i64,
    /// Whether the node is still warming up.
    pub is_warming_up: bool,
}

/// `GET /status/peers` row — like [`Status`] but per-peer, with a
/// `request_failed` flag set when the snapshot couldn't be gathered.
#[derive(Clone, Debug, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerStatus {
    /// Inner per-peer status snapshot (zero-valued if the request failed).
    #[serde(flatten)]
    pub status: Status,
    /// True when Bee couldn't reach this peer in time.
    #[serde(default)]
    pub request_failed: bool,
}

/// `GET /status/neighborhoods` row — per-neighbourhood reserve stats.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Neighborhood {
    /// Binary prefix string (e.g. `"01101"`).
    pub neighborhood: String,
    /// Reserve size within the neighbourhood radius.
    pub reserve_size_within_radius: i64,
    /// Proximity order.
    pub proximity: u8,
}

impl DebugApi {
    /// `GET /health` — basic liveness + version info.
    pub async fn health(&self) -> Result<Health, Error> {
        let builder = request(&self.inner, Method::GET, "health")?;
        self.inner.send_json(builder).await
    }

    /// Structured version triple derived from `/health`.
    pub async fn versions(&self) -> Result<BeeVersions, Error> {
        let h = self.health().await?;
        Ok(BeeVersions {
            bee_version: h.version,
            bee_api_version: h.api_version,
            supported_api_version: SUPPORTED_API_VERSION.to_string(),
            supported_bee_version_exact: SUPPORTED_BEE_VERSION_EXACT.to_string(),
        })
    }

    /// True iff the Bee node's reported API version equals
    /// [`SUPPORTED_API_VERSION`].
    pub async fn is_supported_api_version(&self) -> Result<bool, Error> {
        let h = self.health().await?;
        Ok(h.api_version == SUPPORTED_API_VERSION)
    }

    /// True iff the Bee node's reported version equals
    /// [`SUPPORTED_BEE_VERSION_EXACT`].
    pub async fn is_supported_exact_version(&self) -> Result<bool, Error> {
        let h = self.health().await?;
        Ok(h.version == SUPPORTED_BEE_VERSION_EXACT)
    }

    /// `GET /chainstate` — current price and total amount as bigints.
    pub async fn chain_state(&self) -> Result<ChainState, Error> {
        let builder = request(&self.inner, Method::GET, "chainstate")?;
        self.inner.send_json(builder).await
    }

    /// `GET /node` — operator-mode flags.
    pub async fn node_info(&self) -> Result<NodeInfo, Error> {
        let builder = request(&self.inner, Method::GET, "node")?;
        self.inner.send_json(builder).await
    }

    /// `GET /status` — operational snapshot (reserve size, sync, peers).
    pub async fn status(&self) -> Result<Status, Error> {
        let builder = request(&self.inner, Method::GET, "status")?;
        self.inner.send_json(builder).await
    }

    /// `GET /status/peers` — per-peer status snapshots gathered in
    /// parallel by the Bee node. Peers that don't respond have
    /// `request_failed = true`.
    pub async fn status_peers(&self) -> Result<Vec<PeerStatus>, Error> {
        let builder = request(&self.inner, Method::GET, "status/peers")?;
        #[derive(Deserialize)]
        struct Resp {
            snapshots: Vec<PeerStatus>,
        }
        let r: Resp = self.inner.send_json(builder).await?;
        Ok(r.snapshots)
    }

    /// `GET /status/neighborhoods` — reserve statistics per
    /// neighbourhood.
    pub async fn status_neighborhoods(&self) -> Result<Vec<Neighborhood>, Error> {
        let builder = request(&self.inner, Method::GET, "status/neighborhoods")?;
        #[derive(Deserialize)]
        struct Resp {
            neighborhoods: Vec<Neighborhood>,
        }
        let r: Resp = self.inner.send_json(builder).await?;
        Ok(r.neighborhoods)
    }

    /// `GET /readiness` — true if the node returns 2xx, false on 404,
    /// otherwise the underlying error.
    pub async fn readiness(&self) -> Result<bool, Error> {
        let builder = request(&self.inner, Method::GET, "readiness")?;
        match self.inner.send(builder).await {
            Ok(_) => Ok(true),
            Err(e) if e.status() == Some(404) || e.status() == Some(503) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// `GET /gateway` — true when Bee is running in gateway mode. A
    /// 404 (gateway disabled) becomes `Ok(false)` — matching bee-js.
    pub async fn is_gateway(&self) -> Result<bool, Error> {
        let builder = request(&self.inner, Method::GET, "gateway")?;
        match self.inner.send(builder).await {
            Ok(resp) => {
                #[derive(Deserialize)]
                struct Resp {
                    gateway: bool,
                }
                let r: Resp = serde_json::from_slice(&Inner::read_capped(resp, MAX_JSON_RESPONSE_BYTES).await?)?;
                Ok(r.gateway)
            }
            Err(e) if e.status() == Some(404) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Ping the base URL — true on any 2xx response. Mirrors bee-js
    /// `Bee.isConnected`.
    pub async fn is_connected(&self) -> bool {
        self.check_connection().await.is_ok()
    }

    /// Same as [`DebugApi::is_connected`] but returns the underlying
    /// error if the node is unreachable.
    pub async fn check_connection(&self) -> Result<(), Error> {
        let builder = self
            .inner
            .http
            .request(Method::GET, self.inner.base_url.clone());
        self.inner.send(builder).await?;
        Ok(())
    }
}
