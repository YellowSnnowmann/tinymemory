//! [`NlpHost`] — spaCy entity extraction, run by the host.
//!
//! The summary tree extracts entities from a query so it can match them against
//! the entities indexed on chunks. spaCy gives it far better recall than the
//! regex fallback, but running spaCy means provisioning a Python toolchain,
//! downloading a model and supervising a server process — none of which belongs
//! in a memory engine.
//!
//! So the host owns the runtime and this trait is the one call the core makes
//! into it. The wire types are in [`tinymemory_api::host`], shared by both
//! sides.
//!
//! # Unwired falls back, it does not fail
//!
//! Unlike the embedding and Composio seams, an absent [`NlpHost`] is benign:
//! the caller already has a regex extractor for exactly this case (spaCy
//! disabled in config, model not provisioned yet, extraction erroring). Losing
//! recall is the documented degraded mode; failing the query would be worse.

use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;

use crate::Config;

pub use tinymemory_api::host::{SpacyEntity, SpacyResponse};

/// Runs spaCy extraction on the core's behalf.
#[async_trait]
pub trait NlpHost: Send + Sync + std::fmt::Debug {
    /// Extract entities and noun chunks from `text`.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the runtime is disabled, not provisioned, or the
    /// request fails. Callers fall back to regex extraction.
    async fn extract_spacy(&self, config: &Config, text: &str) -> Result<SpacyResponse, String>;
}

static HOST: RwLock<Option<Arc<dyn NlpHost>>> = RwLock::new(None);

/// Install the host's NLP runtime. Called once during startup wiring.
pub fn set_nlp_host(host: Arc<dyn NlpHost>) {
    *HOST.write() = Some(host);
}

/// Remove any installed host. For tests.
pub fn clear_nlp_host() {
    *HOST.write() = None;
}

/// The installed host, or `None` when nothing has been wired up.
#[must_use]
pub fn nlp_host() -> Option<Arc<dyn NlpHost>> {
    HOST.read().clone()
}

/// Extract entities from `text`.
///
/// # Errors
///
/// Returns `Err` when no host is installed or extraction fails — in both cases
/// the caller should fall back to regex extraction.
pub async fn extract_spacy(config: &Config, text: &str) -> Result<SpacyResponse, String> {
    let host = nlp_host().ok_or_else(|| "no NlpHost installed".to_string())?;
    host.extract_spacy(config, text).await
}
