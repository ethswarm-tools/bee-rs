//! `/stamps` and `/batches` endpoint methods.

use num_bigint::BigInt;
use reqwest::Method;
use serde::Deserialize;

use crate::client::request;
use crate::swarm::{BatchId, Error};

use super::PostageApi;
use super::types::{GlobalPostageBatch, PostageBatch, PostageBatchBuckets};

#[derive(Deserialize)]
struct StampsResp {
    stamps: Vec<PostageBatch>,
}

#[derive(Deserialize)]
struct BatchesResp {
    batches: Vec<GlobalPostageBatch>,
}

#[derive(Deserialize)]
struct BatchIdResp {
    #[serde(rename = "batchID")]
    batch_id: String,
}

impl PostageApi {
    /// Every postage batch owned by this node — `GET /stamps`.
    pub async fn get_postage_batches(&self) -> Result<Vec<PostageBatch>, Error> {
        let builder = request(&self.inner, Method::GET, "stamps")?;
        let res: StampsResp = self.inner.send_json(builder).await?;
        Ok(res.stamps)
    }

    /// Single owned batch by id — `GET /stamps/{id}`.
    pub async fn get_postage_batch(&self, batch_id: &BatchId) -> Result<PostageBatch, Error> {
        let path = format!("stamps/{}", batch_id.to_hex());
        let builder = request(&self.inner, Method::GET, &path)?;
        self.inner.send_json(builder).await
    }

    /// Per-bucket collision stats — `GET /stamps/{id}/buckets`.
    pub async fn get_postage_batch_buckets(
        &self,
        batch_id: &BatchId,
    ) -> Result<PostageBatchBuckets, Error> {
        let path = format!("stamps/{}/buckets", batch_id.to_hex());
        let builder = request(&self.inner, Method::GET, &path)?;
        self.inner.send_json(builder).await
    }

    /// Every chain-visible postage batch — `GET /batches`. Returns the
    /// chain-wide view (no owner-only fields).
    pub async fn get_global_postage_batches(&self) -> Result<Vec<GlobalPostageBatch>, Error> {
        let builder = request(&self.inner, Method::GET, "batches")?;
        let res: BatchesResp = self.inner.send_json(builder).await?;
        Ok(res.batches)
    }

    /// Buy a new postage batch — `POST /stamps/{amount}/{depth}`.
    /// Returns the freshly-minted [`BatchId`].
    pub async fn create_postage_batch(
        &self,
        amount: &BigInt,
        depth: u8,
        label: Option<&str>,
    ) -> Result<BatchId, Error> {
        let path = format!("stamps/{amount}/{depth}");
        let mut builder = request(&self.inner, Method::POST, &path)?;
        if let Some(l) = label {
            builder = builder.query(&[("label", l)]);
        }
        let res: BatchIdResp = self.inner.send_json(builder).await?;
        BatchId::from_hex(&res.batch_id)
    }

    /// Top up an existing batch — `PATCH /stamps/topup/{id}/{amount}`.
    pub async fn top_up_batch(&self, batch_id: &BatchId, amount: &BigInt) -> Result<(), Error> {
        let path = format!("stamps/topup/{}/{amount}", batch_id.to_hex());
        let builder = request(&self.inner, Method::PATCH, &path)?;
        self.inner.send(builder).await?;
        Ok(())
    }

    /// Increase the depth of an existing batch — `PATCH /stamps/dilute/{id}/{depth}`.
    pub async fn dilute_batch(&self, batch_id: &BatchId, depth: u8) -> Result<(), Error> {
        let path = format!("stamps/dilute/{}/{depth}", batch_id.to_hex());
        let builder = request(&self.inner, Method::PATCH, &path)?;
        self.inner.send(builder).await?;
        Ok(())
    }
}
