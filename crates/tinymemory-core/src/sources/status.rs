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

use anyhow::Result;
use rusqlite::Connection;
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

/// What one source's chunks amount to, before a freshness label is put on them.
///
/// The counting half of a [`SourceStatus`], split out because two callers need
/// exactly it and nothing else: this module's own [`source_status`], and the
/// memory contract's `MemoryChunks::source_ingest_status`, which answers for a
/// registry it does not own and therefore cannot name a source's freshness
/// vocabulary either.
///
/// Freshness is deliberately not here. It is arithmetic over
/// [`Self::last_chunk_at_ms`] and a clock, and folding it in would make every
/// caller inherit the moment the count was taken as the moment the label was
/// computed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IngestCounts {
    /// Chunk rows the store holds under the prefix, whatever their state.
    pub chunks_synced: u64,
    /// Rows still in flight: no embedding, not dropped, not skipped.
    pub chunks_pending: u64,
    /// Source time of the newest row under the prefix, epoch milliseconds.
    pub last_chunk_at_ms: Option<i64>,
}

/// The one definition of what a source's chunks count as.
///
/// "Pending" is **not resolved**, and a chunk resolves three ways: it has an
/// embedding, it was dropped by the lifecycle, or it was deliberately recorded
/// as skipped for re-embedding. All three are terminal. This is the engine's
/// own predicate from `list_sync_statuses`, kept identical so the per-source
/// view and the per-provider one cannot disagree about the same chunk — and
/// written once here so the two callers of it cannot drift apart the way the
/// second copy of [`source_id_prefix`] did.
///
/// `?1` is a `LIKE` pattern, not a literal prefix: the wildcard is the
/// caller's to place, and `ESCAPE` is declared so a caller that means a `%` or
/// a `_` literally can say so. [`source_id_prefix`] deliberately does not
/// escape — its ids are generated and its patterns are the ones this module
/// has always sent.
const INGEST_COUNTS_SQL: &str = "SELECT \
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
     WHERE c.source_id LIKE ?1 ESCAPE '\\'";

/// Count the chunks one `source_id LIKE` pattern selects, on an open
/// connection.
///
/// Always yields a row. The query is a bare aggregate with no `GROUP BY`, so a
/// pattern matching nothing returns one row of `(0, NULL, NULL)` rather than no
/// rows — which is what lets a caller ask about a source that has never synced
/// and get zeroes instead of an absence.
fn ingest_counts_on_connection(conn: &Connection, pattern: &str) -> Result<IngestCounts> {
    let (synced, pending, last_ts): (i64, i64, Option<i64>) =
        conn.query_row(INGEST_COUNTS_SQL, [pattern], |r| {
            Ok((
                r.get(0)?,
                // `SUM` over no rows is `NULL`, not `0`.
                r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                r.get(2)?,
            ))
        })?;

    Ok(IngestCounts {
        chunks_synced: synced.max(0) as u64,
        chunks_pending: pending.max(0) as u64,
        last_chunk_at_ms: last_ts,
    })
}

/// Counts for several `source_id LIKE` patterns, in the order asked.
///
/// One connection for the batch and one statement per pattern. Synchronous
/// SQLite work: an async caller runs it on a blocking thread, as
/// [`source_status`] does.
///
/// Surfaces real query errors rather than degrading, so status telemetry cannot
/// report a healthy zero-row state over a database that is actually broken. An
/// empty `patterns` opens no connection at all.
///
/// # Errors
///
/// Any failure opening the chunk store or running the count.
pub fn ingest_counts_for_patterns(
    config: &Config,
    patterns: &[String],
) -> Result<Vec<IngestCounts>> {
    if patterns.is_empty() {
        return Ok(Vec::new());
    }
    with_connection(config, |conn| {
        patterns
            .iter()
            .map(|pattern| ingest_counts_on_connection(conn, pattern))
            .collect()
    })
}

/// Compute status for one source.
pub async fn source_status(
    config: &Config,
    source: &MemorySourceEntry,
) -> Result<SourceStatus, String> {
    let cfg = config.to_arc();
    let source_id = source.id.clone();
    let pattern = source_id_prefix(source);

    tokio::task::spawn_blocking(move || {
        let counts = ingest_counts_for_patterns(&*cfg, std::slice::from_ref(&pattern))
            .map_err(|e| format!("source_status: {e}"))?;
        // One pattern in, one row out — the batch always answers per pattern.
        let counts = counts.first().copied().unwrap_or_default();

        let now_ms = chrono::Utc::now().timestamp_millis();
        Ok(SourceStatus {
            source_id,
            chunks_synced: counts.chunks_synced,
            chunks_pending: counts.chunks_pending,
            last_chunk_at_ms: counts.last_chunk_at_ms,
            freshness: FreshnessLabel::from_age_ms(counts.last_chunk_at_ms, now_ms),
        })
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
#[path = "status_tests.rs"]
mod tests;
