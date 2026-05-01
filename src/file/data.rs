//! `/bytes` endpoints: upload / download / probe.

use bytes::Bytes;
use reqwest::Method;
use serde::Deserialize;

use crate::api::{
    DownloadOptions, RedundantUploadOptions, UploadResult, prepare_download_headers,
    prepare_redundant_upload_headers,
};
use crate::client::{Inner, request};
use crate::swarm::{BatchId, Error, Reference};

use super::FileApi;

/// Result of [`FileApi::probe_data`]: the size of the data behind a
/// `/bytes` reference, learned via `HEAD` without downloading the body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceInformation {
    /// `Content-Length` of the referenced data, in bytes.
    pub content_length: u64,
}

#[derive(Deserialize)]
struct UploadBody {
    reference: String,
}

impl FileApi {
    /// Upload raw bytes via `POST /bytes`. The body is sent as
    /// `application/octet-stream`. Returns an [`UploadResult`] with the
    /// content reference, optional tag UID, and (when ACT was
    /// requested) the history address.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use bee::Client;
    /// use bee::swarm::BatchId;
    /// use bytes::Bytes;
    ///
    /// # async fn run(batch_id: BatchId) -> Result<(), bee::Error> {
    /// let client = Client::new("http://localhost:1633")?;
    /// let result = client
    ///     .file()
    ///     .upload_data(&batch_id, Bytes::from_static(b"Hello Swarm!"), None)
    ///     .await?;
    /// println!("reference: {}", result.reference.to_hex());
    /// # Ok(()) }
    /// ```
    pub async fn upload_data(
        &self,
        batch_id: &BatchId,
        data: impl Into<Bytes>,
        opts: Option<&RedundantUploadOptions>,
    ) -> Result<UploadResult, Error> {
        let builder = request(&self.inner, Method::POST, "bytes")?
            .header("Content-Type", "application/octet-stream")
            .body(data.into());
        let builder =
            Inner::apply_headers(builder, prepare_redundant_upload_headers(batch_id, opts));
        let resp = self.inner.send(builder).await?;
        let headers = resp.headers().clone();
        let body: UploadBody = serde_json::from_slice(&resp.bytes().await?)?;
        UploadResult::from_response(&body.reference, &headers)
    }

    /// Download raw bytes via `GET /bytes/{ref}`. Returns the full body
    /// in memory. For streaming downloads use
    /// [`FileApi::download_data_response`](Self::download_data_response).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use bee::Client;
    /// use bee::swarm::Reference;
    ///
    /// # async fn run(reference: Reference) -> Result<(), bee::Error> {
    /// let client = Client::new("http://localhost:1633")?;
    /// let body = client.file().download_data(&reference, None).await?;
    /// println!("downloaded {} bytes", body.len());
    /// # Ok(()) }
    /// ```
    pub async fn download_data(
        &self,
        reference: &Reference,
        opts: Option<&DownloadOptions>,
    ) -> Result<Bytes, Error> {
        let resp = self.download_data_response(reference, opts).await?;
        Ok(resp.bytes().await?)
    }

    /// Download raw bytes via `GET /bytes/{ref}` and return the raw
    /// [`reqwest::Response`] for streaming. The caller drives reading
    /// from `resp.bytes_stream()` or `resp.chunk()`.
    pub async fn download_data_response(
        &self,
        reference: &Reference,
        opts: Option<&DownloadOptions>,
    ) -> Result<reqwest::Response, Error> {
        let path = format!("bytes/{}", reference.to_hex());
        let builder = request(&self.inner, Method::GET, &path)?;
        let builder = Inner::apply_headers(builder, prepare_download_headers(opts));
        self.inner.send(builder).await
    }

    /// Probe the size of the data behind a `/bytes` reference using
    /// a `HEAD` request. Mirrors bee-js `Bee.probeData`.
    pub async fn probe_data(&self, reference: &Reference) -> Result<ReferenceInformation, Error> {
        let path = format!("bytes/{}", reference.to_hex());
        let builder = request(&self.inner, Method::HEAD, &path)?;
        let resp = self.inner.send(builder).await?;
        let content_length = resp
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .or_else(|| resp.content_length())
            .ok_or_else(|| Error::argument("missing Content-Length"))?;
        Ok(ReferenceInformation { content_length })
    }
}
