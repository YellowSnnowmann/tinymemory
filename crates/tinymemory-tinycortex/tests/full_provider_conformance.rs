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

/// The KV write path canonicalizes identifiers (the shim in `tinymemory-core`
/// routes every `set_*`/`delete_*` through `canonical_identifier`), so a read
/// path that compares the raw caller key misses every rewritten key: put→get
/// answered `None` while put→delete answered `true`. `kv_get` and `kv_list`
/// must apply the same transform the write path did.
#[tokio::test(flavor = "multi_thread")]
async fn kv_reads_find_a_key_the_canonicalizer_rewrites() {
    use tinymemory_api::provider::MemoryProvider;
    use tinymemory_core::store::safety::canonical_identifier;

    let workspace = tempfile::tempdir().expect("workspace");
    let provider = provider_over(workspace.path());
    let graph = provider.as_graph().expect("the full provider serves Graph");

    // A formatted national ID is strict-gated PII, so the write path rewrites
    // it. (A bare Luhn-valid digit run would NOT do here: the strict gate
    // deliberately ignores bare-numeric shapes so scanner-built identifiers —
    // timestamps, phone-shaped JIDs — keep their identity.)
    let key = "ssn-123-45-6789";
    let canonical = canonical_identifier(key);
    assert_ne!(
        canonical, key,
        "fixture must be a key the canonicalizer rewrites"
    );

    let value = serde_json::json!({"ticket": 42});
    graph
        .kv_put(None, key, value.clone())
        .await
        .expect("kv_put");

    let record = graph
        .kv_get(None, key)
        .await
        .expect("kv_get")
        .expect("kv_get must find the key it just put under the same raw key");
    assert_eq!(record.value, value, "kv_get surfaced another record");
    assert_eq!(
        record.key, canonical,
        "the stored key is the canonical form, and reads surface it as stored"
    );

    // Prefix matching is over canonical stored keys, so the raw caller key
    // works as a prefix of its own record.
    let listed = graph.kv_list(None, Some(key), 16).await.expect("kv_list");
    assert!(
        listed.iter().any(|r| r.key == canonical),
        "kv_list under the raw-key prefix must reach the rewritten record, got {listed:?}"
    );

    // Delete already routed through the canonicalizing shim; the fix must not
    // break that half of the symmetry.
    assert!(
        graph.kv_delete(None, key).await.expect("kv_delete"),
        "kv_delete must find the rewritten key"
    );
    assert!(
        graph
            .kv_get(None, key)
            .await
            .expect("kv_get after delete")
            .is_none(),
        "the record must be gone after kv_delete reported true"
    );
}

/// The same symmetry holds for namespaced KV rows: namespace and key are both
/// canonicalized on write, so both must be canonicalized on read.
#[tokio::test(flavor = "multi_thread")]
async fn namespaced_kv_reads_apply_the_write_path_canonicalization() {
    use tinymemory_api::provider::MemoryProvider;

    let workspace = tempfile::tempdir().expect("workspace");
    let provider = provider_over(workspace.path());
    let graph = provider.as_graph().expect("the full provider serves Graph");

    // A namespace the canonicalizer rewrites, guarded like the key leg — a
    // no-op namespace would prove only key symmetry under a namespace.
    let ns = "ssn-123-45-6789";
    assert_ne!(
        tinymemory_core::store::safety::canonical_identifier(ns),
        ns,
        "the fixture namespace must be one the canonicalizer rewrites"
    );
    let key = "cliente-RFC-VECJ880326XK4";
    let value = serde_json::json!("rewritten");
    graph
        .kv_put(Some(ns), key, value.clone())
        .await
        .expect("kv_put");

    let record = graph
        .kv_get(Some(ns), key)
        .await
        .expect("kv_get")
        .expect("a namespaced put must be readable back under the same raw key");
    assert_eq!(record.value, value);
    assert!(
        graph.kv_delete(Some(ns), key).await.expect("kv_delete"),
        "namespaced kv_delete must stay symmetric with kv_put"
    );
}
