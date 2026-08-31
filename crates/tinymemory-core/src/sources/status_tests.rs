//! Tests for the surrounding module.

use super::*;

/// A folder source, the shape the prefix and status tests both start from.
fn folder_entry(id: &str) -> MemorySourceEntry {
    MemorySourceEntry {
        id: id.into(),
        kind: SourceKind::Folder,
        label: "x".into(),
        enabled: true,
        toolkit: None,
        connection_id: None,
        path: Some("/tmp".into()),
        glob: None,
        url: None,
        branch: None,
        paths: Vec::new(),
        query: None,
        since_days: None,
        max_items: None,
        max_commits: None,
        max_issues: None,
        max_prs: None,
        selector: None,
        max_tokens_per_sync: None,
        max_cost_per_sync_usd: None,
        sync_depth_days: None,
    }
}

#[test]
fn source_id_prefix_dispatch() {
    let mut entry = folder_entry("src_abc");
    assert_eq!(source_id_prefix(&entry), "mem_src:src_abc:%");

    // A Composio source is matched on its connection, not just its
    // toolkit: a second Gmail account must not count the first's chunks.
    entry.kind = SourceKind::Composio;
    entry.toolkit = Some("gmail".into());
    entry.connection_id = Some("conn-1".into());
    assert_eq!(source_id_prefix(&entry), "gmail:conn-1:%");

    entry.connection_id = None;
    assert_eq!(source_id_prefix(&entry), "gmail:%");

    entry.toolkit = None;
    assert_eq!(source_id_prefix(&entry), "__no_toolkit__:%");
}

/// A chunk under `source_id`, with a deterministic id the test can address.
fn chunk(id: &str, source_id: &str) -> crate::store::chunks::types::Chunk {
    use crate::store::chunks::types::{Chunk, Metadata, SourceKind as ChunkSourceKind};

    let at = chrono::Utc::now();
    Chunk {
        id: id.into(),
        content: "content".into(),
        metadata: Metadata::point_in_time(ChunkSourceKind::Document, source_id, "owner", at),
        token_count: 1,
        seq_in_source: 0,
        created_at: at,
        partial_message: false,
    }
}

/// The status query counted pending as `embedding IS NULL` over
/// `mem_tree_chunks`. That column is a legacy migration artefact nothing
/// writes, so every chunk read as pending and a healthy source reported
/// `chunks_pending == chunks_synced` forever.
///
/// Pending is "not resolved", and a chunk resolves by carrying an
/// embedding, by being dropped, or by being recorded as skipped for
/// re-embedding. Only the first of these four is genuinely still in
/// flight.
#[tokio::test]
async fn pending_counts_unresolved_chunks_not_the_dead_embedding_column() {
    use tinymemory_api::host::test_support::TestHostConfig;
    use tinymemory_api::host::MemoryHostConfig;

    crate::test_seams::init();
    let workspace = tempfile::tempdir().expect("workspace");
    let mut host = TestHostConfig::default();
    host.workspace_dir = workspace.path().join("workspace");
    let config = host.to_arc();

    let source = folder_entry("src_status");
    let chunks = [
        chunk("chunk-embedded", "mem_src:src_status:item-1"),
        chunk("chunk-pending", "mem_src:src_status:item-2"),
        chunk("chunk-dropped", "mem_src:src_status:item-3"),
        chunk("chunk-skipped", "mem_src:src_status:item-4"),
    ];
    crate::store::chunks::store::upsert_chunks(&*config, &chunks).expect("upsert chunks");

    crate::store::chunks::store::set_chunk_embedding(&*config, "chunk-embedded", &[0.1, 0.2])
        .expect("set embedding");
    crate::store::chunks::store::set_chunk_lifecycle_status(
        &*config,
        "chunk-dropped",
        crate::store::chunks::store::CHUNK_STATUS_DROPPED,
    )
    .expect("set lifecycle status");
    crate::store::chunks::store::mark_chunk_reembed_skipped(
        &*config,
        "chunk-skipped",
        "test-signature",
        "too long",
    )
    .expect("mark reembed skipped");

    // Guard against a vacuous test: the legacy column must still be NULL
    // for every row, so a pending count of 1 is attributable to the new
    // predicate rather than to the old one happening to agree.
    let legacy_nulls: i64 = crate::store::chunks::store::with_connection(&*config, |conn| {
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM mem_tree_chunks \
             WHERE embedding IS NULL AND source_id LIKE 'mem_src:src_status:%'",
            [],
            |row| row.get(0),
        )?)
    })
    .expect("count legacy nulls");
    assert_eq!(
        legacy_nulls, 4,
        "nothing writes the legacy column, so counting it would report all four pending"
    );

    let status = source_status(&*config, &source)
        .await
        .expect("source status");
    assert_eq!(status.chunks_synced, 4);
    assert_eq!(
        status.chunks_pending, 1,
        "only the chunk with no embedding, no drop and no skip is still in flight"
    );
    assert!(status.last_chunk_at_ms.is_some());
}

