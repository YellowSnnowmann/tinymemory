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

#[tokio::test(flavor = "multi_thread")]
async fn document_source_graph_goals_and_tool_rule_state_transitions_round_trip() {
    use tinymemory_api::goals::{GoalItem, GoalsDoc};
    use tinymemory_api::provider::types::SourceItem;
    use tinymemory_api::provider::MemoryProvider;
    use tinymemory_api::tool_memory::{ToolMemoryPriority, ToolMemoryRule, ToolMemorySource};
    use tinymemory_api::types::{GraphRelationRecord, MemoryTaint, NamespaceDocumentInput};

    let workspace = tempfile::tempdir().expect("workspace");
    let provider = provider_over(workspace.path());

    let documents = provider.as_documents().expect("Documents");
    let input = NamespaceDocumentInput {
        namespace: "project".into(),
        key: "brief".into(),
        title: "Brief".into(),
        content: "Ship the deterministic test suite".into(),
        source_type: "upload".into(),
        priority: "high".into(),
        tags: vec!["tests".into()],
        metadata: serde_json::json!({"ticket": 81}),
        category: "core".into(),
        session_id: Some("session-1".into()),
        document_id: None,
        taint: MemoryTaint::ExternalSync,
    };
    let document_id = documents.put_document(input).await.expect("put document");
    let document = documents
        .get_document("project", "brief")
        .await
        .expect("get document")
        .expect("document present");
    assert_eq!(document.document_id, document_id);
    assert_eq!(document.metadata, serde_json::json!({"ticket": 81}));
    assert_eq!(document.taint, MemoryTaint::ExternalSync);
    assert!(documents
        .list_namespaces()
        .await
        .expect("namespaces")
        .contains(&"project".into()));
    documents
        .delete_document("project", &document_id)
        .await
        .expect("delete document");
    assert!(documents
        .get_document("project", "brief")
        .await
        .expect("get after delete")
        .is_none());

    let source = provider.as_sources().expect("SourceSink");
    let outcome = source
        .accept_source_items(
            "drive-1",
            "drive",
            vec![SourceItem {
                item_id: "item-1".into(),
                title: "Source item".into(),
                content: "source body".into(),
                mime: Some("text/plain".into()),
                url: Some("https://example.invalid/item-1".into()),
                updated_at_ms: Some(42),
                tags: vec!["source".into()],
            }],
            MemoryTaint::ExternalSync,
        )
        .await
        .expect("accept source item");
    assert_eq!(outcome.written, 1);
    assert_eq!(
        source
            .forget_source("drive-1")
            .await
            .expect("forget source"),
        1
    );

    let graph = provider.as_graph().expect("Graph");
    let relation = GraphRelationRecord {
        namespace: Some("project".into()),
        subject: "suite".into(),
        predicate: "covers".into(),
        object: "adapter".into(),
        attrs: serde_json::json!({"confidence": 1.0}),
        updated_at: 0.0,
        evidence_count: 0,
        order_index: None,
        document_ids: Vec::new(),
        chunk_ids: Vec::new(),
    };
    graph.put_relation(relation).await.expect("put relation");
    let relations = graph
        .relations(Some("project"), Some("suite"), Some("covers"), 1)
        .await
        .expect("relations");
    assert_eq!(relations.len(), 1);
    assert_eq!(relations[0].object, "ADAPTER");
    assert!(graph
        .relations(Some("project"), None, None, 0)
        .await
        .expect("zero limit")
        .is_empty());

    let goals = provider.as_goals().expect("Goals");
    let expected_goals = GoalsDoc {
        items: vec![GoalItem::new("g1", "finish coverage")],
    };
    goals
        .set_goals(expected_goals.clone())
        .await
        .expect("set goals");
    assert_eq!(goals.goals().await.expect("goals"), expected_goals);

    let tools = provider.as_tool_memory().expect("ToolMemory");
    let rule = ToolMemoryRule::new(
        "shell",
        "never delete broad paths",
        ToolMemoryPriority::Critical,
        ToolMemorySource::UserExplicit,
    );
    let rule_id = rule.id.clone();
    tools.put_tool_rule(rule).await.expect("put tool rule");
    let rules = tools.tool_rules("shell").await.expect("tool rules");
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].id, rule_id);
    assert!(tools
        .delete_tool_rule("shell", &rule_id)
        .await
        .expect("delete rule"));
    assert!(!tools
        .delete_tool_rule("shell", &rule_id)
        .await
        .expect("delete missing rule"));
}

