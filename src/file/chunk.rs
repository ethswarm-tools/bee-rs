//! `/chunks` endpoints: upload / download a single content-addressed
//! chunk.

use bytes::Bytes;
use reqwest::Method;
use serde::Deserialize;

use crate::api::{
    DownloadOptions, UploadOptions, UploadResult, prepare_download_headers, prepare_upload_headers,
};
use crate::client::{Inner, MAX_JSON_RESPONSE_BYTES, request};
use crate::swarm::{BatchId, Error, Reference};

use super::FileApi;

#[derive(Deserialize)]
struct UploadBody {
    reference: String,
}

impl FileApi {
    /// Upload a single raw chunk (`span || payload`) via
    /// `POST /chunks`.
    pub async fn upload_chunk(
        &self,
        batch_id: &BatchId,
        data: impl Into<Bytes>,
        opts: Option<&UploadOptions>,
    ) -> Result<UploadResult, Error> {
        let builder = request(&self.inner, Method::POST, "chunks")?
            .header("Content-Type", "application/octet-stream")
            .body(data.into());
        let builder = Inner::apply_headers(builder, prepare_upload_headers(batch_id, opts));
        let resp = self.inner.send(builder).await?;
        let headers = resp.headers().clone();
        let body: UploadBody = serde_json::from_slice(&Inner::read_capped(resp, MAX_JSON_RESPONSE_BYTES).await?)?;
        UploadResult::from_response(&body.reference, &headers)
    }

    /// Download a single chunk's bytes via `GET /chunks/{ref}`.
    pub async fn download_chunk(
        &self,
        reference: &Reference,
        opts: Option<&DownloadOptions>,
    ) -> Result<Bytes, Error> {
        let resp = download_chunk_response(&self.inner, reference, opts).await?;
        Ok(resp.bytes().await?)
    }
}

/// Internal helper: return the raw `Response` so other modules
/// (feeds, GSOC subscribe verification) can inspect headers without a
/// double round-trip.
pub(crate) async fn download_chunk_response(
    inner: &Inner,
    reference: &Reference,
    opts: Option<&DownloadOptions>,
) -> Result<reqwest::Response, Error> {
    let path = format!("chunks/{}", reference.to_hex());
    let builder = request(inner, Method::GET, &path)?;
    let builder = Inner::apply_headers(builder, prepare_download_headers(opts));
    inner.send(builder).await
}
