//! Debug / operator endpoints. Mirrors `pkg/debug` in bee-go.

mod accounting;
mod chequebook;
mod loggers;
mod node;
mod peers;
mod transactions;

pub use accounting::{
    Balance, ChunkInclusionProof, ChunkInclusionProofs, PeerAccounting, PostageProof,
    RCHashResponse, RedistributionState, SocProof,
};
pub use chequebook::{
    CashoutResult, Cheque, ChequebookBalance, LastCashoutAction, LastCheque, PeerCheques,
    Settlement, Settlements, Wallet,
};
pub use loggers::{Logger, LoggerListing};
pub use node::*;
pub use peers::{Addresses, Peer, ReserveState, Topology};
pub use transactions::TransactionInfo;

use std::sync::Arc;

use crate::client::Inner;

/// Handle exposing the debug endpoints. Cheap to clone.
#[derive(Clone, Debug)]
pub struct DebugApi {
    pub(crate) inner: Arc<Inner>,
}

impl DebugApi {
    pub(crate) fn new(inner: Arc<Inner>) -> Self {
        Self { inner }
    }
}
