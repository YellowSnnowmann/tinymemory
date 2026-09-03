use std::sync::Arc;

use tinymemory_api::host::test_support::TestHostConfig;
use tinymemory_api::host::MemoryHostConfig;

use crate::sources::MemorySourceEntry;
use crate::store::{MemoryClient, MemoryClientRef};

fn composio_source(id: &str, toolkit: &str, connection_id: &str) -> MemorySourceEntry {
    serde_json::from_value(serde_json::json!({
        "id": id,
        "kind": "composio",
        "label": "Test connector",
        "enabled": true,
        "toolkit": toolkit,
        "connection_id": connection_id,
    }))
    .expect("a valid composio source entry")
}

/// A workspace with a registry, a store client, and the stub seams installed.
///
/// Opening the client starts the ingestion queue, so every caller has to be a
/// `#[tokio::test]` even when the body itself does no awaiting.
fn workspace(
    sources: &[MemorySourceEntry],
) -> (tempfile::TempDir, Arc<crate::Config>, MemoryClientRef) {
    crate::test_seams::init();
    let dir = tempfile::tempdir().expect("workspace");
    let workspace_dir = dir.path().join("workspace");
    let mut host = TestHostConfig::default();
    host.workspace_dir = workspace_dir.clone();
    // The source registry is written beside the host's config file, so the
    // default's empty `config_path` has to be given a real one here.
    host.config_path = dir.path().join("config.toml");
    let config = host.to_arc();
    crate::sources::registry::replace_sources_in(&*config, sources).expect("write the registry");
    let client: MemoryClientRef = Arc::new(
        MemoryClient::from_workspace_dir(workspace_dir).expect("memory client initialises"),
    );
    (dir, config, client)
}

async fn store_document(client: &MemoryClientRef, namespace: &str, key: &str, body: &str) {
    client
        .put_doc(tinymemory_api::types::NamespaceDocumentInput {
            namespace: namespace.to_string(),
            key: key.to_string(),
            title: "Quarterly planning".into(),
            content: body.to_string(),
            source_type: "composio".into(),
            priority: "medium".into(),
            tags: vec!["gmail".into()],
            metadata: serde_json::json!({}),
            category: "core".into(),
            session_id: None,
            document_id: None,
            taint: tinymemory_api::types::MemoryTaint::ExternalSync,
        })
        .await
        .expect("store a connector document");
}

/// The namespaces are rebuilt from the registry the writers used, so a
/// connected toolkit yields both its current namespace and — because it has
/// exactly one connection — its pre-migration `skill-` one.
#[tokio::test]
async fn targets_are_rebuilt_from_the_registry_including_the_legacy_namespace() {
    let (_dir, config, _client) = workspace(&[composio_source("src_gmail", "gmail", "conn-1")]);
    let mut report = super::BackfillReport::default();
    let targets = super::resolve_targets(&*config, &mut report).expect("resolve targets");

    let namespaces: Vec<&str> = targets.iter().map(|t| t.namespace.as_str()).collect();
    assert!(
        namespaces.contains(&"source:gmail:conn-1"),
        "the current namespace must be swept: {namespaces:?}"
    );
    assert!(
        namespaces.contains(&"skill-gmail"),
        "one connection means the legacy namespace is unambiguous: {namespaces:?}"
    );
    assert!(
        report.notes.is_empty(),
        "nothing was skipped, so nothing should be reported: {:?}",
        report.notes
    );
}

/// Two accounts on one toolkit make the legacy documents unattributable, and a
/// wrong attribution in a memory system is worse than a missing one. The
/// current namespaces are still swept — only the shared `skill-` one is not.
#[tokio::test]
async fn the_legacy_namespace_is_skipped_when_a_toolkit_has_several_connections() {
    let (_dir, config, _client) = workspace(&[
        composio_source("src_a", "gmail", "conn-1"),
        composio_source("src_b", "gmail", "conn-2"),
    ]);
    let mut report = super::BackfillReport::default();
    let targets = super::resolve_targets(&*config, &mut report).expect("resolve targets");

    let namespaces: Vec<&str> = targets.iter().map(|t| t.namespace.as_str()).collect();
    assert!(
        !namespaces.contains(&"skill-gmail"),
        "an ambiguous legacy namespace must not be guessed at: {namespaces:?}"
    );
    assert!(
        namespaces.contains(&"source:gmail:conn-1") && namespaces.contains(&"source:gmail:conn-2"),
        "both current namespaces stay addressable: {namespaces:?}"
    );
    assert!(
        report.notes.iter().any(|n| n.contains("skill-gmail")),
        "the skip must say which namespace and why: {:?}",
        report.notes
    );
}

