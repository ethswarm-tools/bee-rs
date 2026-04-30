//! Transaction lifecycle endpoints — pending list, get, rebroadcast,
//! cancel. Mirrors bee-go's `pkg/debug/transactions.go`.

use num_bigint::BigInt;
use reqwest::Method;
use serde::{Deserialize, Deserializer};

use crate::client::request;
use crate::swarm::Error;

use super::DebugApi;

/// One pending transaction Bee knows about. Mirrors bee-go
/// `TransactionInfo`. The bigint fields arrive as decimal strings on
/// the wire; missing/empty values become `None`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionInfo {
    /// Transaction hash (hex).
    pub transaction_hash: String,
    /// Recipient address (hex).
    pub to: String,
    /// Sender nonce.
    pub nonce: u64,
    /// Gas price (PLUR).
    #[serde(default, deserialize_with = "deserialize_opt_bigint")]
    pub gas_price: Option<BigInt>,
    /// Gas limit.
    pub gas_limit: u64,
    /// Tip boost.
    #[serde(default)]
    pub gas_tip_boost: i64,
    /// EIP-1559 tip cap (PLUR).
    #[serde(default, deserialize_with = "deserialize_opt_bigint")]
    pub gas_tip_cap: Option<BigInt>,
    /// EIP-1559 fee cap (PLUR).
    #[serde(default, deserialize_with = "deserialize_opt_bigint")]
    pub gas_fee_cap: Option<BigInt>,
    /// Calldata (hex).
    #[serde(default)]
    pub data: String,
    /// Creation time (RFC 3339 string).
    #[serde(default)]
    pub created: String,
    /// Operator-supplied description.
    #[serde(default)]
    pub description: String,
    /// Transaction value (PLUR).
    #[serde(default, deserialize_with = "deserialize_opt_bigint")]
    pub value: Option<BigInt>,
}

fn deserialize_opt_bigint<'de, D>(d: D) -> Result<Option<BigInt>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(d)?;
    if s.is_empty() {
        return Ok(None);
    }
    s.parse::<BigInt>().map(Some).map_err(serde::de::Error::custom)
}

impl DebugApi {
    /// `GET /transactions` — every pending transaction Bee tracks.
    pub async fn pending_transactions(&self) -> Result<Vec<TransactionInfo>, Error> {
        let builder = request(&self.inner, Method::GET, "transactions")?;
        #[derive(Deserialize)]
        struct Resp {
            #[serde(rename = "pendingTransactions")]
            pending_transactions: Vec<TransactionInfo>,
        }
        let r: Resp = self.inner.send_json(builder).await?;
        Ok(r.pending_transactions)
    }

    /// `GET /transactions/{hash}` — info for one pending transaction.
    pub async fn pending_transaction(&self, tx_hash: &str) -> Result<TransactionInfo, Error> {
        let path = format!("transactions/{tx_hash}");
        let builder = request(&self.inner, Method::GET, &path)?;
        self.inner.send_json(builder).await
    }

    /// `POST /transactions/{hash}` — replay a pending transaction to
    /// the network. Returns the resulting transaction hash.
    pub async fn rebroadcast_transaction(&self, tx_hash: &str) -> Result<String, Error> {
        let path = format!("transactions/{tx_hash}");
        let builder = request(&self.inner, Method::POST, &path)?;
        #[derive(Deserialize)]
        struct Resp {
            #[serde(rename = "transactionHash")]
            transaction_hash: String,
        }
        let r: Resp = self.inner.send_json(builder).await?;
        Ok(r.transaction_hash)
    }

    /// `DELETE /transactions/{hash}` — cancel a pending transaction by
    /// replacing it at the same nonce. `gas_price` is optional (`None`
    /// lets Bee pick); when `Some(_)` it's sent in the `gas-price`
    /// header so the replacement bumps the fee. Returns the resulting
    /// transaction hash.
    pub async fn cancel_transaction(
        &self,
        tx_hash: &str,
        gas_price: Option<&BigInt>,
    ) -> Result<String, Error> {
        let path = format!("transactions/{tx_hash}");
        let mut builder = request(&self.inner, Method::DELETE, &path)?;
        if let Some(gp) = gas_price {
            builder = builder.header("gas-price", gp.to_string());
        }
        #[derive(Deserialize)]
        struct Resp {
            #[serde(rename = "transactionHash")]
            transaction_hash: String,
        }
        let r: Resp = self.inner.send_json(builder).await?;
        Ok(r.transaction_hash)
    }
}
