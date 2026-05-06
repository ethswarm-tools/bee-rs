//! Chequebook / settlements / wallet endpoints. Mirrors bee-go's
//! `pkg/debug/chequebook.go`.

use bytes::Bytes;
use num_bigint::BigInt;
use reqwest::Method;
use serde::{Deserialize, Deserializer};

use crate::client::{Inner, request};
use crate::swarm::Error;

use super::DebugApi;

fn de_bigint<'de, D: Deserializer<'de>>(d: D) -> Result<BigInt, D::Error> {
    let s: String = Deserialize::deserialize(d)?;
    if s.is_empty() {
        return Ok(BigInt::from(0));
    }
    s.parse::<BigInt>().map_err(serde::de::Error::custom)
}

fn de_opt_bigint<'de, D: Deserializer<'de>>(d: D) -> Result<Option<BigInt>, D::Error> {
    let s: Option<String> = Deserialize::deserialize(d)?;
    match s {
        None => Ok(None),
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => s
            .parse::<BigInt>()
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

/// `GET /wallet` response. Mirrors bee-go `WalletResponse`.
///
/// **API-version note:** Bee 2.7.2 / API 8.0.0 dropped the legacy
/// `bzzAddress` / `nativeAddress` fields and renamed `chequebook` to
/// `chequebookContractAddress`. We accept both spellings: the legacy
/// fields are optional, and `chequebook_contract_address` carries an
/// alias for the old `chequebook` key so older Bee builds still parse.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Wallet {
    /// Legacy BZZ address field (Bee ≤ 2.7.1). `None` on 2.7.2+.
    #[serde(default)]
    pub bzz_address: Option<String>,
    /// Legacy native-token address field (Bee ≤ 2.7.1). `None` on 2.7.2+.
    #[serde(default)]
    pub native_address: Option<String>,
    /// Chequebook contract address. Accepts the new
    /// `chequebookContractAddress` key (Bee 2.7.2+) and falls back to
    /// the legacy `chequebook` key.
    #[serde(default, alias = "chequebook")]
    pub chequebook_contract_address: Option<String>,
    /// BZZ balance in PLUR.
    #[serde(default, deserialize_with = "de_opt_bigint")]
    pub bzz_balance: Option<BigInt>,
    /// Native token balance in wei.
    #[serde(default, deserialize_with = "de_opt_bigint")]
    pub native_token_balance: Option<BigInt>,
    /// On-chain chain ID.
    #[serde(rename = "chainID")]
    pub chain_id: i64,
    /// Wallet (operator) address.
    pub wallet_address: String,
}

/// `GET /chequebook/balance` response.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChequebookBalance {
    /// Total balance held in the chequebook (PLUR).
    #[serde(deserialize_with = "de_bigint")]
    pub total_balance: BigInt,
    /// Available (uncashed) balance (PLUR).
    #[serde(deserialize_with = "de_bigint")]
    pub available_balance: BigInt,
}

/// One peer settlement entry.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Settlement {
    /// Peer overlay address.
    pub peer: String,
    /// Cumulative received PLUR.
    #[serde(default, deserialize_with = "de_opt_bigint")]
    pub received: Option<BigInt>,
    /// Cumulative sent PLUR.
    #[serde(default, deserialize_with = "de_opt_bigint")]
    pub sent: Option<BigInt>,
}

/// `GET /settlements` response.
#[derive(Clone, Debug, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settlements {
    /// Sum of `received` across peers.
    #[serde(default, deserialize_with = "de_opt_bigint")]
    pub total_received: Option<BigInt>,
    /// Sum of `sent` across peers.
    #[serde(default, deserialize_with = "de_opt_bigint")]
    pub total_sent: Option<BigInt>,
    /// Per-peer breakdown.
    #[serde(default)]
    pub settlements: Vec<Settlement>,
}

