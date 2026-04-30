//! High-level storage helpers built on top of [`Client`].
//!
//! Wraps the postage-batch lifecycle so callers can think in terms of
//! `(size, duration)` instead of `(amount, depth)`. The math comes
//! from [`crate::postage`] (depth/cost formulas) and [`crate::swarm`]
//! ([`Network`] for block time, [`Size`] for byte-count parsing).
//!
//! Mirrors bee-js's `client.buy_storage` / `client.get_storage_cost`.

use std::time::Duration;

use num_bigint::BigInt;

use crate::Client;
use crate::postage::PostageBatch;
use crate::swarm::{BatchId, Error, Network, Size};

/// Options for [`buy_storage`]. `network` shapes the per-block amount
/// math; `label` is the optional batch label; `immutable` selects an
/// immutable batch.
#[derive(Clone, Debug, Default)]
pub struct StorageOptions {
    /// Settlement chain (controls block time).
    pub network: Network,
    /// Optional batch label.
    pub label: Option<String>,
    /// Whether the batch is immutable. `None` keeps Bee's default.
    pub immutable: Option<bool>,
}

/// Storage-cost preview returned by [`get_storage_cost`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageCost {
    /// Stamp depth covering `size`.
    pub depth: u8,
    /// Per-block amount (PLUR) needed for `duration`.
    pub amount_per_chunk: BigInt,
    /// Total cost: `2^depth * amount_per_chunk` (PLUR).
    pub total_cost: BigInt,
    /// Duration in blocks (`duration / network.block_time`).
    pub blocks: u64,
}

/// Compute the depth + amount for a `(size, duration)` tuple, using
/// the chain's current price (PLUR/chunk/block) as the per-block
/// price floor. Does not perform the purchase; pure preview.
pub async fn get_storage_cost(
    client: &Client,
    size: Size,
    duration: Duration,
    network: Network,
) -> Result<StorageCost, Error> {
    let chain = client.debug().chain_state().await?;
    let blocks = network.seconds_to_blocks(duration.as_secs());
    let depth_i32 = crate::postage::get_depth_for_size(size.to_bytes());
    let depth: u8 = depth_i32
        .try_into()
        .map_err(|_| Error::argument(format!("computed depth {depth_i32} out of u8 range")))?;
    let amount_per_chunk = &chain.current_price * BigInt::from(blocks);
    let total_cost = crate::postage::get_stamp_cost(depth_i32, &amount_per_chunk);
    Ok(StorageCost {
        depth,
        amount_per_chunk,
        total_cost,
        blocks,
    })
}

/// Preview + buy in one call: compute the cost via
/// [`get_storage_cost`] and forward to
/// [`crate::postage::PostageApi::create_postage_batch`]. Returns the
/// freshly-minted [`BatchId`].
pub async fn buy_storage(
    client: &Client,
    size: Size,
    duration: Duration,
    opts: &StorageOptions,
) -> Result<BatchId, Error> {
    let cost = get_storage_cost(client, size, duration, opts.network).await?;
    client
        .postage()
        .create_postage_batch(&cost.amount_per_chunk, cost.depth, opts.label.as_deref())
        .await
}

/// Top up an existing batch by the per-chunk amount needed to extend
/// it for the given wall-clock `duration` on the chosen `network`.
/// The on-chain top-up amount is `current_price * blocks(duration)`.
pub async fn extend_storage_duration(
    client: &Client,
    batch_id: &BatchId,
    duration: Duration,
    network: Network,
) -> Result<(), Error> {
    let chain = client.debug().chain_state().await?;
    let blocks = network.seconds_to_blocks(duration.as_secs());
    let amount = &chain.current_price * BigInt::from(blocks);
    client.postage().top_up_batch(batch_id, &amount).await
}

/// Dilute (deepen) an existing batch so its effective capacity covers
/// `new_size`. No-op (returns `Ok(())`) if the batch is already deep
/// enough.
pub async fn extend_storage_size(
    client: &Client,
    batch_id: &BatchId,
    new_size: Size,
) -> Result<(), Error> {
    let batch: PostageBatch = client.postage().get_postage_batch(batch_id).await?;
    let target_depth_i32 = crate::postage::get_depth_for_size(new_size.to_bytes());
    let target: u8 = target_depth_i32
        .try_into()
        .map_err(|_| Error::argument(format!("computed depth {target_depth_i32} out of u8 range")))?;
    if target <= batch.depth {
        return Ok(());
    }
    client.postage().dilute_batch(batch_id, target).await
}

/// Total cost (PLUR) of extending a batch by `duration` on
/// `network`, given the current per-chunk price from `/chainstate`.
/// `current_price * blocks(duration) * 2^current_depth`.
pub async fn get_duration_extension_cost(
    client: &Client,
    batch_id: &BatchId,
    duration: Duration,
    network: Network,
) -> Result<BigInt, Error> {
    let batch = client.postage().get_postage_batch(batch_id).await?;
    let chain = client.debug().chain_state().await?;
    let blocks = network.seconds_to_blocks(duration.as_secs());
    let amount_per_chunk = &chain.current_price * BigInt::from(blocks);
    Ok(crate::postage::get_stamp_cost(
        batch.depth as i32,
        &amount_per_chunk,
    ))
}

/// Cost (PLUR) of growing a batch's effective capacity to cover
/// `new_size`. The dilution cost equals
/// `(2^new_depth - 2^old_depth) * batch.amount`. Returns `0` if the
/// batch is already deep enough.
pub async fn get_size_extension_cost(
    client: &Client,
    batch_id: &BatchId,
    new_size: Size,
) -> Result<BigInt, Error> {
    let batch = client.postage().get_postage_batch(batch_id).await?;
    let target_depth_i32 = crate::postage::get_depth_for_size(new_size.to_bytes());
    let target: u8 = target_depth_i32
        .try_into()
        .map_err(|_| Error::argument(format!("computed depth {target_depth_i32} out of u8 range")))?;
    if target <= batch.depth {
        return Ok(BigInt::from(0));
    }
    let amount = batch
        .amount
        .as_ref()
        .ok_or_else(|| Error::argument("batch missing amount"))?;
    let scale = BigInt::from(2u32).pow(target as u32) - BigInt::from(2u32).pow(batch.depth as u32);
    Ok(scale * amount)
}

/// Top-up amount (PLUR) needed for the batch to reach `target_bzz`
/// total spend, using the chain's current per-chunk price as the
/// floor. Mirrors bee-js `calculateTopUpForBzz`.
pub async fn calculate_top_up_for_bzz(
    client: &Client,
    batch_id: &BatchId,
    target_bzz: &BigInt,
) -> Result<BigInt, Error> {
    let batch = client.postage().get_postage_batch(batch_id).await?;
    let current = batch
        .amount
        .as_ref()
        .ok_or_else(|| Error::argument("batch missing amount"))?;
    if target_bzz <= current {
        return Ok(BigInt::from(0));
    }
    Ok(target_bzz - current)
}
