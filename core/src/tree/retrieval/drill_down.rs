use anyhow::Result;

use crate::source_scope::current_source_scope;
use crate::tinycortex::engine_config;
use crate::tree::retrieval::engine::EmbedderBridge;
use crate::tree::retrieval::types::RetrievalHit;
use crate::tree::score::embed::{build_embedder_from_config, InertEmbedder};
use crate::Config;

/// Walk a summary tree from `node_id`, using the **ambient** scope.
///
/// Correct in-process; see [`drill_down_scoped`] for the transport-facing path
/// and why it cannot use this one.
pub async fn drill_down(
    config: &Config,
    node_id: &str,
    max_depth: u32,
    query: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<RetrievalHit>> {
    drill_down_scoped(
        config,
        node_id,
        max_depth,
        query,
        limit,
        current_source_scope(),
    )
    .await
}

/// Walk a summary tree from `node_id`, using an **explicitly supplied** scope.
///
/// Exists for the same reason as
/// [`fast_retrieve_scoped`](super::fast::fast_retrieve_scoped): a task-local
/// scope does not cross a transport, and reading it as absent means
/// unrestricted — a source gate failing open.
pub async fn drill_down_scoped(
    config: &Config,
    node_id: &str,
    max_depth: u32,
    query: Option<&str>,
    limit: Option<usize>,
    scope: Option<std::collections::HashSet<String>>,
) -> Result<Vec<RetrievalHit>> {
    log::debug!(
        "[retrieval::drill_down] tinycortex max_depth={} has_query={} limit={:?}",
        max_depth,
        query.is_some(),
        limit
    );
    let embedder = if query.is_none() || max_depth == 0 {
        log::debug!("[retrieval::drill_down] using inert embedder for non-semantic traversal");
        Box::new(InertEmbedder::new()) as Box<dyn crate::tree::score::embed::Embedder>
    } else {
        build_embedder_from_config(config)?
    };
    let bridge = EmbedderBridge(embedder.as_ref());
    let engine_limit = current_source_scope()
        .as_ref()
        .map(|_| None)
        .unwrap_or(limit);
    let mut hits = tinycortex::memory::retrieval::drill_down(
        &engine_config(config),
        node_id,
        max_depth,
        query,
        &bridge,
        engine_limit,
    )
    .await?;
    if let Some(set) = current_source_scope() {
        hits.retain(|hit| set.contains(&hit.tree_scope));
    }
    if let Some(limit) = limit {
        hits.truncate(limit);
    }
    Ok(hits)
}
