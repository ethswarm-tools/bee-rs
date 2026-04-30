//! SOC upload, reader, writer. Mirrors bee-go's `pkg/file/soc.go`.

use bytes::Bytes;
use reqwest::Method;
use serde::Deserialize;

use crate::api::{UploadOptions, UploadResult, prepare_upload_headers};
use crate::client::{Inner, request};
use crate::swarm::{
    BatchId, Error, EthAddress, Identifier, PrivateKey, Reference, Signature, SingleOwnerChunk,
    calculate_single_owner_chunk_address, make_single_owner_chunk, unmarshal_single_owner_chunk,
};

use super::FileApi;
use super::chunk::download_chunk_response;

#[derive(Deserialize)]
struct UploadBody {
    reference: String,
}

impl FileApi {
    /// Upload a Single Owner Chunk to `POST /soc/{owner}/{id}?sig=…`.
    /// `data` must be the SOC body in wire form: `span (8) || payload`.
    /// Mirrors bee-go `(*Service).UploadSOC`.
    pub async fn upload_soc(
        &self,
        batch_id: &BatchId,
        owner: &EthAddress,
        identifier: &Identifier,
        signature: &Signature,
        data: impl Into<Bytes>,
        opts: Option<&UploadOptions>,
    ) -> Result<UploadResult, Error> {
        let path = format!("soc/{}/{}", owner.to_hex(), identifier.to_hex());
        let builder = request(&self.inner, Method::POST, &path)?
            .header("Content-Type", "application/octet-stream")
            .query(&[("sig", signature.to_hex())])
            .body(data.into());
        let builder = Inner::apply_headers(builder, prepare_upload_headers(batch_id, opts));
        let resp = self.inner.send(builder).await?;
        let headers = resp.headers().clone();
        let body: UploadBody = serde_json::from_slice(&resp.bytes().await?)?;
        UploadResult::from_response(&body.reference, &headers)
    }

    /// Construct a [`SocReader`] for the given owner.
    pub fn make_soc_reader(&self, owner: EthAddress) -> SocReader {
        SocReader {
            owner,
            inner: self.inner.clone(),
        }
    }

    /// Construct a [`SocWriter`] for the given signer. Owner is
    /// derived from `signer.public_key().address()`.
    pub fn make_soc_writer(&self, signer: PrivateKey) -> Result<SocWriter, Error> {
        let owner = signer.public_key()?.address();
        Ok(SocWriter {
            reader: SocReader {
                owner,
                inner: self.inner.clone(),
            },
            signer,
        })
    }
}

/// Reader for SOCs owned by a known [`EthAddress`]. Fetches each
/// chunk by computing its `keccak256(identifier || owner)` address
/// and verifying the recovered signer matches the owner.
#[derive(Clone, Debug)]
pub struct SocReader {
    owner: EthAddress,
    inner: std::sync::Arc<Inner>,
}

impl SocReader {
    /// Address whose SOCs this reader downloads.
    pub fn owner(&self) -> &EthAddress {
        &self.owner
    }

    /// Fetch the SOC at `keccak256(identifier || owner)`, parse the
    /// wire form, and verify it.
    pub async fn download(&self, identifier: &Identifier) -> Result<SingleOwnerChunk, Error> {
        let address = calculate_single_owner_chunk_address(identifier, &self.owner)?;
        let resp = download_chunk_response(&self.inner, &address, None).await?;
        let bytes = resp.bytes().await?;
        unmarshal_single_owner_chunk(&bytes, &address)
    }
}

/// Reader + writer for SOCs signed by a known [`PrivateKey`].
#[derive(Clone, Debug)]
pub struct SocWriter {
    reader: SocReader,
    signer: PrivateKey,
}

impl SocWriter {
    /// Owner address derived from the signer.
    pub fn owner(&self) -> &EthAddress {
        self.reader.owner()
    }

    /// Read side of this writer.
    pub fn reader(&self) -> &SocReader {
        &self.reader
    }

    /// Sign and upload a SOC for `identifier` with `data`. Returns
    /// the SOC reference (`= keccak256(identifier || owner)`).
    pub async fn upload(
        &self,
        batch_id: &BatchId,
        identifier: &Identifier,
        data: &[u8],
        opts: Option<&UploadOptions>,
    ) -> Result<UploadResult, Error> {
        let soc = make_single_owner_chunk(identifier, data, &self.signer)?;
        let mut full = Vec::with_capacity(soc.span.as_bytes().len() + soc.payload.len());
        full.extend_from_slice(soc.span.as_bytes());
        full.extend_from_slice(&soc.payload);
        let api = FileApi {
            inner: self.reader.inner.clone(),
        };
        api.upload_soc(
            batch_id,
            &self.reader.owner,
            identifier,
            &soc.signature,
            full,
            opts,
        )
        .await
    }
}

/// Reference to a SOC chunk: `keccak256(identifier || owner)`.
/// Re-exported convenience wrapper over the swarm-level helper.
pub fn soc_address(identifier: &Identifier, owner: &EthAddress) -> Result<Reference, Error> {
    calculate_single_owner_chunk_address(identifier, owner)
}
