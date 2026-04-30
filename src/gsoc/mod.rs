//! GSOC send / subscribe + offline `soc_address`. Mirrors `pkg/gsoc`
//! in bee-go and bee-js `Bee.gsocSend` / `gsocSubscribe`.
//!
//! GSOC piggy-backs on the SOC primitive: a signer is "mined" (see
//! [`crate::swarm::gsoc_mine`]) so the SOC address
//! `keccak256(identifier || owner)` lands inside the target node's
//! neighbourhood. The target subscribes to
//! `/gsoc/subscribe/{soc_address}` and receives any chunks that hash
//! to that address.

use std::sync::Arc;

use bytes::Bytes;

use crate::api::{UploadOptions, UploadResult};
use crate::client::Inner;
use crate::pss::Subscription;
use crate::swarm::{
    BatchId, Error, EthAddress, Identifier, PrivateKey, Reference, SPAN_LENGTH,
    calculate_single_owner_chunk_address, make_single_owner_chunk,
};

/// Handle exposing the GSOC endpoints. Cheap to clone.
#[derive(Clone, Debug)]
pub struct GsocApi {
    pub(crate) inner: Arc<Inner>,
}

impl GsocApi {
    pub(crate) fn new(inner: Arc<Inner>) -> Self {
        Self { inner }
    }

    /// Build a SOC at `(identifier, signer)` carrying `data` as the
    /// payload, then upload via `POST /soc/{owner}/{id}` (mirrors
    /// bee-js `gsoc.send` / bee-go `gsoc.Service.Send`).
    ///
    /// Use [`crate::swarm::gsoc_mine`] to derive `signer` so the SOC
    /// lands in the target's neighbourhood. Returns the upload
    /// result; `reference` is the SOC address.
    pub async fn send(
        &self,
        batch_id: &BatchId,
        signer: &PrivateKey,
        identifier: &Identifier,
        data: &[u8],
        opts: Option<&UploadOptions>,
    ) -> Result<UploadResult, Error> {
        let soc = make_single_owner_chunk(identifier, data, signer)?;
        // Wire body for `/soc/{owner}/{id}`: `span (8) || payload`.
        let mut body = Vec::with_capacity(SPAN_LENGTH + soc.payload.len());
        body.extend_from_slice(soc.span.as_bytes());
        body.extend_from_slice(&soc.payload);

        let owner = signer.public_key()?.address();
        let file_api = crate::file::FileApi::new(self.inner.clone());
        file_api
            .upload_soc(
                batch_id,
                &owner,
                identifier,
                &soc.signature,
                Bytes::from(body),
                opts,
            )
            .await
    }

    /// Subscribe to `/gsoc/subscribe/{soc_address}` over a websocket.
    /// `soc_address = keccak256(identifier || owner)`. Returns a
    /// [`Subscription`] yielding the raw bytes of each delivered
    /// message (see [`Subscription::recv`]).
    pub async fn subscribe(
        &self,
        owner: &EthAddress,
        identifier: &Identifier,
    ) -> Result<Subscription, Error> {
        let addr = calculate_single_owner_chunk_address(identifier, owner)?;
        let path = format!("gsoc/subscribe/{}", addr.to_hex());
        Subscription::open(&self.inner, &path).await
    }

    /// Address GSOC subscribers listen on:
    /// `keccak256(identifier || owner)`. Pure function exposed at
    /// service level so callers can derive the address without
    /// pulling in `crate::swarm`.
    pub fn soc_address(
        &self,
        identifier: &Identifier,
        owner: &EthAddress,
    ) -> Result<Reference, Error> {
        calculate_single_owner_chunk_address(identifier, owner)
    }
}
