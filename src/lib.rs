//! bee-rs — Rust client for the Swarm Bee API.
//!
//! Functional parity target with [bee-js] (the canonical TypeScript
//! client) and [bee-go] (the typed Go port). bee-go is the primary
//! reference for shape and behavior; bee-js is the source of truth for
//! wire-format edge cases.
//!
//! [bee-js]: https://github.com/ethersphere/bee-js
//! [bee-go]: https://github.com/ethswarm-tools/bee-go

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod api;
pub(crate) mod client;
pub mod debug;
pub mod dev;
pub mod file;
pub mod gsoc;
pub mod manifest;
pub mod postage;
pub mod pss;
pub mod storage;
pub mod swarm;

pub use dev::DevClient;

pub use client::Client;
pub use swarm::{Error, Result};