/// One sent or received cheque.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Cheque {
    /// Cheque beneficiary (eth address).
    pub beneficiary: String,
    /// Issuing chequebook contract.
    pub chequebook: String,
    /// Cumulative payout (PLUR).
    #[serde(default, deserialize_with = "de_opt_bigint")]
    pub payout: Option<BigInt>,
}

/// One row of `GET /chequebook/cheque`. `last_received` may be `None`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct LastCheque {
    /// Peer overlay.
    pub peer: String,
    /// Last received cheque or `None` if no cheques yet.
    #[serde(default, rename = "lastreceived")]
    pub last_received: Option<Cheque>,
}

/// Sent + received cheques for one peer (`GET /chequebook/cheque/{peer}`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct PeerCheques {
    /// Peer overlay.
    pub peer: String,
    /// Last received cheque (if any).
    #[serde(default, rename = "lastreceived")]
    pub last_received: Option<Cheque>,
    /// Last sent cheque (if any).
    #[serde(default, rename = "lastsent")]
    pub last_sent: Option<Cheque>,
}

/// On-chain outcome of a previous cashout.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct CashoutResult {
    /// Recipient eth address.
    pub recipient: String,
    /// Last on-chain payout (PLUR).
    #[serde(default, rename = "lastPayout", deserialize_with = "de_opt_bigint")]
    pub last_payout: Option<BigInt>,
    /// Whether the cashout transaction bounced.
    #[serde(default)]
    pub bounced: bool,
}

/// `GET /chequebook/cashout/{peer}` snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct LastCashoutAction {
    /// Peer overlay.
    pub peer: String,
    /// Uncashed amount (PLUR).
    #[serde(default, rename = "uncashedAmount", deserialize_with = "de_opt_bigint")]
    pub uncashed_amount: Option<BigInt>,
    /// Last cashout transaction hash, if any.
    #[serde(default, rename = "transactionHash")]
    pub transaction_hash: Option<String>,
    /// Last cashed cheque, if any.
    #[serde(default, rename = "lastCashedCheque")]
    pub last_cashed_cheque: Option<Cheque>,
    /// On-chain result, if any.
    #[serde(default)]
    pub result: Option<CashoutResult>,
}

impl DebugApi {
    // ---- wallet -------------------------------------------------------

    /// `GET /wallet` — node operator wallet snapshot.
    pub async fn wallet(&self) -> Result<Wallet, Error> {
        let builder = request(&self.inner, Method::GET, "wallet")?;
        self.inner.send_json(builder).await
    }

    /// `POST /wallet/withdraw/bzz?amount=&address=` — withdraw BZZ to
    /// an external address. Returns the on-chain transaction hash.
    pub async fn withdraw_bzz(&self, amount: &BigInt, address: &str) -> Result<String, Error> {
        let builder = request(&self.inner, Method::POST, "wallet/withdraw/bzz")?.query(&[
            ("amount", amount.to_string()),
            ("address", address.to_string()),
        ]);
        tx_hash(&self.inner, builder).await
    }

    /// `POST /wallet/withdraw/nativetoken?amount=&address=` — withdraw
    /// the native settlement token (xDAI / ETH).
    pub async fn withdraw_native_token(
        &self,
        amount: &BigInt,
        address: &str,
    ) -> Result<String, Error> {
        let builder = request(&self.inner, Method::POST, "wallet/withdraw/nativetoken")?.query(&[
            ("amount", amount.to_string()),
            ("address", address.to_string()),
        ]);
        tx_hash(&self.inner, builder).await
    }

    // ---- chequebook ---------------------------------------------------

    /// `GET /chequebook/balance` — total + available chequebook PLUR.
    pub async fn chequebook_balance(&self) -> Result<ChequebookBalance, Error> {
        let builder = request(&self.inner, Method::GET, "chequebook/balance")?;
        self.inner.send_json(builder).await
    }

    /// `POST /chequebook/deposit?amount=` — deposit BZZ into the
    /// chequebook from the operator wallet.
    pub async fn chequebook_deposit(&self, amount: &BigInt) -> Result<String, Error> {
        let builder = request(&self.inner, Method::POST, "chequebook/deposit")?
            .query(&[("amount", amount.to_string())]);
        tx_hash(&self.inner, builder).await
    }

