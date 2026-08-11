//! The process-global [`EmbeddingHost`], and the accessors the extracted code
//! calls in place of the host's `inference::embeddings` factory functions.
//!
//! Mirrors [`crate::events`]'s shape — see that module for why provider
//! construction is reached through a global rather than threaded as a
//! parameter.
//!
//! # Unwired is an error here, unlike the event sink
//!
//! [`crate::events::publish`] drops events when no sink is installed, because
//! the work being announced already happened. Embedding construction is the
//! opposite: silently returning "no embedder" would write vectors into the
//! wrong space or downgrade a semantic query to lexical-only, and neither
//! failure is visible until a later search returns the wrong answer. So the
//! accessors here fail loudly, and callers propagate.

use std::sync::Arc;

use parking_lot::RwLock;

pub use tinymemory_api::host::EmbeddingHost;

static HOST: RwLock<Option<Arc<dyn EmbeddingHost>>> = RwLock::new(None);

/// The message every accessor fails with before a host wires itself up.
const NOT_INSTALLED: &str =
    "no EmbeddingHost installed — the host must call memory::embedding_host::set_embedding_host \
     during startup wiring, before any memory work begins";

/// Install the host's embedding factory. Called once during startup wiring.
/// Calling it again replaces it, which is what test harnesses want between
/// cases.
pub fn set_embedding_host(host: Arc<dyn EmbeddingHost>) {
    *HOST.write() = Some(host);
}

/// Remove any installed host. For tests.
pub fn clear_embedding_host() {
    *HOST.write() = None;
}

/// The installed host, or `None` when nothing has been wired up.
#[must_use]
pub fn embedding_host() -> Option<Arc<dyn EmbeddingHost>> {
    HOST.read().clone()
}

/// The installed host.
///
/// # Errors
///
/// Returns `Err` when no host has been installed.
pub fn require_embedding_host() -> Result<Arc<dyn EmbeddingHost>, String> {
    embedding_host().ok_or_else(|| NOT_INSTALLED.to_string())
}

/// The host's default embedding provider — the managed cloud embedder.
///
/// # Errors
///
/// Returns `Err` when no [`EmbeddingHost`] has been installed.
pub fn default_embedding_provider(
) -> Result<Arc<dyn tinymemory_api::host::EmbeddingProvider>, String> {
    Ok(require_embedding_host()?.default_embedding_provider())
}

/// Serialises tests that mutate embedding-related process environment.
///
/// The host has its own guard over the same variables (`inference::local::
/// inference_test_guard`). They are deliberately *different* locks: each crate's
/// tests link into their own binary and therefore their own process, so a shared
/// lock would buy nothing and would mean the contract crate owning a mutex for
/// the host's benefit.
#[must_use]
pub fn embedding_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());
    GUARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A stub [`EmbeddingHost`] for tests.
///
/// Several tests assert on the cloud-fallback tuple, which the core no longer
/// owns — the managed model id and dimensionality are the host's to state, so
/// with no host installed there is nothing true to assert. Installing this
/// gives those tests a known answer without reaching for the real provider
/// stack.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct TestEmbeddingHost;

#[cfg(test)]
impl TestEmbeddingHost {
    /// The model id [`Self`] reports as the managed cloud default.
    pub(crate) const CLOUD_MODEL: &'static str = "test-cloud-embed";
    /// The dimensionality [`Self::CLOUD_MODEL`] emits.
    pub(crate) const CLOUD_DIMENSIONS: usize = 1024;

    /// Install this stub as the process-global embedding host.
    pub(crate) fn install() {
        set_embedding_host(Arc::new(Self));
    }
}

#[cfg(test)]
impl EmbeddingHost for TestEmbeddingHost {
    fn resolve_api_key(&self, _provider: &str) -> Option<String> {
        None
    }

    fn ollama_base_url(&self) -> String {
        std::env::var("OPENHUMAN_OLLAMA_BASE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:11434".to_string())
    }

    fn default_embedding_provider(&self) -> Arc<dyn tinymemory_api::host::EmbeddingProvider> {
        Arc::new(tinymemory_api::host::NoopEmbedding)
    }

    fn create_embedding_provider_with_credentials(
        &self,
        _provider: &str,
        _model: &str,
        _dims: usize,
        _api_key: &str,
        _custom_endpoint: Option<&str>,
    ) -> Result<Box<dyn tinymemory_api::host::EmbeddingProvider>, String> {
        Ok(Box::new(tinymemory_api::host::NoopEmbedding))
    }

    fn model_supports_dimensions(&self, model: &str) -> bool {
        // Mirrors the host's rule rather than answering `true`: the tests that
        // reach this are about the *ladder's* reaction to a non-reducible
        // model, so a stub that says everything is reducible would make them
        // pass without exercising anything.
        model.starts_with("text-embedding-3-")
    }

    fn cloud_embedding_provider(
        &self,
        _model: &str,
        _dims: usize,
    ) -> Result<Box<dyn tinymemory_api::host::EmbeddingProvider>, String> {
        Ok(Box::new(tinymemory_api::host::NoopEmbedding))
    }

    fn default_cloud_embedding_model(&self) -> &str {
        Self::CLOUD_MODEL
    }

    fn default_cloud_embedding_dimensions(&self) -> usize {
        Self::CLOUD_DIMENSIONS
    }

    fn ollama_embedding_provider(
        &self,
        _base_url: &str,
        _model: &str,
        _dims: usize,
    ) -> Result<Box<dyn tinymemory_api::host::EmbeddingProvider>, String> {
        Ok(Box::new(tinymemory_api::host::NoopEmbedding))
    }
}
