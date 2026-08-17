//! Product adapters for tinycortex-owned deterministic fast retrieval.

use anyhow::Result;

use crate::source_scope::current_source_scope;
use crate::tinycortex::engine_config;
use crate::tree::nlp;
use crate::tree::retrieval::engine::EmbedderBridge;
use crate::tree::retrieval::types::QueryResponse;
use crate::tree::score::embed::build_embedder_from_config;
use crate::Config;

pub use tinycortex::memory::retrieval::FastRetrieveOptions;

/// Deterministic graph-walk retrieval using the **ambient** source scope.
///
/// Correct in-process; see [`fast_retrieve_scoped`] for the transport-facing
/// path and why it cannot use this one.
pub async fn fast_retrieve(
    config: &Config,
    query: &str,
    options: FastRetrieveOptions,
) -> Result<QueryResponse> {
    fast_retrieve_scoped(config, query, options, current_source_scope()).await
}

/// Deterministic graph-walk retrieval using an **explicitly supplied** scope.
///
/// Exists for the same reason as
/// [`cover_window_scoped`](super::cover::cover_window_scoped): a task-local
/// source scope does not cross a transport, and reading it as absent means
/// unrestricted — a source gate failing open.
pub async fn fast_retrieve_scoped(
    config: &Config,
    query: &str,
    options: FastRetrieveOptions,
    scope: Option<std::collections::HashSet<String>>,
) -> Result<QueryResponse> {
    let query_entities = nlp::extract_query_entities(config, query).await;
    let entity_ids: Vec<_> = query_entities
        .into_iter()
        .map(|entity| entity.canonical_id)
        .collect();
    log::debug!(
        "[retrieval::fast] tinycortex query_len={} entities={} limit={} hops={}",
        query.len(),
        entity_ids.len(),
        options.limit,
        options.max_hops
    );
    let embedder = build_embedder_from_config(config)?;
    tinycortex::memory::retrieval::fast_retrieve(
        &engine_config(config),
        query,
        &entity_ids,
        &EmbedderBridge(embedder.as_ref()),
        scope.as_ref(),
        options,
    )
    .await
}
