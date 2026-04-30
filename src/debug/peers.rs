//! Peer/connectivity endpoints: peers list, blocklist, ping, connect,
//! topology, reserve state. Mirrors bee-go's
//! `pkg/debug/connectivity.go` plus the relevant pieces of `node.go`.

use reqwest::Method;
use serde::Deserialize;

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

/// Topology snapshot — `GET /topology`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
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
