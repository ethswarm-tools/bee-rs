//! Accounting / balances / stake endpoints. Mirrors bee-go's
//! `pkg/debug/{accounting,stake}.go`.

use std::collections::HashMap;

use bytes::Bytes;
use num_bigint::BigInt;
use reqwest::Method;
use serde::{Deserialize, Deserializer};

use crate::client::request;
use crate::swarm::Error;

use super::DebugApi;

/// Settlement balance with one peer. Mirrors bee-go `Balance`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Balance {
    /// Peer overlay address.
    pub peer: String,
    /// Settlement balance (PLUR).
    #[serde(deserialize_with = "deserialize_bigint")]
    pub balance: BigInt,
}

fn deserialize_bigint<'de, D>(d: D) -> Result<BigInt, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(d)?;
    if s.is_empty() {
        return Ok(BigInt::from(0));
    }
    s.parse::<BigInt>().map_err(serde::de::Error::custom)
}

fn deserialize_opt_bigint<'de, D>(d: D) -> Result<Option<BigInt>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(d)?;
    if s.is_empty() {
        return Ok(None);
    }
    s.parse::<BigInt>()
        .map(Some)
        .map_err(serde::de::Error::custom)
}

/// Full per-peer accounting state (richer than [`Balance`]). All
/// monetary fields are PLUR.
#[derive(Clone, Debug, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerAccounting {
    /// Live settlement balance.
    #[serde(default, deserialize_with = "deserialize_opt_bigint")]
    pub balance: Option<BigInt>,
    /// Past-due consumption balance.
    #[serde(default, deserialize_with = "deserialize_opt_bigint")]
    pub consumed_balance: Option<BigInt>,
    /// Configured received-credit threshold.
    #[serde(default, deserialize_with = "deserialize_opt_bigint")]
    pub threshold_received: Option<BigInt>,
    /// Configured given-credit threshold.
    #[serde(default, deserialize_with = "deserialize_opt_bigint")]
    pub threshold_given: Option<BigInt>,
    /// Dynamic received-credit threshold.
    #[serde(default, deserialize_with = "deserialize_opt_bigint")]
    pub current_threshold_received: Option<BigInt>,
    /// Dynamic given-credit threshold.
    #[serde(default, deserialize_with = "deserialize_opt_bigint")]
    pub current_threshold_given: Option<BigInt>,
    /// Surplus balance.
    #[serde(default, deserialize_with = "deserialize_opt_bigint")]
    pub surplus_balance: Option<BigInt>,
    /// Reserved-balance (in-flight credits).
    #[serde(default, deserialize_with = "deserialize_opt_bigint")]
    pub reserved_balance: Option<BigInt>,
    /// Shadow-reserved balance.
    #[serde(default, deserialize_with = "deserialize_opt_bigint")]
    pub shadow_reserved_balance: Option<BigInt>,
    /// Ghost balance (recovered after disconnect).
    #[serde(default, deserialize_with = "deserialize_opt_bigint")]
    pub ghost_balance: Option<BigInt>,
}

/// Redistribution-state snapshot. Mirrors bee-go
/// `RedistributionStateResponse`.
#[derive(Clone, Debug, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedistributionState {
    /// Minimum gas funds to play a round.
    #[serde(default, deserialize_with = "deserialize_opt_bigint")]
    pub minimum_gas_funds: Option<BigInt>,
    /// Whether the node currently has those funds.
    pub has_sufficient_funds: bool,
    /// Whether the node is frozen out of redistribution.
    pub is_frozen: bool,
    /// Whether the node believes it is fully synced.
    pub is_fully_synced: bool,
    /// Current phase string.
    pub phase: String,
    /// Current round.
    pub round: u64,
    /// Last round won.
    pub last_won_round: u64,
    /// Last round played.
    pub last_played_round: u64,
    /// Last round frozen.
    pub last_frozen_round: u64,
    /// Last round selected.
    pub last_selected_round: u64,
    /// Last sample duration in seconds. Bee returns this as a
    /// fractional float (e.g. `37.96302`), not an integer.
    pub last_sample_duration_seconds: f64,
    /// Latest seen block.
    pub block: u64,
    /// Cumulative reward (PLUR).
    #[serde(default, deserialize_with = "deserialize_opt_bigint")]
    pub reward: Option<BigInt>,
    /// Cumulative fees (PLUR).
    #[serde(default, deserialize_with = "deserialize_opt_bigint")]
    pub fees: Option<BigInt>,
    /// Whether the redistribution worker is healthy.
    pub is_healthy: bool,
}

impl DebugApi {
    /// `GET /balances` — settlement balances with every known peer.
    pub async fn balances(&self) -> Result<Vec<Balance>, Error> {
        let builder = request(&self.inner, Method::GET, "balances")?;
        #[derive(Deserialize)]
        struct Resp {
            balances: Vec<Balance>,
        }
        let r: Resp = self.inner.send_json(builder).await?;
        Ok(r.balances)
    }

