//! Bee dev-mode wrapper. The dev-mode endpoint surface is a strict
//! subset of production Bee, so this is mostly a documentation /
//! discovery signal: callers can `let bee = DevClient::new(url)?;`
//! and use the same accessors they already know from [`Client`].
//!
//! Mirrors bee-js `BeeDev`. The endpoints currently exposed
//! (`addresses`, `topology`) accept a slimmer payload in dev mode but
//! the same accessors work; bee-rs avoids splitting the type system
//! here and reuses the regular debug handles.
//!
//! # Endpoints that work
//!
//! - Health / readiness / addresses / topology / node-info / status
//!   (dev-shaped, simpler JSON — the existing parsers tolerate the
//!   missing fields).
//! - File upload / download (`/bytes`, `/bzz`, `/chunks`, `/soc`,
//!   `/feeds`).
//! - PSS send / subscribe; GSOC send / subscribe.
//! - Tags, pins, stewardship, grantees, envelopes.
//! - The `/stamps` endpoints behave as no-ops in dev mode but do not
//!   404.
//!
//! # Endpoints that return 404
//!
//! - Chequebook lifecycle: `chequebook_balance`, `deposit_tokens`,
//!   `withdraw_tokens`, `last_cheques`, `get_last_cheques_for_peer`,
//!   `get_last_cashout_action`, `cashout_last_cheque`.
//! - Settlements: `settlements`, `peer_settlement`.
//! - Stake: `get_stake`, `stake`, `deposit_stake`,
//!   `withdraw_surplus_stake`, `migrate_stake`,
//!   `get_withdrawable_stake`.
//! - Pending transactions: list, get, rebroadcast, cancel.
//! - Chain-state reads: `chain_state`, `reserve_state`,
//!   `redistribution_state`, `rc_hash`.
//! - Per-peer accounting and balances.
//! - High-level helpers that internally call any of the above —
//!   [`crate::storage::buy_storage`], `extend_storage_*`,
//!   `get_storage_cost`.

use crate::Client;
use crate::swarm::Error;

/// Thin newtype around [`Client`] for use against Bee in dev mode.
/// Cheap to clone.
#[derive(Clone, Debug)]
pub struct DevClient {
    inner: Client,
}

impl DevClient {
    /// Construct a dev-mode client from a base URL. Same shape as
    /// [`Client::new`].
    pub fn new(url: &str) -> Result<Self, Error> {
        Ok(Self {
            inner: Client::new(url)?,
        })
    }

    /// Construct a dev-mode client with a caller-provided HTTP client.
    pub fn with_http_client(url: &str, http: reqwest::Client) -> Result<Self, Error> {
        Ok(Self {
            inner: Client::with_http_client(url, http)?,
        })
    }

    /// Borrow the wrapped [`Client`] for full API access.
    pub fn client(&self) -> &Client {
        &self.inner
    }

    /// Convert into the wrapped [`Client`].
    pub fn into_client(self) -> Client {
        self.inner
    }
}

impl std::ops::Deref for DevClient {
    type Target = Client;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