    /// `POST /chequebook/withdraw?amount=` — withdraw BZZ from the
    /// chequebook back to the operator wallet.
    pub async fn chequebook_withdraw(&self, amount: &BigInt) -> Result<String, Error> {
        let builder = request(&self.inner, Method::POST, "chequebook/withdraw")?
            .query(&[("amount", amount.to_string())]);
        tx_hash(&self.inner, builder).await
    }

    /// `GET /chequebook/cheque` — the last received cheque per peer.
    pub async fn last_cheques(&self) -> Result<Vec<LastCheque>, Error> {
        let builder = request(&self.inner, Method::GET, "chequebook/cheque")?;
        #[derive(Deserialize)]
        struct Resp {
            #[serde(rename = "lastcheques")]
            last_cheques: Vec<LastCheque>,
        }
        let r: Resp = self.inner.send_json(builder).await?;
        Ok(r.last_cheques)
    }

    /// `GET /chequebook/cheque/{peer}` — last sent + received cheque
    /// for one peer.
    pub async fn peer_cheques(&self, peer: &str) -> Result<PeerCheques, Error> {
        let path = format!("chequebook/cheque/{peer}");
        let builder = request(&self.inner, Method::GET, &path)?;
        self.inner.send_json(builder).await
    }

    /// `GET /chequebook/cashout/{peer}` — last cashout action snapshot
    /// for one peer.
    pub async fn last_cashout_action(&self, peer: &str) -> Result<LastCashoutAction, Error> {
        let path = format!("chequebook/cashout/{peer}");
        let builder = request(&self.inner, Method::GET, &path)?;
        self.inner.send_json(builder).await
    }

    /// `POST /chequebook/cashout/{peer}` — cash out the last received
    /// cheque from a peer. `gas_price` is optional (sent in the
    /// `gas-price` header when present). Returns the cashout
    /// transaction hash.
    pub async fn cashout_last_cheque(
        &self,
        peer: &str,
        gas_price: Option<&BigInt>,
    ) -> Result<String, Error> {
        let path = format!("chequebook/cashout/{peer}");
        let mut builder = request(&self.inner, Method::POST, &path)?;
        if let Some(gp) = gas_price {
            builder = builder.header("gas-price", gp.to_string());
        }
        tx_hash(&self.inner, builder).await
    }

    // ---- settlements --------------------------------------------------

    /// `GET /settlements` — totals + per-peer settlement breakdown.
    pub async fn settlements(&self) -> Result<Settlements, Error> {
        let builder = request(&self.inner, Method::GET, "settlements")?;
        self.inner.send_json(builder).await
    }

    /// `GET /settlements/{peer}` — settlement totals for one peer.
    pub async fn peer_settlement(&self, peer: &str) -> Result<Settlement, Error> {
        let path = format!("settlements/{peer}");
        let builder = request(&self.inner, Method::GET, &path)?;
        self.inner.send_json(builder).await
    }

    /// `GET /timesettlements` — totals + per-peer breakdown for the
    /// time-based (pseudo-settle / refresh) settlement path. Same
    /// schema as [`Self::settlements`] but counts only refresh-rate
    /// settlements rather than cheques.
    pub async fn time_settlements(&self) -> Result<Settlements, Error> {
        let builder = request(&self.inner, Method::GET, "timesettlements")?;
        self.inner.send_json(builder).await
    }
}

async fn tx_hash(inner: &Inner, builder: reqwest::RequestBuilder) -> Result<String, Error> {
    #[derive(Deserialize)]
    struct Resp {
        #[serde(rename = "transactionHash")]
        transaction_hash: String,
    }
    let resp = inner.send(builder).await?;
    let bytes: Bytes = resp.bytes().await?;
    let r: Resp = serde_json::from_slice(&bytes)?;
    Ok(r.transaction_hash)
}