/// The point of the whole issue: a document stored before the routing fix
/// reaches the memory tree, under the identity the sync path would have given
/// it — and a second pass writes nothing, because the ingest gate recognises it.
#[tokio::test]
async fn a_stored_document_is_filed_into_the_tree_and_never_twice() {
    let (_dir, config, client) = workspace(&[composio_source("src_gmail", "gmail", "conn-1")]);
    store_document(
        &client,
        "source:gmail:conn-1",
        "msg-1",
        "Let's finalise the Q3 roadmap and align on the launch date.",
    )
    .await;

    assert_eq!(
        crate::store::chunks::store::count_chunks(&*config).expect("count chunks"),
        0,
        "the document store alone must leave the tree empty — that IS openhuman#6007"
    );

    let first = super::backfill_connector_trees(&*config, &client, None, false)
        .await
        .expect("backfill");
    assert_eq!(
        first.ingested, 1,
        "the stored document must be treed: {first:?}"
    );
    assert_eq!(
        first.already_present, 0,
        "nothing was there before: {first:?}"
    );

    // The identity has to match what the sync path writes, because that is the
    // prefix OpenHuman counts a Composio source's ingest by.
    let treed = crate::store::chunks::store::list_chunks(
        &*config,
        &tinycortex::memory::chunks::ListChunksQuery {
            source_id: Some("gmail:conn-1:msg-1".into()),
            limit: Some(8),
            ..Default::default()
        },
    )
    .expect("list chunks by source id");
    assert!(
        !treed.is_empty(),
        "backfilled rows must carry the per-item connector source id"
    );
    assert!(
        treed
            .iter()
            .all(|chunk| chunk.metadata.path_scope.as_deref() == Some("gmail:conn-1")),
        "backfilled rows must carry the platform-prefixed path_scope, or retrieval never \
         resolves them"
    );

    let chunks_after_first =
        crate::store::chunks::store::count_chunks(&*config).expect("count chunks");

    // Idempotence is the property that makes this safe to re-run and safe to
    // interrupt: no watermark, just an ingest gate that recognises its own work.
    let second = super::backfill_connector_trees(&*config, &client, None, false)
        .await
        .expect("second backfill");
    assert_eq!(
        second.ingested, 0,
        "a second pass must write nothing: {second:?}"
    );
    assert_eq!(
        second.already_present, 1,
        "and must say why it wrote nothing: {second:?}"
    );
    assert_eq!(
        crate::store::chunks::store::count_chunks(&*config).expect("count chunks"),
        chunks_after_first,
        "a repeated pass must not duplicate chunks"
    );
}

/// The preview an operator gets before paying for a full pass: it counts what
/// it would examine and writes nothing at all.
#[tokio::test]
async fn a_dry_run_counts_without_writing() {
    let (_dir, config, client) = workspace(&[composio_source("src_gmail", "gmail", "conn-1")]);
    store_document(&client, "source:gmail:conn-1", "msg-1", "Q3 roadmap.").await;

    let report = super::backfill_connector_trees(&*config, &client, None, true)
        .await
        .expect("dry run");

    assert_eq!(
        report.scanned, 1,
        "a dry run still reports the size of the job"
    );
    assert_eq!(report.ingested, 0, "a dry run must not ingest");
    assert_eq!(
        crate::store::chunks::store::count_chunks(&*config).expect("count chunks"),
        0,
        "a dry run must leave the tree untouched"
    );
}

/// `limit` bounds cost, and says so rather than looking like a finished pass.
#[tokio::test]
async fn a_bounded_pass_reports_that_more_is_pending() {
    let (_dir, config, client) = workspace(&[composio_source("src_gmail", "gmail", "conn-1")]);
    for key in ["msg-1", "msg-2", "msg-3"] {
        store_document(&client, "source:gmail:conn-1", key, "Q3 roadmap.").await;
    }

    let report = super::backfill_connector_trees(&*config, &client, Some(2), true)
        .await
        .expect("bounded dry run");

    assert_eq!(report.scanned, 2, "the limit is respected: {report:?}");
    assert!(
        report.more_pending,
        "a pass that stopped on its limit must not read as complete: {report:?}"
    );
}