#[tokio::test(flavor = "multi_thread")]
async fn people_profile_and_episodic_lifecycles_are_real_and_typed() {
    use tinymemory_api::error::MemoryError;
    use tinymemory_api::provider::{
        EpisodicTurn, FacetType, MemoryProvider, PersonHandle, PersonInteraction, UserState,
    };

    let workspace = tempfile::tempdir().expect("workspace");
    let provider = provider_over(workspace.path());

    let people = provider.as_people().expect("People");
    let handle = PersonHandle::Email(" Friend@Example.com ".into());
    assert!(people
        .resolve_handle(&handle, false)
        .await
        .expect("resolve absent")
        .is_none());
    let resolved = people
        .resolve_handle(&handle, true)
        .await
        .expect("resolve/create")
        .expect("person created");
    assert!(resolved.created);
    assert_eq!(
        people
            .resolve_handle(&PersonHandle::Email("friend@example.com".into()), false)
            .await
            .expect("resolve canonical")
            .expect("same person")
            .id,
        resolved.id
    );
    assert!(matches!(
        people
            .record_interaction(&PersonInteraction {
                person_id: resolved.id.clone(),
                at: "not-a-time".into(),
                is_outbound: true,
                length: 10,
            })
            .await,
        Err(MemoryError::Invalid(_))
    ));
    people
        .record_interaction(&PersonInteraction {
            person_id: resolved.id.clone(),
            at: "2026-08-21T00:00:00Z".into(),
            is_outbound: true,
            length: 120,
        })
        .await
        .expect("record interaction");
    assert_eq!(
        people
            .score_person(&resolved.id)
            .await
            .expect("score")
            .expect("person score")
            .interaction_count,
        1
    );

    let profile = provider.as_profile().expect("Profile");
    profile
        .upsert_provider_facet(
            "facet-1",
            FacetType::Preference,
            "style/verbosity",
            "concise",
            0.9,
            Some("segment-1"),
            100.0,
        )
        .await
        .expect("upsert facet");
    let facet = profile
        .get_facet("style/verbosity")
        .await
        .expect("get facet")
        .expect("facet present");
    assert_eq!(facet.value, "concise");
    assert!(profile
        .set_facet_user_state("style/verbosity", UserState::Pinned)
        .await
        .expect("pin facet"));
    assert_eq!(
        profile
            .get_facet("style/verbosity")
            .await
            .expect("get pinned facet")
            .expect("facet present")
            .user_state,
        UserState::Pinned
    );
    assert!(profile
        .delete_facet("style/verbosity")
        .await
        .expect("delete facet"));

    let episodic = provider.as_episodic().expect("Episodic");
    let turn_id = episodic
        .insert_turn(&EpisodicTurn {
            id: None,
            session_id: "session-1".into(),
            timestamp: 10.0,
            role: "user".into(),
            content: "remember the test".into(),
            lesson: Some("verify state".into()),
            tool_calls_json: None,
            cost_microdollars: -1,
        })
        .await
        .expect("insert turn");
    let turns = episodic
        .session_turns("session-1")
        .await
        .expect("session turns");
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].id, Some(turn_id));
    assert_eq!(turns[0].cost_microdollars, 0, "negative costs clamp");
    episodic
        .create_segment("seg-1", "session-1", "global", turn_id, 10.0, 10.0)
        .await
        .expect("create segment");
    episodic
        .append_turn("seg-1", turn_id, 10.0, 11.0)
        .await
        .expect("append turn");
    let segment = episodic
        .open_segment("session-1")
        .await
        .expect("open segment")
        .expect("segment present");
    assert_eq!(segment.turn_count, 2);
    episodic
        .close_segment("seg-1", 13.0)
        .await
        .expect("close segment");
    episodic
        .set_segment_summary("seg-1", "one remembered turn", 14.0)
        .await
        .expect("set summary");
    assert!(episodic
        .open_segment("session-1")
        .await
        .expect("open segment after close")
        .is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn ingest_chunk_and_retrieval_validation_paths_are_exercised_without_network() {
    use tinymemory_api::chunks::DataSource;
    use tinymemory_api::error::MemoryError;
    use tinymemory_api::provider::types::IngestItem;
    use tinymemory_api::provider::{ChunkQuery, FastRetrieveQuery, MemoryProvider};
    use tinymemory_api::types::MemoryTaint;

    let workspace = tempfile::tempdir().expect("workspace");
    let provider = provider_over(workspace.path());
    let ingest = provider.as_ingest().expect("Ingest");
    let invalid = IngestItem {
        namespace: None,
        source: DataSource::Upload,
        source_id: "upload-1".into(),
        owner: "owner".into(),
        source_ref: None,
        content: "binary".into(),
        mime: Some("application/pdf".into()),
        timestamp: None,
        tags: Vec::new(),
        taint: MemoryTaint::Internal,
        path_scope: None,
    };
    assert!(matches!(
        ingest.ingest_document(invalid).await,
        Err(MemoryError::Invalid(_))
    ));
    assert!(ingest
        .ingest_chat(Vec::new())
        .await
        .expect("empty chat")
        .ids
        .is_empty());

    let chunks = provider.as_chunks().expect("Chunks");
    assert!(chunks
        .list_chunks(&ChunkQuery::default(), None)
        .await
        .expect("empty chunk list")
        .is_empty());
    assert!(chunks
        .get_chunk("missing")
        .await
        .expect("missing chunk")
        .is_none());
    assert!(!chunks
        .storage_kinds()
        .await
        .expect("storage kinds")
        .is_empty());

    let retrieval = provider.as_retrieval().expect("Retrieval");
    assert!(matches!(
        retrieval
            .fast_retrieve(
                "   ",
                FastRetrieveQuery {
                    limit: 5,
                    max_hops: 1,
                    time_window_days: None,
                },
                None,
            )
            .await,
        Err(MemoryError::Invalid(_))
    ));
    assert!(matches!(
        retrieval
            .search_entities("x", Some(&["not-a-kind".to_string()]), 5)
            .await,
        Err(MemoryError::Invalid(_))
    ));
}
