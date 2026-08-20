//! The conformance suite over the FULL eighteen-family driver (#18 §E1/§E3).
//!
//! `conformance_test.rs` (in-lib) covers `crate::provider` — the mandatory
//! three families over any engine backend. This target covers
//! [`tinymemory_tinycortex::engine::TinycortexProvider`], which the in-lib
//! test cannot: the provider needs a `MemoryClient`, and a `MemoryClient`
//! needs the host's process-global embedding seam installed. A process global
//! makes tests order-dependent inside a shared binary, so this lives in its
//! own integration target that owns the global for its whole lifetime — the
//! arrangement the in-lib test's module doc promised.
//!
//! The seam is the same noop shape the §B5 acceptance test uses: recall
//! quality is not under test here, contract shape is.

// A panic in a test IS the failure report — same allowance the in-lib
// conformance test carries.
#![allow(clippy::expect_used)]

use std::sync::Arc;

use tinymemory_tinycortex::engine::{EngineRuntimeConfig, TinycortexProvider};

/// The one piece of host wiring `MemoryClient` requires.
#[derive(Debug)]
struct NoopEmbeddingHost;

impl tinymemory_api::host::EmbeddingHost for NoopEmbeddingHost {
    fn resolve_api_key(&self, _provider: &str) -> Option<String> {
        None
    }

    fn ollama_base_url(&self) -> String {
        "http://127.0.0.1:1".into()
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

    fn model_supports_dimensions(&self, _model: &str) -> bool {
        false
    }

    fn cloud_embedding_provider(
        &self,
        _model: &str,
        _dims: usize,
    ) -> Result<Box<dyn tinymemory_api::host::EmbeddingProvider>, String> {
        Ok(Box::new(tinymemory_api::host::NoopEmbedding))
    }

    fn default_cloud_embedding_model(&self) -> &str {
        "noop"
    }

    fn default_cloud_embedding_dimensions(&self) -> usize {
        8
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

fn provider_over(workspace: &std::path::Path) -> TinycortexProvider {
    tinymemory_core::embedding_host::set_embedding_host(Arc::new(NoopEmbeddingHost));
    let client = Arc::new(
        tinymemory_core::store::MemoryClient::from_workspace_dir(workspace.to_path_buf())
            .expect("open the workspace store"),
    );
    let config = EngineRuntimeConfig {
        workspace_dir: workspace.to_path_buf(),
        config_path: workspace.join("config.toml"),
        memory: Default::default(),
        memory_tree: Default::default(),
        scheduler_gate: Default::default(),
        local_ai: Default::default(),
        embeddings_provider: None,
        memory_provider: None,
        default_model: None,
        default_temperature: 0.2,
        output_language: None,
        memory_sources: serde_json::Value::Null,
    };
    TinycortexProvider::new("tinycortex".into(), config, client)
}

#[tokio::test(flavor = "multi_thread")]
async fn the_full_tinycortex_provider_upholds_the_contract() {
    let workspace = tempfile::tempdir().expect("workspace");
    let provider = provider_over(workspace.path());
    tinymemory_conformance::assert_provider(Arc::new(provider)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_full_provider_actually_retains() {
    let workspace = tempfile::tempdir().expect("workspace");
    let provider = provider_over(workspace.path());
    assert!(
        tinymemory_conformance::retains_writes(&provider).await,
        "the workspace store must retain writes, or the suite above asserts \
         almost nothing"
    );
}
