//! Per-source sync status — chunks ingested, freshness, in-flight progress.
//!
//! Queries `mem_tree_chunks` filtered by source-id prefix:
//! - Reader-backed kinds (folder/github/rss/web/twitter) tag chunks
//!   with `mem_src:{source.id}:%`, so we count those directly.
//! - Composio sources tag chunks with the connector id
//!   (`{toolkit}:{connection_id}:{document_id}`), so we match by that
//!   prefix instead.
//!
//! # Where "pending" lives
//!
//! Not on `mem_tree_chunks`. That table carries a legacy `embedding` column,
//! added by an idempotent migration and written by nothing — counting
//! `embedding IS NULL` reports every chunk as pending forever, so a healthy
//! source shows `chunks_pending == chunks_synced` and the memory-sources UI
//! shows eternal work in flight.
//!
//! Embeddings live in the `mem_tree_chunk_embeddings` sidecar, one row per
//! `(chunk, model signature)`. A chunk with no row there is still not
//! necessarily pending: the lifecycle may have dropped it, or it may be
//! recorded in `mem_tree_chunk_reembed_skipped`. Both are terminal, and both
//! count as resolved.

use serde::Serialize;

use crate::sources::types::{MemorySourceEntry, SourceKind};
use crate::store::chunks::store::with_connection;
use crate::Config;

/// Freshness is one vocabulary, owned by [`crate::sync::sync_status`].
///
/// It was declared a second time here, with the same variants, the same
/// snake_case wire strings and the same thresholds — two definitions that had
/// to be kept in step by hand and nothing checking that they were. Re-exported
/// rather than merely imported, so `sources::status::FreshnessLabel` stays a
/// working path for callers that already name it.
pub use crate::sync::sync_status::FreshnessLabel;

#[derive(Clone, Debug, Serialize)]
pub struct SourceStatus {
    pub source_id: String,
    pub chunks_synced: u64,
    pub chunks_pending: u64,
    pub last_chunk_at_ms: Option<i64>,
    pub freshness: FreshnessLabel,
}

/// Compute status for one source.
pub async fn source_status(
    config: &Config,
    source: &MemorySourceEntry,
) -> Result<SourceStatus, String> {
    let cfg = config.to_arc();
    let source_clone = source.clone();

    tokio::task::spawn_blocking(move || {
        with_connection(&*cfg, |conn| {
            let prefix = source_id_prefix(&source_clone);

            // Surface real query errors so status telemetry doesn't lie about
            // a healthy zero-row state when the DB is actually broken.
            //
            // "Pending" is "not resolved", and a chunk resolves three ways:
            // it has an embedding, it was dropped by the lifecycle, or it was
            // deliberately skipped for re-embedding. This is the engine's own
            // predicate from `list_sync_statuses`, kept identical so the
            // per-source view and the per-provider one cannot disagree about
            // the same chunk.
            let (synced, pending, last_ts): (i64, i64, Option<i64>) = conn.query_row(
                "SELECT \
                       COUNT(*), \
                       SUM(CASE WHEN EXISTS ( \
                                 SELECT 1 FROM mem_tree_chunk_embeddings e \
                                 WHERE e.chunk_id = c.id) \
                               OR c.lifecycle_status = 'dropped' \
                               OR EXISTS ( \
                                 SELECT 1 FROM mem_tree_chunk_reembed_skipped s \
                                 WHERE s.chunk_id = c.id) \
                             THEN 0 ELSE 1 END), \
                       MAX(c.timestamp_ms) \
                     FROM mem_tree_chunks c \
                     WHERE c.source_id LIKE ?1",
                [&prefix],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                        r.get(2)?,
                    ))
                },
            )?;

            let now_ms = chrono::Utc::now().timestamp_millis();
            Ok(SourceStatus {
                source_id: source_clone.id.clone(),
                chunks_synced: synced.max(0) as u64,
                chunks_pending: pending.max(0) as u64,
                last_chunk_at_ms: last_ts,
                freshness: FreshnessLabel::from_age_ms(last_ts, now_ms),
            })
        })
        .map_err(|e| format!("source_status: {e}"))
    })
    .await
    .map_err(|e| format!("source_status join: {e}"))?
}

/// Compute status for all configured sources (one SQL roundtrip per source).
pub async fn status_list(config: &Config) -> Result<Vec<SourceStatus>, String> {
    let sources = crate::sources::registry::list_sources().await?;
    let mut out = Vec::with_capacity(sources.len());
    for source in sources {
        match source_status(config, &source).await {
            Ok(s) => out.push(s),
            Err(e) => {
                tracing::warn!(
                    source_id = %source.id,
                    error = %e,
                    "[memory_sources:status] query failed"
                );
                out.push(SourceStatus {
                    source_id: source.id,
                    chunks_synced: 0,
                    chunks_pending: 0,
                    last_chunk_at_ms: None,
                    freshness: FreshnessLabel::Idle,
                });
            }
        }
    }
    Ok(out)
}

/// Build the `source_id LIKE` prefix that matches chunks belonging to a source.
///
/// The scheme is set by the ingest paths, not chosen here: reader-backed kinds
/// key chunks `mem_src:{source.id}:{item}`, and the Composio sync keys them
/// `{toolkit}:{connection_id}:{document_id}`.
///
/// Matching a Composio source on its toolkit alone would sweep in every *other*
/// connection of that toolkit — two Gmail accounts would each report the
/// other's chunks as their own — so the connection narrows it. A Composio entry
/// without a connection id does not pass validation; the toolkit-only fallback
/// is there so a malformed row degrades to a wide match rather than to no
/// match at all.
///
/// Shared with [`crate::diff::source`], which builds its snapshot item source
/// from the same prefixes. It held a second copy of this function whose comment
/// said it mirrored this one, which is a mirror only for as long as someone
/// remembers it is.
pub(crate) fn source_id_prefix(source: &MemorySourceEntry) -> String {
    match source.kind {
        SourceKind::Composio => {
            match (source.toolkit.as_deref(), source.connection_id.as_deref()) {
                (Some(toolkit), Some(connection_id)) => format!("{toolkit}:{connection_id}:%"),
                (Some(toolkit), None) => format!("{toolkit}:%"),
                (None, _) => "__no_toolkit__:%".to_string(),
            }
        }
        _ => format!("mem_src:{}:%", source.id),
    }
}

#[cfg(test)]
mod tests {
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
}