    /// `GET /balances/{address}` — settlement balance with one peer.
    pub async fn peer_balance(&self, address: &str) -> Result<Balance, Error> {
        let path = format!("balances/{address}");
        let builder = request(&self.inner, Method::GET, &path)?;
        self.inner.send_json(builder).await
    }

    /// `GET /consumed` — past-due consumption balances with every peer.
    pub async fn consumed_balances(&self) -> Result<Vec<Balance>, Error> {
        let builder = request(&self.inner, Method::GET, "consumed")?;
        #[derive(Deserialize)]
        struct Resp {
            balances: Vec<Balance>,
        }
        let r: Resp = self.inner.send_json(builder).await?;
        Ok(r.balances)
    }

    /// `GET /consumed/{address}` — past-due consumption balance with
    /// one peer.
    pub async fn peer_consumed_balance(&self, address: &str) -> Result<Balance, Error> {
        let path = format!("consumed/{address}");
        let builder = request(&self.inner, Method::GET, &path)?;
        self.inner.send_json(builder).await
    }

    /// `GET /accounting` — full per-peer accounting snapshot keyed by
    /// peer overlay address.
    pub async fn accounting(&self) -> Result<HashMap<String, PeerAccounting>, Error> {
        let builder = request(&self.inner, Method::GET, "accounting")?;
        #[derive(Deserialize)]
        struct Resp {
            #[serde(rename = "peerData")]
            peer_data: HashMap<String, PeerAccounting>,
        }
        let r: Resp = self.inner.send_json(builder).await?;
        Ok(r.peer_data)
    }

    // ---- stake -------------------------------------------------------

    /// `GET /stake` — staked BZZ amount (PLUR).
    pub async fn stake(&self) -> Result<BigInt, Error> {
        let builder = request(&self.inner, Method::GET, "stake")?;
        #[derive(Deserialize)]
        struct Resp {
            #[serde(rename = "stakedAmount")]
            staked_amount: String,
        }
        let r: Resp = self.inner.send_json(builder).await?;
        r.staked_amount.parse::<BigInt>().map_err(|e| {
            Error::argument(format!("invalid stakedAmount {:?}: {e}", r.staked_amount))
        })
    }

    /// `POST /stake/{amount}` — stake the given amount (PLUR). Returns
    /// the on-chain transaction hash.
    pub async fn deposit_stake(&self, amount: &BigInt) -> Result<String, Error> {
        let path = format!("stake/{amount}");
        let builder = request(&self.inner, Method::POST, &path)?;
        tx_hash_response(&self.inner, builder).await
    }

    /// `GET /stake/withdrawable` — withdrawable staked BZZ (PLUR).
    pub async fn withdrawable_stake(&self) -> Result<BigInt, Error> {
        let builder = request(&self.inner, Method::GET, "stake/withdrawable")?;
        #[derive(Deserialize)]
        struct Resp {
            #[serde(rename = "withdrawableAmount")]
            withdrawable_amount: String,
        }
        let r: Resp = self.inner.send_json(builder).await?;
        r.withdrawable_amount.parse::<BigInt>().map_err(|e| {
            Error::argument(format!(
                "invalid withdrawableAmount {:?}: {e}",
                r.withdrawable_amount
            ))
        })
    }

    /// `DELETE /stake/withdrawable` — withdraw surplus stake.
    pub async fn withdraw_surplus_stake(&self) -> Result<String, Error> {
        let builder = request(&self.inner, Method::DELETE, "stake/withdrawable")?;
        tx_hash_response(&self.inner, builder).await
    }

    /// `DELETE /stake` — migrate the stake. Returns the transaction
    /// hash.
    pub async fn migrate_stake(&self) -> Result<String, Error> {
        let builder = request(&self.inner, Method::DELETE, "stake")?;
        tx_hash_response(&self.inner, builder).await
    }

    /// `GET /redistributionstate` — redistribution worker snapshot.
    pub async fn redistribution_state(&self) -> Result<RedistributionState, Error> {
        let builder = request(&self.inner, Method::GET, "redistributionstate")?;
        self.inner.send_json(builder).await
    }
}

async fn tx_hash_response(
    inner: &crate::client::Inner,
    builder: reqwest::RequestBuilder,
) -> Result<String, Error> {
    #[derive(Deserialize)]
    struct Resp {
        #[serde(rename = "txHash")]
        tx_hash: String,
    }
    let resp = inner.send(builder).await?;
    let bytes: Bytes = resp.bytes().await?;
    let r: Resp = serde_json::from_slice(&bytes)?;
    Ok(r.tx_hash)
}
