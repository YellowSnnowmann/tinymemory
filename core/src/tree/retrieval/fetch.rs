use anyhow::Result;

use crate::source_scope::chunk_source_allowed_in;
use crate::source_scope::current_source_scope;
use crate::store::chunks::store::get_chunks_batch;
use crate::tinycortex::engine_config;
use crate::tree::retrieval::types::RetrievalHit;
use crate::Config;

pub use tinycortex::memory::retrieval::MAX_BATCH;

/// Fetch leaf chunks by id, using the **ambient** scope.
///
/// Correct in-process; see [`fetch_leaves_scoped`] for the transport-facing
/// path and why it cannot use this one.
pub async fn fetch_leaves(config: &Config, chunk_ids: &[String]) -> Result<Vec<RetrievalHit>> {
    fetch_leaves_scoped(config, chunk_ids, current_source_scope()).await
}

/// Fetch leaf chunks by id, using an **explicitly supplied** scope.
///
/// Exists for the same reason as
/// [`fast_retrieve_scoped`](super::fast::fast_retrieve_scoped): the task-local
/// scope belongs to the host's task and does not cross a transport, so a bus
/// caller reading it would find it absent — and absent means unrestricted,
/// which is a source gate failing open.
pub async fn fetch_leaves_scoped(
    config: &Config,
    chunk_ids: &[String],
    scope: Option<std::collections::HashSet<String>>,
) -> Result<Vec<RetrievalHit>> {
    log::debug!(
        "[retrieval::fetch] tinycortex requested={}",
        chunk_ids.len()
    );
    let permitted_ids = if let Some(set) = scope {
        let chunks = get_chunks_batch(config, chunk_ids)?;
        chunk_ids
            .iter()
            .filter(|id| {
                chunks.get(*id).is_some_and(|chunk| {
                    chunk_source_allowed_in(&set, &chunk.metadata.tags, &chunk.metadata.source_id)
                })
            })
            .cloned()
            .collect::<Vec<_>>()
    } else {
        chunk_ids.to_vec()
    };
    tinycortex::memory::retrieval::fetch_leaves(&engine_config(config), &permitted_ids)
}
