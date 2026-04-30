//! Network endpoint methods on [`ApiService`]: pin, tag, stewardship.
//! Mirrors bee-go's `pkg/api/{pin,tag,stewardship}.go`.

use std::sync::Arc;

use bytes::Bytes;
use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::client::{Inner, request};
use crate::swarm::{BatchId, Error, Reference};

/// Handle exposing the generic `api/*` endpoints (pin, tag,
/// stewardship). Cheap to clone.
#[derive(Clone, Debug)]
pub struct ApiService {
    pub(crate) inner: Arc<Inner>,
}

impl ApiService {
    pub(crate) fn new(inner: Arc<Inner>) -> Self {
        Self { inner }
    }

    // ---- pins ---------------------------------------------------------

    /// Pin a reference — `POST /pins/{ref}`.
    pub async fn pin(&self, reference: &Reference) -> Result<(), Error> {
        let path = format!("pins/{}", reference.to_hex());
        let builder = request(&self.inner, Method::POST, &path)?;
        self.inner.send(builder).await?;
        Ok(())
    }

    /// Unpin a reference — `DELETE /pins/{ref}`.
    pub async fn unpin(&self, reference: &Reference) -> Result<(), Error> {
        let path = format!("pins/{}", reference.to_hex());
        let builder = request(&self.inner, Method::DELETE, &path)?;
        self.inner.send(builder).await?;
        Ok(())
    }

    /// Check whether a reference is pinned — `GET /pins/{ref}`.
    /// Returns `true` on `200`, `false` on `404`, otherwise the
    /// underlying response error.
    pub async fn get_pin(&self, reference: &Reference) -> Result<bool, Error> {
        let path = format!("pins/{}", reference.to_hex());
        let builder = request(&self.inner, Method::GET, &path)?;
        match self.inner.send(builder).await {
            Ok(_) => Ok(true),
            Err(e) if e.status() == Some(404) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// List every pinned reference — `GET /pins`.
    pub async fn list_pins(&self) -> Result<Vec<Reference>, Error> {
        let builder = request(&self.inner, Method::GET, "pins")?;
        #[derive(Deserialize)]
        struct Resp {
            references: Vec<Reference>,
        }
        let r: Resp = self.inner.send_json(builder).await?;
        Ok(r.references)
    }

    // ---- tags ---------------------------------------------------------

    /// Create a new tag — `POST /tags`.
    pub async fn create_tag(&self) -> Result<Tag, Error> {
        let builder = request(&self.inner, Method::POST, "tags")?;
        self.inner.send_json(builder).await
    }

    /// Get a tag by UID — `GET /tags/{uid}`.
    pub async fn get_tag(&self, uid: u32) -> Result<Tag, Error> {
        let path = format!("tags/{uid}");
        let builder = request(&self.inner, Method::GET, &path)?;
        self.inner.send_json(builder).await
    }

    /// bee-js name for [`ApiService::get_tag`].
    pub async fn retrieve_tag(&self, uid: u32) -> Result<Tag, Error> {
        self.get_tag(uid).await
    }

    /// List tags with optional pagination — `GET /tags`.
    pub async fn list_tags(
        &self,
        offset: Option<u32>,
        limit: Option<u32>,
    ) -> Result<Vec<Tag>, Error> {
        let mut builder = request(&self.inner, Method::GET, "tags")?;
        if let Some(o) = offset {
            if o > 0 {
                builder = builder.query(&[("offset", o.to_string())]);
            }
        }
        if let Some(l) = limit {
            if l > 0 {
                builder = builder.query(&[("limit", l.to_string())]);
            }
        }
        #[derive(Deserialize)]
        struct Resp {
            tags: Vec<Tag>,
        }
        let r: Resp = self.inner.send_json(builder).await?;
        Ok(r.tags)
    }

    /// Delete a tag — `DELETE /tags/{uid}`.
    pub async fn delete_tag(&self, uid: u32) -> Result<(), Error> {
        let path = format!("tags/{uid}");
        let builder = request(&self.inner, Method::DELETE, &path)?;
        self.inner.send(builder).await?;
        Ok(())
    }

    /// Update a tag — `PATCH /tags/{uid}`.
    pub async fn update_tag(&self, uid: u32, tag: &Tag) -> Result<(), Error> {
        let path = format!("tags/{uid}");
        let body = serde_json::to_vec(tag)?;
        let builder = request(&self.inner, Method::PATCH, &path)?
            .header("Content-Type", "application/json")
            .body(Bytes::from(body));
        self.inner.send(builder).await?;
        Ok(())
    }

    // ---- stewardship --------------------------------------------------

    /// Re-upload locally pinned data — `PUT /stewardship/{ref}`.
    pub async fn reupload(&self, reference: &Reference, batch_id: &BatchId) -> Result<(), Error> {
        let path = format!("stewardship/{}", reference.to_hex());
        let builder = request(&self.inner, Method::PUT, &path)?
            .header("swarm-postage-batch-id", batch_id.to_hex());
        self.inner.send(builder).await?;
        Ok(())
    }

    /// Check whether a reference is currently retrievable from the
    /// network — `GET /stewardship/{ref}`. Mirrors bee-js
    /// `Bee.isRetrievable`.
    pub async fn is_retrievable(&self, reference: &Reference) -> Result<bool, Error> {
        let path = format!("stewardship/{}", reference.to_hex());
        let builder = request(&self.inner, Method::GET, &path)?;
        #[derive(Deserialize)]
        struct Resp {
            is_retrievable: bool,
        }
        #[derive(Deserialize)]
        struct CamelResp {
            #[serde(rename = "isRetrievable")]
            is_retrievable: bool,
        }
        // Bee uses camelCase here; accept either via try_from on the
        // body bytes.
        let resp = self.inner.send(builder).await?;
        let bytes = resp.bytes().await?;
        if let Ok(r) = serde_json::from_slice::<CamelResp>(&bytes) {
            return Ok(r.is_retrievable);
        }
        let r: Resp = serde_json::from_slice(&bytes)?;
        Ok(r.is_retrievable)
    }

    // ---- grantee ------------------------------------------------------

    /// Get the grantees for a reference — `GET /grantee/{ref}`.
    pub async fn get_grantees(&self, reference: &Reference) -> Result<Vec<String>, Error> {
        let path = format!("grantee/{}", reference.to_hex());
        let builder = request(&self.inner, Method::GET, &path)?;
        // Live Bee returns a bare JSON array `["pk1", "pk2", …]`. The
        // earlier `{ "grantees": [...] }` wrapper shape never shipped.
        let v: Vec<String> = self.inner.send_json(builder).await?;
        Ok(v)
    }

    /// Create a new grantee list — `POST /grantee`.
    pub async fn create_grantees(
        &self,
        batch_id: &BatchId,
        grantees: &[String],
    ) -> Result<GranteeResponse, Error> {
        #[derive(Serialize)]
        struct Body<'a> {
            grantees: &'a [String],
        }
        let body = serde_json::to_vec(&Body { grantees })?;
        let builder = request(&self.inner, Method::POST, "grantee")?
            .header("Content-Type", "application/json")
            .header("Swarm-Postage-Batch-Id", batch_id.to_hex())
            .body(Bytes::from(body));
        self.inner.send_json(builder).await
    }

    /// Patch the grantees for a reference — `PATCH /grantee/{ref}`.
    pub async fn patch_grantees(
        &self,
        batch_id: &BatchId,
        reference: &Reference,
        history_address: &Reference,
        add: &[String],
        revoke: &[String],
    ) -> Result<GranteeResponse, Error> {
        #[derive(Serialize)]
        struct Body<'a> {
            #[serde(skip_serializing_if = "<[String]>::is_empty")]
            add: &'a [String],
            #[serde(skip_serializing_if = "<[String]>::is_empty")]
            revoke: &'a [String],
        }
        let body = serde_json::to_vec(&Body { add, revoke })?;
        let path = format!("grantee/{}", reference.to_hex());
        let builder = request(&self.inner, Method::PATCH, &path)?
            .header("Content-Type", "application/json")
            .header("Swarm-Postage-Batch-Id", batch_id.to_hex())
            .header("Swarm-Act-History-Address", history_address.to_hex())
            .body(Bytes::from(body));
        self.inner.send_json(builder).await
    }