/// A source with no chunks reports zeroes rather than failing on the
/// `NULL` a `SUM` over no rows produces.
#[tokio::test]
async fn a_source_with_no_chunks_reports_zeroes() {
    use tinymemory_api::host::test_support::TestHostConfig;
    use tinymemory_api::host::MemoryHostConfig;

    crate::test_seams::init();
    let workspace = tempfile::tempdir().expect("workspace");
    let mut host = TestHostConfig::default();
    host.workspace_dir = workspace.path().join("workspace");
    let config = host.to_arc();

    let status = source_status(&*config, &folder_entry("src_empty"))
        .await
        .expect("source status");
    assert_eq!(status.chunks_synced, 0);
    assert_eq!(status.chunks_pending, 0);
    assert_eq!(status.last_chunk_at_ms, None);
    assert_eq!(status.freshness, FreshnessLabel::Idle);
}

/// A pattern that matches nothing still gets a row.
///
/// This is the difference between the counting surface and a `GROUP BY` over
/// the chunk table: a group with no rows is *absent*, so a caller building a
/// dashboard from groups loses a never-synced source instead of showing it
/// idle. The batch answers per pattern, in order, so the caller can pair rows
/// with the sources it asked about.
#[tokio::test]
async fn the_batch_answers_one_row_per_pattern_including_the_empty_ones() {
    use tinymemory_api::host::test_support::TestHostConfig;
    use tinymemory_api::host::MemoryHostConfig;

    crate::test_seams::init();
    let workspace = tempfile::tempdir().expect("workspace");
    let mut host = TestHostConfig::default();
    host.workspace_dir = workspace.path().join("workspace");
    let config = host.to_arc();

    crate::store::chunks::store::upsert_chunks(
        &*config,
        &[chunk("chunk-batch-1", "mem_src:src_batch:item-1")],
    )
    .expect("upsert chunks");

    let patterns = vec![
        "mem_src:src_batch:%".to_string(),
        "mem_src:src_never_synced:%".to_string(),
        "mem_src:src_batch:%".to_string(),
    ];
    let counts = ingest_counts_for_patterns(&*config, &patterns).expect("counts");

    assert_eq!(
        counts.len(),
        patterns.len(),
        "one row per pattern, in order"
    );
    assert_eq!(counts[0].chunks_synced, 1);
    assert_eq!(
        counts[1],
        IngestCounts::default(),
        "a source that has never synced reports zeroes, not an absent row"
    );
    assert_eq!(counts[1].last_chunk_at_ms, None);
    assert_eq!(
        counts[2].chunks_synced, 1,
        "the batch does not consume rows"
    );
}

/// An empty ask is answered without opening the store.
#[test]
fn an_empty_batch_touches_nothing() {
    use tinymemory_api::host::test_support::TestHostConfig;
    use tinymemory_api::host::MemoryHostConfig;

    // Deliberately a workspace that was never created: if the empty case
    // reached the store this would fail rather than return an empty vector.
    let mut host = TestHostConfig::default();
    host.workspace_dir = std::path::PathBuf::from("/nonexistent/tinymemory/status/batch");
    let config = host.to_arc();

    let counts = ingest_counts_for_patterns(&*config, &[]).expect("empty batch");
    assert!(counts.is_empty());
}

/// `_` and `%` in a pattern are the caller's to escape.
///
/// The `ESCAPE` clause is what lets the memory contract's
/// `source_ingest_status` honour its own promise that a chunk-id prefix is
/// matched literally — without it, a source keyed `src_a` also counts the
/// chunks of any source whose id differs only where the underscore is.
/// [`source_id_prefix`] deliberately does not escape, so this pins the clause
/// rather than the existing caller's use of it.
#[tokio::test]
async fn an_escaped_wildcard_matches_itself() {
    use tinymemory_api::host::test_support::TestHostConfig;
    use tinymemory_api::host::MemoryHostConfig;

    crate::test_seams::init();
    let workspace = tempfile::tempdir().expect("workspace");
    let mut host = TestHostConfig::default();
    host.workspace_dir = workspace.path().join("workspace");
    let config = host.to_arc();

    crate::store::chunks::store::upsert_chunks(
        &*config,
        &[
            chunk("chunk-underscore", "mem_src:src_a:item-1"),
            chunk("chunk-collider", "mem_src:srcXa:item-1"),
        ],
    )
    .expect("upsert chunks");

    let unescaped = ingest_counts_for_patterns(&*config, &["mem_src:src_a:%".to_string()])
        .expect("unescaped counts");
    assert_eq!(
        unescaped[0].chunks_synced, 2,
        "an unescaped `_` is a single-character wildcard, which is why the contract escapes"
    );

    let escaped = ingest_counts_for_patterns(&*config, &[r"mem_src:src\_a:%".to_string()])
        .expect("escaped counts");
    assert_eq!(
        escaped[0].chunks_synced, 1,
        "an escaped `_` matches only itself"
    );
}
