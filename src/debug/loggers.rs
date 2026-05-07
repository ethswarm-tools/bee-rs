//! `/loggers` endpoints — list, query by expression, set verbosity.
//! Mirrors bee-go's `pkg/debug/loggers.go`.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE;
use reqwest::Method;
use serde::Deserialize;

use crate::client::request;
use crate::swarm::Error;

use super::DebugApi;

/// Verbosity levels accepted by Bee's `PUT /loggers/{exp}/{verbosity}`
/// route (`pkg/api/logger.go`). Anything outside this set is rejected
/// by Bee with a 400.
pub const LOG_LEVELS: &[&str] = &["none", "error", "warning", "info", "debug", "all"];

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

    /// `GET /loggers/{base64url(expression)}` — loggers matching the
    /// regex / subsystem expression. The expression is base64url
    /// (padded) encoded per the Bee `/loggers/{exp}` contract.
    pub async fn loggers_by_expression(&self, expression: &str) -> Result<LoggerListing, Error> {
        let enc = URL_SAFE.encode(expression.as_bytes());
        let path = format!("loggers/{enc}");
        let builder = request(&self.inner, Method::GET, &path)?;
        self.inner.send_json(builder).await
    }

    /// `PUT /loggers/{base64url(expression)}/{verbosity}` — set the
    /// verbosity of every logger matching `expression`. `verbosity`
    /// must be one of [`LOG_LEVELS`] (`none|error|warning|info|debug|
    /// all`); anything else is rejected client-side with
    /// [`Error::Argument`] before the request is built.
    ///
    /// Example: `set_logger("node/pushsync", "debug")` raises the
    /// pushsync subsystem to debug verbosity. To bump every logger
    /// at once, pass `"."` (Bee treats it as a regex match-all).
    pub async fn set_logger(&self, expression: &str, verbosity: &str) -> Result<(), Error> {
        if !LOG_LEVELS.contains(&verbosity) {
            return Err(Error::argument(format!(
                "verbosity {verbosity:?} not in {LOG_LEVELS:?}"
            )));
        }
        let enc = URL_SAFE.encode(expression.as_bytes());
        let path = format!("loggers/{enc}/{verbosity}");
        let builder = request(&self.inner, Method::PUT, &path)?;
        self.inner.send(builder).await?;
        Ok(())
    }

    /// **Deprecated, broken method.** Bee's actual route is
    /// `PUT /loggers/{exp}/{verbosity}` — verbosity is **mandatory**
    /// in the path, but this method emits `PUT /loggers/{exp}` which
    /// 404s on every real Bee build. Always returned 404 against a
    /// live node; only ever "succeeded" against mock servers wired
    /// to the wrong path.
    ///
    /// Use [`DebugApi::set_logger`] instead — it takes both the
    /// expression and verbosity and emits the correct path.
    #[deprecated(
        since = "1.6.0",
        note = "broken — emits the wrong path. Use `set_logger(expr, verbosity)` instead."
    )]
    pub async fn set_logger_verbosity(&self, _expression: &str) -> Result<(), Error> {
        Err(Error::argument(
            "set_logger_verbosity is broken — Bee's route requires a verbosity component. \
             Call set_logger(expression, verbosity) instead.",
        ))
    }
}