    // ---- envelope -----------------------------------------------------

    /// Build an envelope (postage stamp signature triple) for a
    /// reference — `POST /envelope/{ref}`. Returns the issuer / index /
    /// timestamp / signature quadruple.
    pub async fn post_envelope(
        &self,
        batch_id: &BatchId,
        reference: &Reference,
    ) -> Result<EnvelopeResponse, Error> {
        let path = format!("envelope/{}", reference.to_hex());
        let builder = request(&self.inner, Method::POST, &path)?
            .header("Swarm-Postage-Batch-Id", batch_id.to_hex());
        self.inner.send_json(builder).await
    }
}

/// Response from [`ApiService::create_grantees`] /
/// [`ApiService::patch_grantees`]: the new grantee-set reference and
/// the ACT history root.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct GranteeResponse {
    /// Reference of the grantee set.
    #[serde(rename = "ref")]
    pub reference: String,
    /// ACT history reference.
    #[serde(rename = "historyref")]
    pub history_reference: String,
}

/// Response from [`ApiService::post_envelope`].
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct EnvelopeResponse {
    /// Stamper public key (hex).
    pub issuer: String,
    /// Stamp index (hex).
    pub index: String,
    /// Stamp timestamp (decimal seconds, hex string).
    pub timestamp: String,
    /// Stamp signature (hex).
    pub signature: String,
}

/// A Swarm tag — tracks sync progress for an upload.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    /// Tag UID (Bee returns this as `"uid"` on the wire).
    #[serde(default)]
    pub uid: u32,
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// Total chunks expected.
    #[serde(default)]
    pub total: i64,
    /// Chunks split.
    #[serde(default)]
    pub split: i64,
    /// Chunks already known to Bee.
    #[serde(default)]
    pub seen: i64,
    /// Chunks stored locally.
    #[serde(default)]
    pub stored: i64,
    /// Chunks sent over the network.
    #[serde(default)]
    pub sent: i64,
    /// Chunks confirmed synced.
    #[serde(default)]
    pub synced: i64,
    /// Address (root reference of the upload).
    #[serde(default)]
    pub address: String,
    /// Tag start timestamp (RFC 3339).
    #[serde(default, rename = "startedAt")]
    pub started_at: String,
}
