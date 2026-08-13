use anyhow::Result;

use crate::source_scope::current_source_scope;
use crate::store::chunks::types::SourceKind;
use crate::tinycortex::engine_config;
use crate::tree::retrieval::types::QueryResponse;
use crate::Config;

const DEFAULT_LIMIT: usize = 200;

/// Cover a window using the **ambient** source scope.
///
/// Correct for an in-process caller, which shares this task-local. A caller
/// reached over a transport does not — see [`cover_window_scoped`].
pub async fn cover_window(
    config: &Config,
    since_ms: i64,
    until_ms: i64,
    source_id: Option<&str>,
    source_kind: Option<SourceKind>,
    limit: usize,
) -> Result<QueryResponse> {
    cover_window_scoped(
        config,
        since_ms,
        until_ms,
        source_id,
        source_kind,
        limit,
        current_source_scope(),
    )
    .await
}

/// Cover a window using an **explicitly supplied** source scope.
///
/// # Why this exists separately
///
/// [`cover_window`] reads the source scope from a task-local, which is
/// invisible to a caller in another process — or, in the module's case, on the
/// other side of a bus call within this one. The scope would silently read as
/// absent there, and "absent" means *unrestricted*, so a per-profile source gate
/// would quietly stop applying. That is a permission check failing open, so the
/// transport-facing path takes the scope as an argument and never infers it.
#[allow(clippy::too_many_arguments)]
pub async fn cover_window_scoped(
    config: &Config,
    since_ms: i64,
    until_ms: i64,
    source_id: Option<&str>,
    source_kind: Option<SourceKind>,
    limit: usize,
    scope: Option<std::collections::HashSet<String>>,
) -> Result<QueryResponse> {
    let limit = if limit == 0 { DEFAULT_LIMIT } else { limit };
    if source_id.is_some_and(|id| scope.as_ref().is_some_and(|set| !set.contains(id))) {
        return Ok(QueryResponse::empty());
    }
    log::debug!(
        "[retrieval::cover] tinycortex has_source_id={} source_kind={:?} limit={}",
        source_id.is_some(),
        source_kind.map(|k| k.as_str()),
        limit
    );
    let mut response = tinycortex::memory::retrieval::cover_window_scoped(
        &engine_config(config),
        since_ms,
        until_ms,
        source_id,
        source_kind,
        scope,
        usize::MAX,
    )?;
    let total = response.hits.len();
    response.hits.truncate(limit);
    Ok(QueryResponse::new(response.hits, total))
}
