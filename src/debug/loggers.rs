//! `/loggers` endpoints — list, query by expression, set verbosity.
//! Mirrors bee-go's `pkg/debug/loggers.go`.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use reqwest::Method;
use serde::Deserialize;

use crate::client::request;
use crate::swarm::Error;

use super::DebugApi;

/// One logger row in [`LoggerListing`].
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Logger {
    /// Fully qualified logger name.
    pub logger: String,
    /// Verbosity level (`"info"`, `"debug"`, `"warning"`, …).
    pub verbosity: String,
    /// Subsystem the logger belongs to.
    pub subsystem: String,
    /// Stable logger ID (base64).
    pub id: String,
}

/// `GET /loggers` and `GET /loggers/{expr}` response. The `tree`
/// payload is recursive and rarely needed — it is preserved as a raw
/// JSON value.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct LoggerListing {
    /// Recursive logger tree as raw JSON.
    #[serde(default)]
    pub tree: serde_json::Value,
    /// Flat list of registered loggers.
    #[serde(default)]
    pub loggers: Vec<Logger>,
}

impl DebugApi {
    /// `GET /loggers` — every logger registered in the running node.
    pub async fn loggers(&self) -> Result<LoggerListing, Error> {
        let builder = request(&self.inner, Method::GET, "loggers")?;
        self.inner.send_json(builder).await
    }

    /// `GET /loggers/{base64(expression)}` — loggers matching the
    /// regex / subsystem expression. The expression is base64-encoded
    /// per the Bee `/loggers/{exp}` contract.
    pub async fn loggers_by_expression(&self, expression: &str) -> Result<LoggerListing, Error> {
        let enc = STANDARD.encode(expression.as_bytes());
        let path = format!("loggers/{enc}");
        let builder = request(&self.inner, Method::GET, &path)?;
        self.inner.send_json(builder).await
    }

    /// `PUT /loggers/{base64(expression)}` — set verbosity for loggers
    /// matching the expression.
    pub async fn set_logger_verbosity(&self, expression: &str) -> Result<(), Error> {
        let enc = STANDARD.encode(expression.as_bytes());
        let path = format!("loggers/{enc}");
        let builder = request(&self.inner, Method::PUT, &path)?;
        self.inner.send(builder).await?;
        Ok(())
    }
}
