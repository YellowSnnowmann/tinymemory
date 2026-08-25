//! Issue #18 §B5 / §E4 — the acceptance test for the sync section:
//! **Composio Gmail sync completes end to end against a driver that is not
//! TinyCortex.**
//!
//! The pieces under test, and what each proves:
//!
//! - A mock Composio (wiremock, loopback-only) serves two pages of
//!   `GMAIL_FETCH_EMAILS` — pagination, cursor advance and dedup are real.
//! - The pipeline is `sync::pipelines::composio::GmailSyncPipeline`, run
//!   through the real `SyncDispatcher` — the exact production path.
//! - The host is `PipelineHost::without_tree_ingest` over a `MemoryClient`
//!   bound to the **namespace store** — the driver #42 (§A3) registers as its
//!   own non-TinyCortex class. No engine is initialised, no tree exists, and
//!   the pipeline code under `core/src/sync/` names no engine module. The
//!   engine's `KvStore` appears only as a storage *library* inside the
//!   namespace store's SQLite file — it is not the bound driver, and nothing
//!   in the pipeline knows it is there.
//!
//! Offline by construction: the only socket is wiremock's 127.0.0.1 listener.

use std::sync::Arc;

use serde_json::json;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Match, Mock, MockServer, Request, ResponseTemplate};

/// Matches the *first* fetch only: an execute body whose arguments carry no
/// `page_token`. `body_partial_json` cannot express absence, and without this
/// the page-1 mount also matches the page-2 request (which still contains
/// `max_results`), serving page 1 twice — dedup then eats the repeats and the
/// test fails honestly but confusingly.
struct NoPageToken;

impl Match for NoPageToken {
    fn matches(&self, request: &Request) -> bool {
        serde_json::from_slice::<serde_json::Value>(&request.body)
            .map(|body| body["arguments"].get("page_token").is_none())
            .unwrap_or(false)
    }
}

use tinymemory_core::store::MemoryClient;

/// The one piece of host wiring `MemoryClient` requires: an embedding host.
/// Noop — this test is about the sync path, and recall is not asserted.
#[derive(Debug)]
struct NoopEmbeddingHost;

impl tinymemory_api::host::EmbeddingHost for NoopEmbeddingHost {
    fn resolve_api_key(&self, _provider: &str) -> Option<String> {
        None
    }

    fn ollama_base_url(&self) -> String {
        "http://127.0.0.1:1".into()
    }

    fn default_embedding_provider(
        &self,
    ) -> std::sync::Arc<dyn tinymemory_api::host::EmbeddingProvider> {
        std::sync::Arc::new(tinymemory_api::host::NoopEmbedding)
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
// `load`/`save` are the extension trait, not inherent methods: `SyncState`
// itself moved to the contract crate, which stays free of I/O, so persistence
// lives here in the engine and arrives through `PersistedSyncState`.
use tinymemory_core::sync::composio::providers::sync_state::{
    PersistedSyncState, SyncState, KV_NAMESPACE,
};
use tinymemory_core::sync::pipelines::composio::ComposioClient;
use tinymemory_core::sync::pipelines::composio::GmailSyncPipeline;
use tinymemory_core::sync::pipelines::dispatcher::SyncDispatcher;
use tinymemory_core::sync::pipelines::host::PipelineHost;
use tinymemory_core::sync::pipelines::traits::{
    ComposioMode, ComposioSyncConfig, PipelineConfig, SecretString, SyncPipeline,
};

fn message(id: &str, subject: &str, body_md: &str) -> serde_json::Value {
    json!({
        "id": id,
        "subject": subject,
        "from": "sender@example.com",
        "markdown": body_md,
        "messageTimestamp": "2026-01-02T03:04:05Z",
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn composio_gmail_sync_completes_against_the_namespace_driver() {
    // ── The mock Composio ────────────────────────────────────────────────
    let server = MockServer::start().await;

    // Page 1: two messages and a cursor. Matched on the *absence* of a page
    // token in the arguments, so retries stay deterministic.
    Mock::given(method("POST"))
        .and(path("/tools/execute/GMAIL_FETCH_EMAILS"))
        .and(body_partial_json(json!({"arguments": {"max_results": 25}})))
        .and(NoPageToken)
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "successful": true,
            "data": {
                "messages": [
                    message("m1", "First", "hello one"),
                    message("m2", "Second", "hello two"),
                ],
                "nextPageToken": "page-2",
            }
        })))
        .mount(&server)
        .await;

    // Page 2: one message, no cursor — the sync must stop here.
    Mock::given(method("POST"))
        .and(path("/tools/execute/GMAIL_FETCH_EMAILS"))
        .and(body_partial_json(
            json!({"arguments": {"page_token": "page-2"}}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "successful": true,
            "data": {
                "messages": [message("m3", "Third", "hello three")],
            }
        })))
        .mount(&server)
        .await;

    // ── The non-TinyCortex driver ────────────────────────────────────────
    tinymemory_core::embedding_host::set_embedding_host(Arc::new(NoopEmbeddingHost));
    let workspace = tempfile::tempdir().expect("workspace");
    let memory = Arc::new(
        MemoryClient::from_workspace_dir(workspace.path().to_path_buf())
            .expect("bind the namespace store"),
    );

    // ── The engine-free pipeline, on the production dispatcher ───────────
    let composio = ComposioSyncConfig {
        mode: ComposioMode::Direct,
        base_url: server.uri(),
        api_key: Some(SecretString::new("test-key")),
        bearer_token: None,
        entity_id: Some("entity-1".into()),
    };
    let pipeline = Arc::new(GmailSyncPipeline::new(
        ComposioClient::new(composio),
        "conn-1",
    ));
    let pipeline_id = pipeline.id().to_owned();

    let host = Arc::new(PipelineHost::without_tree_ingest(memory.clone()));
    let mut dispatcher = SyncDispatcher::new();
    dispatcher.register(pipeline).expect("register pipeline");
    let outcome = dispatcher
        .tick(&pipeline_id, &PipelineConfig::default(), &host.context())
        .await
        .expect("gmail sync must complete");

    // ── End to end: the outcome ──────────────────────────────────────────
    assert_eq!(
        outcome.records_ingested, 3,
        "all three messages ingest; outcome={outcome:?}"
    );
    assert!(!outcome.more_pending, "page 2 carried no cursor");

    // ── End to end: the documents landed in the bound store ──────────────
    let docs = memory
        .list_documents(Some("skill-gmail"))
        .await
        .expect("list synced documents");
    let listed = docs
        .as_array()
        .or_else(|| docs.get("documents").and_then(|d| d.as_array()))
        .map(|a| a.len())
        .unwrap_or_default();
    assert_eq!(listed, 3, "three documents in skill-gmail: {docs}");

    // ── End to end: the canonical markdown, not raw JSON ─────────────────
    let doc = memory
        .get_document("skill-gmail", "gmail:m1")
        .await
        .expect("read gmail:m1")
        .expect("gmail:m1 stored");
    assert!(
        doc.content.contains("From: sender@example.com") && doc.content.contains("hello one"),
        "canonical markdown stored, got: {}",
        doc.content
    );

    // ── End to end: cursor + dedup state persisted through the KV seam ───
    let state = SyncState::load(&*host, "gmail", "conn-1")
        .await
        .expect("load persisted state");
    assert!(state.is_synced("m1") && state.is_synced("m3"), "dedup ids");
    let raw = memory
        .kv_get(Some(KV_NAMESPACE), "gmail:conn-1")
        .await
        .expect("kv read");
    assert!(raw.is_some(), "sync state persisted under {KV_NAMESPACE}");
}
