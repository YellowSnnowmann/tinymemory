//! OpenHuman service adapters for tinycortex live synchronization.

use async_trait::async_trait;
use std::sync::Arc;
use tinycortex::memory::sync::{
    ExternalSourceReader, GithubRepoSyncPipeline, LocalDocument, LocalDocumentSink, SkillDocSink,
    SkillDocument, SyncContext, SyncDispatcher, SyncEvent, SyncEventSink, SyncOutcome,
    SyncPipeline, SyncStage, SyncStateStore, WorkspaceSourcePipeline,
};

use crate::sources::{MemorySourceEntry, SourceKind};
use crate::store::MemoryClientRef;
use crate::Config;

/// The KV namespace Composio sync state is persisted under.
///
/// Re-exported from the engine rather than re-declared. It was a second
/// `const` holding the same literal as
/// `tinycortex::memory::sync::state::STATE_NAMESPACE`, so the host and the
/// engine agreed only by coincidence of the string: change either and the two
/// would silently read and write *different* namespaces, stranding every
/// persisted sync cursor with no error anywhere. A duplicated literal is a
/// drift hazard precisely when the thing it names is durable (#18 §B2).
pub use tinycortex::memory::sync::state::STATE_NAMESPACE as HOST_SYNC_STATE_NAMESPACE;
pub use tinycortex::memory::sync::{RawCoverage, RawFileRef, RealCostAccumulator, RebuildOutcome};

pub struct HostSyncAdapter {
    memory: MemoryClientRef,
    config: Option<Arc<Config>>,
}

#[derive(Debug)]
pub struct SourcePipelineFailure {
    pub message: String,
    pub actions_called: u32,
    pub provider_cost_usd: f64,
}

impl std::fmt::Display for SourcePipelineFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl SourcePipelineFailure {
    fn without_usage(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            actions_called: 0,
            provider_cost_usd: 0.0,
        }
    }
}

impl HostSyncAdapter {
    pub fn new(memory: MemoryClientRef) -> Self {
        Self {
            memory,
            config: None,
        }
    }

    fn with_config(memory: MemoryClientRef, config: Arc<Config>) -> Self {
        Self {
            memory,
            config: Some(config),
        }
    }

    /// Reconnect a synced Composio document to the memory tree (#5473).
    ///
    /// The TinyCortex migration (#4794) dropped the per-provider tree-ingest
    /// half of the connector sync: synced items reached the `skill-<toolkit>`
    /// document store but never `mem_tree_chunks`, so connector memories fell
    /// out of tree-backed recall. This routes each synced item through the
    /// engine's document ingest — the same L0-chunk path local folder sources
    /// use via [`LocalDocumentSink`] — additively alongside the skill store.
    ///
    /// Scope naming matches the tree retrieval contract: the tree scope
    /// (`path_scope`) is `"{toolkit}:{connection_id}"` so `query_source` resolves
    /// it by platform prefix (`gmail:` → email, `slack:` → chat, …), while the
    /// per-item `source_id` carries the document id so each message admits
    /// independently rather than colliding on one dedup key.
    ///
    /// `ingest_document` writes the L0 chunk rows synchronously and enqueues the
    /// summary seal on the async extract worker. Retrieval (`query_source`) reads
    /// sealed summaries, so an item becomes retrievable once its buffer seals —
    /// on the token threshold or the time-based `flush_stale_buffers` — and the
    /// seal degrades to a fallback summary when no LLM is available.
    async fn ingest_document_into_memory_tree(
        &self,
        config: &Config,
        document: &SkillDocument,
    ) -> anyhow::Result<()> {
        let toolkit = document.toolkit.trim().to_ascii_lowercase();
        let connection_id = document.connection_id.trim();
        // A blank toolkit/connection would yield a scope with no platform prefix
        // (`":conn"`), which no retrieval kind matches; skip rather than write an
        // unreachable tree. The skill store still holds the item.
        if toolkit.is_empty() || connection_id.is_empty() {
            tracing::debug!(
                document_id = %document.document_id,
                "[tinycortex:sync] skipping memory-tree ingest: item has no toolkit/connection scope"
            );
            return Ok(());
        }
        let tree_scope = format!("{toolkit}:{connection_id}");
        let source_id = format!("{tree_scope}:{}", document.document_id);
        let owner = format!("{toolkit}-sync:{connection_id}");
        let input = tinycortex::memory::ingest::canonicalize::document::DocumentInput {
            provider: format!("composio:{toolkit}"),
            title: document.title.clone(),
            body: document.content.clone(),
            modified_at: chrono::Utc::now(),
            source_ref: Some(document.document_id.clone()),
        };
        crate::ingest_pipeline::ingest_document_with_scope(
            config,
            &source_id,
            &owner,
            vec![toolkit],
            input,
            Some(tree_scope),
        )
        .await
        .map(|_| ())
        .map_err(|error| {
            anyhow::anyhow!("memory-tree ingest failed for source `{source_id}`: {error}")
        })
    }
}

/// Read persisted sync audit records for best-effort RPC and reporting surfaces.
///
/// Backed by `crate::sync::audit` — the host-owned log — not the engine;
/// this stays in the engine module only because OpenHuman reaches it through
/// the engine shim path.
pub fn read_audit_log(config: &Config) -> Vec<crate::sync::audit::SyncAuditEntry> {
    crate::sync::audit::read_audit_log(config.workspace_dir()).unwrap_or_default()
}

/// Estimate sync inference cost using TinyCortex's canonical pricing model.
/// Delegates to the host-owned pricing (#18 §B1); kept because OpenHuman
/// reaches it through the engine shim path.
pub fn estimate_cost_usd(input_tokens: u64, output_tokens: u64) -> f64 {
    crate::sync::audit::estimate_cost_usd(input_tokens, output_tokens)
}

/// Measure coverage of a raw archive by its TinyCortex memory tree.
pub fn raw_coverage(
    config: &Config,
    tree_scope: &str,
    archive_source_id: &str,
) -> anyhow::Result<RawCoverage> {
    tracing::debug!("[tinycortex:sync] raw coverage scan starting");
    let memory_config = super::memory_config_from(config, config.workspace_dir().clone());
    let coverage =
        tinycortex::memory::sync::raw_coverage(&memory_config, tree_scope, archive_source_id)
            .map_err(|error| {
                tracing::warn!(%error, "[tinycortex:sync] raw coverage scan failed");
                error
            })?;
    tracing::debug!(
        total = coverage.total,
        covered = coverage.covered,
        pending = coverage.pending.len(),
        "[tinycortex:sync] raw coverage scan completed"
    );
    Ok(coverage)
}

/// Return whether a raw archive contains records absent from its memory tree.
pub fn needs_rebuild(config: &Config, tree_scope: &str, archive_source_id: &str) -> bool {
    let memory_config = super::memory_config_from(config, config.workspace_dir().clone());
    let required =
        tinycortex::memory::sync::needs_rebuild(&memory_config, tree_scope, archive_source_id);
    tracing::debug!(
        required,
        "[tinycortex:sync] raw rebuild requirement evaluated"
    );
    required
}

/// Rebuild a memory tree from its raw archive through the host summarizer.
pub async fn rebuild_tree_from_raw(
    config: &Config,
    tree_scope: &str,
    archive_source_id: &str,
) -> anyhow::Result<RebuildOutcome> {
    tracing::info!("[tinycortex:sync] raw rebuild starting");
    let memory_config = super::memory_config_from(config, config.workspace_dir().clone());
    let summariser = super::HostSummariser::new(config.to_arc());
    let outcome = tinycortex::memory::sync::rebuild_tree_from_raw(
        &memory_config,
        tree_scope,
        archive_source_id,
        &summariser,
    )
    .await
    .map_err(|error| {
        tracing::warn!(%error, "[tinycortex:sync] raw rebuild failed");
        error
    })?;
    tracing::info!(
        files_read = outcome.files_read,
        batches = outcome.batches,
        "[tinycortex:sync] raw rebuild completed"
    );
    Ok(outcome)
}

/// Run a registered GitHub repository source through TinyCortex synchronization.
pub async fn run_github_sync(
    source: &MemorySourceEntry,
    config: &Config,
) -> anyhow::Result<SyncOutcome> {
    tracing::info!("[tinycortex:sync] GitHub repository sync starting");
    if crate::global::client_if_ready().is_none() {
        tracing::debug!("[tinycortex:sync] GitHub sync initializing memory client");
        crate::global::init(config.workspace_dir().clone())
            .map_err(anyhow::Error::msg)
            .map_err(|error| {
                tracing::warn!(%error, "[tinycortex:sync] GitHub sync memory initialization failed");
                error
            })?;
    }
    let outcome = run_source_pipeline(source, config)
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))
        .map_err(|error| {
            tracing::warn!(%error, "[tinycortex:sync] GitHub repository sync failed");
            error
        })?;
    tracing::info!(
        records_ingested = outcome.records_ingested,
        more_pending = outcome.more_pending,
        actions_called = outcome.actions_called,
        "[tinycortex:sync] GitHub repository sync completed"
    );
    Ok(outcome)
}

#[async_trait]
impl ExternalSourceReader for HostSyncAdapter {
    async fn list_items(
        &self,
        source: &tinycortex::memory::sources::MemorySourceEntry,
    ) -> anyhow::Result<Vec<tinycortex::memory::sources::SourceItem>> {
        let config = self
            .config
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("external source reader requires host config"))?;
        let host_source: MemorySourceEntry = serde_json::from_value(serde_json::to_value(source)?)?;
        let reader = crate::sources::readers::reader_for(&host_source.kind);
        let items = reader
            .list_items(&host_source, &**config)
            .await
            .map_err(anyhow::Error::msg)?;
        serde_json::from_value(serde_json::to_value(items)?).map_err(Into::into)
    }

    async fn read_item(
        &self,
        source: &tinycortex::memory::sources::MemorySourceEntry,
        item_id: &str,
    ) -> anyhow::Result<tinycortex::memory::sources::SourceContent> {
        let config = self
            .config
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("external source reader requires host config"))?;
        let host_source: MemorySourceEntry = serde_json::from_value(serde_json::to_value(source)?)?;
        let reader = crate::sources::readers::reader_for(&host_source.kind);
        let content = reader
            .read_item(&host_source, item_id, &**config)
            .await
            .map_err(anyhow::Error::msg)?;
        serde_json::from_value(serde_json::to_value(content)?).map_err(Into::into)
    }
}

pub fn sync_context(memory: MemoryClientRef) -> SyncContext {
    let adapter = std::sync::Arc::new(HostSyncAdapter::new(memory));
    SyncContext {
        events: adapter.clone(),
        documents: adapter.clone(),
        state: adapter,
        local_documents: None,
        external_sources: None,
        summariser: None,
    }
}

fn source_sync_context(memory: MemoryClientRef, config: &Config, local: bool) -> SyncContext {
    let adapter = std::sync::Arc::new(HostSyncAdapter::with_config(memory, config.to_arc()));
    SyncContext {
        events: adapter.clone(),
        documents: adapter.clone(),
        state: adapter.clone(),
        local_documents: local.then(|| adapter.clone() as std::sync::Arc<dyn LocalDocumentSink>),
        external_sources: local.then_some(adapter as std::sync::Arc<dyn ExternalSourceReader>),
        summariser: local.then(|| {
            std::sync::Arc::new(super::HostSummariser::new(config.to_arc()))
                as std::sync::Arc<dyn tinycortex::memory::tree::Summariser>
        }),
    }
}

pub async fn run_source_pipeline(
    source: &MemorySourceEntry,
    config: &Config,
) -> Result<SyncOutcome, SourcePipelineFailure> {
    // Composio sources run on the engine-free pipelines (#18 §B1); this seam
    // keeps only the tree-coupled kinds (folder/repo/rss/web — they summarise
    // into the engine tree by design) and converts at the boundary for its
    // OpenHuman-facing callers.
    if source.kind == SourceKind::Composio {
        let toolkit = source
            .toolkit
            .as_deref()
            .map(str::trim)
            .filter(|toolkit| !toolkit.is_empty())
            .ok_or_else(|| SourcePipelineFailure::without_usage("composio source missing toolkit"))?
            .to_ascii_lowercase();
        let connection_id = source
            .connection_id
            .as_deref()
            .map(str::trim)
            .filter(|connection_id| !connection_id.is_empty())
            .ok_or_else(|| {
                SourcePipelineFailure::without_usage("composio source missing connection_id")
            })?;
        let outcome = crate::sync::pipelines::host::run_composio_connection(
            &toolkit,
            connection_id,
            config,
            source.max_items,
            source.sync_depth_days,
        )
        .await
        .map_err(|failure| SourcePipelineFailure {
            message: failure.message,
            actions_called: failure.actions_called,
            provider_cost_usd: failure.provider_cost_usd,
        })?;
        return Ok(SyncOutcome {
            records_ingested: outcome.records_ingested,
            more_pending: outcome.more_pending,
            actions_called: outcome.actions_called,
            provider_cost_usd: outcome.provider_cost_usd,
            note: outcome.note,
        });
    }

    let memory = crate::global::client_if_ready()
        .ok_or_else(|| SourcePipelineFailure::without_usage("memory client is not ready"))?;
    let mut memory_config = super::memory_config_from(config, config.workspace_dir().clone());
    memory_config.sync.interval_secs = config.memory_sync_interval_secs();
    memory_config.sync.budget.max_items = source.max_items;
    memory_config.sync.budget.max_tokens_per_sync = source.max_tokens_per_sync;
    memory_config.sync.budget.max_cost_per_sync_usd = source.max_cost_per_sync_usd;
    memory_config.sync.budget.sync_depth_days = source.sync_depth_days;

    let pipeline = build_pipeline(source, config, &mut memory_config)
        .map_err(SourcePipelineFailure::without_usage)?;
    let pipeline_id = pipeline.id().to_owned();
    let mut dispatcher = SyncDispatcher::new();
    dispatcher
        .register(pipeline)
        .map_err(|error| SourcePipelineFailure::without_usage(error.to_string()))?;
    dispatcher
        .tick(
            &pipeline_id,
            &memory_config,
            &source_sync_context(memory, config, source.kind != SourceKind::Composio),
        )
        .await
        .map_err(|error| {
            let usage = error.downcast_ref::<tinycortex::memory::sync::SyncRunError>();
            SourcePipelineFailure {
                message: error.to_string(),
                actions_called: usage.map_or(0, |error| error.actions_called),
                provider_cost_usd: usage.map_or(0.0, |error| error.provider_cost_usd),
            }
        })
}

/// Run a Composio connection through tinycortex, preserving any source-level
/// budgets already configured in OpenHuman's registry.
pub async fn run_composio_connection(
    toolkit: &str,
    connection_id: &str,
    config: &Config,
) -> Result<SyncOutcome, SourcePipelineFailure> {
    run_composio_connection_with_budgets(toolkit, connection_id, config, None, None).await
}

/// Run a Composio connection with request-scoped budget overrides.
///
/// Provider RPCs carry these values in `ProviderContext`, before a source has
/// necessarily been persisted in the registry. Explicit values therefore take
/// precedence, while `None` preserves the registered/default source budget.
pub async fn run_composio_connection_with_budgets(
    toolkit: &str,
    connection_id: &str,
    config: &Config,
    max_items: Option<u32>,
    sync_depth_days: Option<u32>,
) -> Result<SyncOutcome, SourcePipelineFailure> {
    let mut source = crate::sources::decode_memory_sources(config)
        .iter()
        .find(|source| {
            source.kind == SourceKind::Composio
                && source.connection_id.as_deref() == Some(connection_id)
        })
        .cloned()
        .unwrap_or_else(|| {
            let (max_items, sync_depth_days) =
                crate::sources::memory_sync_defaults_for_toolkit(toolkit);
            MemorySourceEntry {
                id: format!("composio:{toolkit}:{connection_id}"),
                kind: SourceKind::Composio,
                label: format!("{toolkit} connection"),
                enabled: true,
                toolkit: Some(toolkit.to_ascii_lowercase()),
                connection_id: Some(connection_id.to_string()),
                path: None,
                glob: None,
                url: None,
                branch: None,
                paths: Vec::new(),
                max_commits: None,
                max_issues: None,
                max_prs: None,
                query: None,
                since_days: None,
                max_items,
                selector: None,
                max_tokens_per_sync: None,
                max_cost_per_sync_usd: None,
                sync_depth_days,
            }
        });

    source.max_items = max_items;
    source.sync_depth_days = sync_depth_days;

    tracing::debug!(
        toolkit,
        connection_id,
        source_id = %source.id,
        max_items = ?source.max_items,
        sync_depth_days = ?source.sync_depth_days,
        "[tinycortex:sync] dispatching Composio connection"
    );
    run_source_pipeline(&source, config).await
}

/// Load the persisted Composio sync state, in core's own vocabulary.
///
/// Was typed with the engine's `SyncState`; the copies share one serde shape
/// and one KV namespace (pinned by tests in
/// `sync::composio::providers::sync_state`), so the retype changes no bytes.
/// Kept in the engine module only because OpenHuman reaches it through the
/// engine shim path.
pub async fn load_composio_sync_state(
    toolkit: &str,
    connection_id: &str,
) -> anyhow::Result<crate::sync::composio::providers::sync_state::SyncState> {
    let memory = crate::global::client_if_ready()
        .ok_or_else(|| anyhow::anyhow!("memory client is not ready"))?;
    let host = crate::sync::pipelines::host::PipelineHost::without_tree_ingest(memory);
    crate::sync::composio::providers::sync_state::SyncState::load(&host, toolkit, connection_id)
        .await
}

pub async fn run_slack_search_backfill(
    connection_id: &str,
    backfill_days: i64,
    config: &Config,
) -> Result<SyncOutcome, SourcePipelineFailure> {
    // Delegates to the engine-free pipelines (#18 §B1); kept here because
    // OpenHuman reaches this function through the engine shim path.
    let outcome = crate::sync::pipelines::host::run_slack_search_backfill(
        connection_id,
        backfill_days,
        config,
    )
    .await
    .map_err(|failure| SourcePipelineFailure {
        message: failure.message,
        actions_called: failure.actions_called,
        provider_cost_usd: failure.provider_cost_usd,
    })?;
    Ok(SyncOutcome {
        records_ingested: outcome.records_ingested,
        more_pending: outcome.more_pending,
        actions_called: outcome.actions_called,
        provider_cost_usd: outcome.provider_cost_usd,
        note: outcome.note,
    })
}

/// Delegates to the engine-free pipelines (#18 §B1); kept because OpenHuman's
/// backfill binary reaches it through the engine shim path.
pub async fn run_gmail_backfill(
    connection_id: &str,
    query: &str,
    max_pages: usize,
    page_size: usize,
    config: &Config,
) -> Result<SyncOutcome, SourcePipelineFailure> {
    let outcome = crate::sync::pipelines::host::run_gmail_backfill(
        connection_id,
        query,
        max_pages,
        page_size,
        config,
    )
    .await
    .map_err(|failure| SourcePipelineFailure {
        message: failure.message,
        actions_called: failure.actions_called,
        provider_cost_usd: failure.provider_cost_usd,
    })?;
    Ok(SyncOutcome {
        records_ingested: outcome.records_ingested,
        more_pending: outcome.more_pending,
        actions_called: outcome.actions_called,
        provider_cost_usd: outcome.provider_cost_usd,
        note: outcome.note,
    })
}

fn build_pipeline(
    source: &MemorySourceEntry,
    _config: &Config,
    _memory_config: &mut tinycortex::memory::config::MemoryConfig,
) -> Result<std::sync::Arc<dyn SyncPipeline>, String> {
    if source.kind != SourceKind::Composio {
        let crate_source: tinycortex::memory::sources::MemorySourceEntry = serde_json::from_value(
            serde_json::to_value(source).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        if source.kind == SourceKind::GithubRepo {
            return GithubRepoSyncPipeline::new(crate_source)
                .map(|pipeline| std::sync::Arc::new(pipeline) as std::sync::Arc<dyn SyncPipeline>)
                .map_err(|error| error.to_string());
        }
        return WorkspaceSourcePipeline::new(crate_source)
            .map(|pipeline| std::sync::Arc::new(pipeline) as std::sync::Arc<dyn SyncPipeline>)
            .map_err(|error| error.to_string());
    }

    // Composio sources never reach this seam: `run_source_pipeline` routes
    // them to `crate::sync::pipelines` (#18 §B1) before building. Only the
    // tree-coupled kinds are built here.
    Err(format!(
        "engine seam does not build composio pipelines (kind {:?} unexpected here)",
        source.kind
    ))
}

#[async_trait]
impl SkillDocSink for HostSyncAdapter {
    async fn store(&self, document: SkillDocument) -> anyhow::Result<()> {
        tracing::debug!(
            toolkit = %document.toolkit,
            connection_id = %document.connection_id,
            document_id = %document.document_id,
            "[tinycortex:sync] storing synchronized document"
        );
        self.memory
            .store_skill_sync(
                &document.namespace_skill_id,
                &document.connection_id,
                &document.title,
                &document.content,
                Some("tinycortex-sync".into()),
                Some(document.metadata.clone()),
                Some("medium".into()),
                None,
                None,
                Some(document.document_id.clone()),
            )
            .await
            .map_err(anyhow::Error::msg)?;

        // #5473: additively reconnect the synced item to the memory tree. This
        // is a best-effort secondary index over the skill store, which is the
        // source of truth and has already committed above. A failure here must
        // NOT abort the connector sync: most providers do not tolerate scope
        // errors, so the orchestrator turns a `store` error into a run-aborting
        // `Err` — propagating would let one deterministically-poisonous item
        // stall the whole connection and re-fetch the page (Composio spend) on
        // every retry. Log and continue; the per-item source gate re-attempts
        // the item on a later sync, and an operator rebuild can backfill.
        // The config-less adapter (`sync_context`) has no ingest pipeline and is
        // not on the connector sync path, so it skips tree ingest entirely.
        if let Some(config) = self.config.as_deref() {
            if let Err(error) = self
                .ingest_document_into_memory_tree(config, &document)
                .await
            {
                tracing::warn!(
                    toolkit = %document.toolkit,
                    connection_id = %document.connection_id,
                    document_id = %document.document_id,
                    %error,
                    "[tinycortex:sync] memory-tree ingest failed; skill store retained"
                );
            }
        }
        Ok(())
    }

    async fn delete(&self, namespace_skill_id: &str, document_id: &str) -> anyhow::Result<()> {
        let namespace = format!("skill-{}", namespace_skill_id.trim());
        tracing::debug!(
            namespace,
            document_id,
            "[tinycortex:sync] deleting synchronized document"
        );
        self.memory
            .delete_document(&namespace, document_id)
            .await
            .map(|_| ())
            .map_err(anyhow::Error::msg)
    }
}

#[async_trait]
impl LocalDocumentSink for HostSyncAdapter {
    async fn upsert(&self, document: LocalDocument) -> anyhow::Result<()> {
        let config = self
            .config
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("local document sink missing host config"))?;
        let input = tinycortex::memory::ingest::canonicalize::document::DocumentInput {
            provider: "memory_sources:local".into(),
            title: document.title,
            body: document.body,
            modified_at: document.modified_at,
            source_ref: document.source_ref,
        };
        crate::ingest_pipeline::ingest_document_with_scope(
            &**config,
            &document.source_id,
            &document.owner,
            document.tags,
            input,
            document.path_scope,
        )
        .await
        .map(|_| ())
        .map_err(anyhow::Error::msg)
    }

    async fn delete(&self, source_id: &str) -> anyhow::Result<()> {
        let config = self
            .config
            .clone()
            .ok_or_else(|| anyhow::anyhow!("local document sink missing host config"))?;
        let source_id = source_id.to_owned();
        tokio::task::spawn_blocking(move || {
            crate::store::chunks::store::delete_chunks_by_source(
                &*config,
                crate::store::chunks::types::SourceKind::Document,
                &source_id,
            )
        })
        .await
        .map_err(|error| anyhow::anyhow!("local delete task failed: {error}"))??;
        Ok(())
    }
}

#[async_trait]
impl SyncStateStore for HostSyncAdapter {
    async fn get(&self, namespace: &str, key: &str) -> anyhow::Result<Option<serde_json::Value>> {
        self.memory
            .kv_get(Some(namespace), key)
            .await
            .map_err(anyhow::Error::msg)
    }

    async fn set(
        &self,
        namespace: &str,
        key: &str,
        value: &serde_json::Value,
    ) -> anyhow::Result<()> {
        self.memory
            .kv_set(Some(namespace), key, value)
            .await
            .map_err(anyhow::Error::msg)
    }
}

#[async_trait]
impl SyncEventSink for HostSyncAdapter {
    async fn emit(&self, event: SyncEvent) -> anyhow::Result<()> {
        crate::events::publish(crate::events::MemoryEvent::SyncStageChanged {
            trigger: "tinycortex".into(),
            stage: stage_name(event.stage).into(),
            provider: Some(event.toolkit),
            connection_id: event.connection_id,
            detail: event.message,
            source_id: Some(event.source_id),
        });
        Ok(())
    }
}

fn stage_name(stage: SyncStage) -> &'static str {
    match stage {
        SyncStage::Requested => "requested",
        SyncStage::Fetching => "fetching",
        SyncStage::Stored => "stored",
        SyncStage::Ingesting => "ingesting",
        SyncStage::Completed => "completed",
        SyncStage::Failed => "failed",
    }
}

#[cfg(test)]
mod tests {
    use super::build_pipeline;
    use crate::sources::MemorySourceEntry;
    use crate::sync::composio::{get_composio_sync_provider, init_default_composio_sync_providers};
    use crate::sync::pipelines::host::{is_composio_toolkit_syncable, syncable_composio_toolkits};

    /// The advertised set (`memory_sources.supported_toolkits`, sourced from the
    /// provider registry) and the syncable set (`build_pipeline`) must not
    /// diverge: a toolkit that is advertised but has no pipeline reports ACTIVE
    /// and then silently never ingests — the exact defect of #4957.
    ///
    /// Both directions are asserted against an explicit built-in slug set. The
    /// provider registry is process-global and sibling tests register throwaway
    /// providers into it without unregistering, so walking it directly would be
    /// order-flaky; pinning the built-in set keeps this deterministic.
    #[test]
    fn advertised_and_syncable_toolkit_sets_cannot_diverge() {
        init_default_composio_sync_providers();

        // Every syncable toolkit must have a registered provider — otherwise it
        // could never be advertised or auto-registered in the first place.
        for &slug in syncable_composio_toolkits() {
            assert!(
                get_composio_sync_provider(slug).is_some(),
                "syncable toolkit `{slug}` has no registered memory-sync provider"
            );
        }

        // Every built-in provider shipped by `init_default_composio_sync_providers`
        // must be syncable. This is the #4957 direction: advertising a provider
        // that `build_pipeline` rejects is the silent failure we guard against.
        //
        // We pin the built-in slug set explicitly rather than walking
        // `all_composio_sync_providers()`: that registry is process-global and
        // sibling tests register throwaway providers into it that they never
        // unregister (e.g. `provideronly` in composio/tools_tests.rs, `stub-no-active`
        // in composio/identity.rs), so a raw registry walk fails nondeterministically
        // depending on test execution order. A new built-in toolkit must be added to
        // this list, to `syncable_composio_toolkits`, and to `build_pipeline` together
        // — the assert_eq below fails loudly if the first two ever drift apart.
        const BUILTIN_SYNC_PROVIDERS: &[&str] =
            &["clickup", "github", "gmail", "linear", "notion", "slack"];

        let mut builtin = BUILTIN_SYNC_PROVIDERS.to_vec();
        builtin.sort_unstable();
        let mut syncable = syncable_composio_toolkits().to_vec();
        syncable.sort_unstable();
        assert_eq!(
            builtin, syncable,
            "the built-in provider set and syncable set diverged — a provider is \
             advertised without a matching `build_pipeline` arm, or vice versa (#4957)"
        );

        for &slug in BUILTIN_SYNC_PROVIDERS {
            assert!(
                get_composio_sync_provider(slug).is_some(),
                "built-in provider `{slug}` is not registered by \
                 init_default_composio_sync_providers"
            );
            assert!(
                is_composio_toolkit_syncable(slug),
                "built-in provider `{slug}` is advertised but has no build_pipeline arm — \
                 it would report ACTIVE and silently fail to sync (#4957)"
            );
        }
    }

    /// Behavioural regression for #4957: an unsupported Composio toolkit is
    /// rejected by `build_pipeline` *before* any credential/client resolution.
    ///
    /// We hand it a default `Config` (no Composio auth configured). If the gate
    /// ran AFTER config resolution we would get a config error ("backend bearer
    /// token is not configured" / "direct API key is not configured"); instead
    /// we must get the unsupported-toolkit error, proving the fail-closed
    /// ordering that stops an unsyncable toolkit from ever reaching a pipeline.
    #[test]
    fn build_pipeline_refuses_composio_sources() {
        // `googlecalendar` is a real Composio toolkit with no native pipeline —
        // exactly the prod case from #4957.
        let source: MemorySourceEntry = serde_json::from_value(serde_json::json!({
            "id": "composio:googlecalendar:conn-1",
            "kind": "composio",
            "label": "googlecalendar connection",
            "toolkit": "googlecalendar",
            "connection_id": "conn-1",
        }))
        .expect("construct composio source");

        let config = tinymemory_api::host::test_support::TestHostConfig::default();
        let mut memory_config =
            tinycortex::memory::config::MemoryConfig::new("/tmp/openhuman-test-ws");

        // Composio never reaches this seam any more: `run_source_pipeline`
        // routes it to the engine-free pipelines (#18 §B1). The seam's job is
        // to say so, not to half-build one.
        let err = match build_pipeline(&source, &config, &mut memory_config) {
            Ok(_) => panic!("the engine seam must refuse composio sources"),
            Err(e) => e,
        };
        assert!(
            err.contains("does not build composio pipelines"),
            "expected the composio refusal, got: {err}"
        );
    }

    /// Locks the reported prod failures (googlecalendar / googlesheets) as
    /// non-syncable, and pins case-insensitive/trimming behaviour.
    #[test]
    fn is_composio_toolkit_syncable_classifies_known_slugs() {
        assert!(!is_composio_toolkit_syncable("googlecalendar"));
        assert!(!is_composio_toolkit_syncable("googlesheets"));
        assert!(!is_composio_toolkit_syncable("discord"));
        assert!(!is_composio_toolkit_syncable(""));
        assert!(is_composio_toolkit_syncable("gmail"));
        assert!(is_composio_toolkit_syncable("Gmail"));
        assert!(is_composio_toolkit_syncable("  slack "));
    }

    /// Regression for #5473: a Composio connector sync must feed the memory tree,
    /// not just the `skill-<toolkit>` document store. The TinyCortex migration
    /// (#4794) dropped the tree-ingest half, so synced items stopped producing
    /// `mem_tree_chunks` rows and fell out of tree-backed recall. This fails if
    /// the `SkillDocSink` store path ever stops writing tree chunks again.
    #[tokio::test]
    async fn composio_sync_document_reaches_memory_tree() {
        use crate::store::{MemoryClient, MemoryClientRef};
        use std::sync::Arc;
        use tinycortex::memory::sync::{SkillDocSink, SkillDocument};
        use tinymemory_api::host::test_support::TestHostConfig;
        use tinymemory_api::host::MemoryHostConfig;

        crate::test_seams::init();
        let workspace = tempfile::tempdir().expect("workspace");
        let workspace_dir = workspace.path().join("workspace");

        let mut host = TestHostConfig::default();
        host.workspace_dir = workspace_dir.clone();
        let config = host.to_arc();

        let client: MemoryClientRef = Arc::new(
            MemoryClient::from_workspace_dir(workspace_dir)
                .expect("memory client initialises against a fresh workspace"),
        );
        let adapter = super::HostSyncAdapter::with_config(client, config.clone());

        // Precondition: a fresh tree is empty, so a post-store non-zero count is
        // attributable to the sync path rather than to pre-existing state.
        assert_eq!(
            crate::store::chunks::store::count_chunks(&*config).expect("count chunks"),
            0,
            "fresh workspace must start with an empty memory tree"
        );

        adapter
            .store(SkillDocument {
                namespace_skill_id: "gmail".into(),
                connection_id: "conn-1".into(),
                document_id: "gmail:msg-1".into(),
                title: "Quarterly planning".into(),
                content: "Let's finalise the Q3 roadmap and align on the launch date.".into(),
                toolkit: "gmail".into(),
                metadata: serde_json::json!({ "source": "composio-provider-incremental" }),
            })
            .await
            .expect("storing a synced document must also ingest it into the memory tree");

        let chunks = crate::store::chunks::store::count_chunks(&*config).expect("count chunks");
        assert!(
            chunks > 0,
            "a Composio sync must add mem_tree_chunks rows for the ingested item (#5473)"
        );

        // The chunk must carry the deterministic per-item source id
        // `{toolkit}:{connection_id}:{document_id}`; its `path_scope`
        // (`gmail:conn-1`) is what tree retrieval resolves by platform prefix.
        // A drift here is the silent "ingests but is never retrievable" trap.
        let scoped = crate::store::chunks::store::list_chunks(
            &*config,
            &tinycortex::memory::chunks::ListChunksQuery {
                source_id: Some("gmail:conn-1:gmail:msg-1".into()),
                limit: Some(8),
                ..Default::default()
            },
        )
        .expect("list chunks by source id");
        assert!(
            !scoped.is_empty(),
            "ingested chunks must be keyed by the deterministic connector source id"
        );
        assert!(
            scoped
                .iter()
                .all(|chunk| chunk.metadata.path_scope.as_deref() == Some("gmail:conn-1")),
            "connector chunks must carry the `{{toolkit}}:{{connection_id}}` tree scope so \
             query_source resolves them (gmail → email)"
        );

        // Retrievability is the real goal, and L0 chunks alone do NOT imply it:
        // `query_source` reads sealed summaries and skips unsealed trees, so
        // before a seal the freshly-ingested item is not yet retrievable.
        let before = crate::tree::retrieval::query_source(
            &*config,
            Some("gmail:conn-1"),
            None,
            None,
            None,
            10,
        )
        .await
        .expect("query_source before seal");
        assert!(
            before.hits.is_empty(),
            "an unsealed connector tree must not yet be retrievable"
        );

        // Drive the async extract worker to append the leaf, then force-seal the
        // buffer (the time-based flush path) so a level-1 summary exists.
        crate::queue::drain_until_idle(&*config)
            .await
            .expect("drain tree jobs");
        crate::tree::tree::flush::flush_stale_buffers(
            &*config,
            chrono::Duration::zero(),
            &crate::tree::tree::bucket_seal::LabelStrategy::Empty,
        )
        .await
        .expect("force-seal stale buffers");

        // Now the connector item is retrievable through the same path the
        // product uses for tree-backed recall — the property #5473 restores.
        let after = crate::tree::retrieval::query_source(
            &*config,
            Some("gmail:conn-1"),
            None,
            None,
            None,
            10,
        )
        .await
        .expect("query_source after seal");
        assert!(
            !after.hits.is_empty(),
            "a sealed connector tree must be retrievable via query_source (#5473)"
        );
    }

    /// The tree-ingest half of `store` is best-effort: when
    /// `ingest_document_with_scope` fails, `store` must log and still return
    /// `Ok(())`, so one deterministically-poisonous item cannot abort the whole
    /// connector run and re-fetch the page (Composio spend) on every retry — the
    /// #4947 stall that propagating the error re-created (sanil-23's review
    /// blocker #2). The skill store runs first and is the source of truth, so it
    /// must remain committed. This forces a real ingest failure by pointing the
    /// adapter's tree-ingest `config.workspace_dir` under a regular file (so the
    /// tree store cannot be created) while the skill-store client keeps a healthy
    /// workspace — isolating the failure to the tree half. If `store` ever
    /// propagates the ingest error again, the `.expect` on the store call fails.
    #[tokio::test]
    async fn tree_ingest_failure_is_tolerated_and_skill_store_is_retained() {
        use crate::store::{MemoryClient, MemoryClientRef};
        use std::sync::Arc;
        use tinycortex::memory::sync::{SkillDocSink, SkillDocument};
        use tinymemory_api::host::test_support::TestHostConfig;
        use tinymemory_api::host::MemoryHostConfig;

        crate::test_seams::init();
        let workspace = tempfile::tempdir().expect("workspace");

        // The skill store (source of truth) gets a healthy workspace …
        let client: MemoryClientRef = Arc::new(
            MemoryClient::from_workspace_dir(workspace.path().join("skill-store"))
                .expect("memory client initialises against a fresh workspace"),
        );

        // … but the tree-ingest config points at a workspace *under* a regular
        // file, so `ingest_document_with_scope` cannot create its store and
        // returns `Err` (same failure shape as the `fallible_audit_read` guard).
        let blocker = workspace.path().join("blocker");
        std::fs::write(&blocker, b"not a directory").expect("write blocker file");
        let mut host = TestHostConfig::default();
        host.workspace_dir = blocker.join("workspace");
        let config = host.to_arc();

        let adapter = super::HostSyncAdapter::with_config(client.clone(), config.clone());
        let document = SkillDocument {
            namespace_skill_id: "gmail".into(),
            connection_id: "conn-1".into(),
            document_id: "gmail:msg-1".into(),
            title: "Quarterly planning".into(),
            content: "Let's finalise the Q3 roadmap.".into(),
            toolkit: "gmail".into(),
            metadata: serde_json::json!({ "source": "composio-provider-incremental" }),
        };

        // Guard against a vacuous test: the tree-ingest half must *genuinely*
        // fail under the broken config. If the lever ever stops failing (e.g.
        // ingest resolves its store path elsewhere), this fires rather than the
        // test silently passing without exercising the tolerance path.
        assert!(
            adapter
                .ingest_document_into_memory_tree(&*config, &document)
                .await
                .is_err(),
            "the broken tree-ingest workspace must make ingest fail"
        );

        // `store` must swallow that tree-ingest failure and still succeed.
        adapter
            .store(document)
            .await
            .expect("store must tolerate a memory-tree ingest failure (best-effort tree)");

        // The skill store, committed before the tree half, still holds the item —
        // best-effort tree ingest must never cost the durable skill write.
        let skill_docs = client
            .list_documents(Some("skill-gmail"))
            .await
            .expect("list skill-gmail documents");
        let documents = skill_docs
            .get("documents")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        assert_eq!(
            documents.len(),
            1,
            "the skill store must retain the synced document even when tree ingest fails"
        );
        let persisted = serde_json::to_string(&documents).expect("serialise skill documents");
        assert!(
            persisted.contains("gmail:msg-1"),
            "the retained skill document must carry the synced id"
        );
    }

    /// The config-less adapter (`sync_context`) has no ingest pipeline and is not
    /// on the connector sync path, so it stores the skill document without
    /// touching the memory tree. Guards the `None` branch of `store` from
    /// regressing into a panic or an accidental (workspace-less) ingest.
    #[tokio::test]
    async fn config_less_adapter_skips_memory_tree_ingest() {
        use crate::store::{MemoryClient, MemoryClientRef};
        use std::sync::Arc;
        use tinycortex::memory::sync::{SkillDocSink, SkillDocument};
        use tinymemory_api::host::test_support::TestHostConfig;
        use tinymemory_api::host::MemoryHostConfig;

        crate::test_seams::init();
        let workspace = tempfile::tempdir().expect("workspace");
        let workspace_dir = workspace.path().join("workspace");

        let mut host = TestHostConfig::default();
        host.workspace_dir = workspace_dir.clone();
        let config = host.to_arc();

        let client: MemoryClientRef = Arc::new(
            MemoryClient::from_workspace_dir(workspace_dir)
                .expect("memory client initialises against a fresh workspace"),
        );
        // `new` leaves `config: None` — the config-less variant. Keep a handle
        // to the shared client so we can read the skill store back afterwards.
        let store_client = client.clone();
        let adapter = super::HostSyncAdapter::new(client);

        adapter
            .store(SkillDocument {
                namespace_skill_id: "gmail".into(),
                connection_id: "conn-1".into(),
                document_id: "gmail:msg-1".into(),
                title: "Quarterly planning".into(),
                content: "Let's finalise the Q3 roadmap and align on the launch date.".into(),
                toolkit: "gmail".into(),
                metadata: serde_json::json!({ "source": "composio-provider-incremental" }),
            })
            .await
            .expect("config-less store must still persist the skill document");

        // The skill store still receives the document (the always-on half of
        // `store`), keyed by its stable document id under `skill-gmail`.
        let skill_docs = store_client
            .list_documents(Some("skill-gmail"))
            .await
            .expect("list skill-gmail documents");
        let documents = skill_docs
            .get("documents")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        assert_eq!(
            documents.len(),
            1,
            "config-less store must persist exactly the one synced skill document"
        );
        let persisted = serde_json::to_string(&documents).expect("serialise skill documents");
        assert!(
            persisted.contains("gmail:msg-1") && persisted.contains("Quarterly planning"),
            "the persisted skill document must carry the synced id and title"
        );

        // …but the tree is untouched, because the config-less adapter has no
        // ingest pipeline.
        assert_eq!(
            crate::store::chunks::store::count_chunks(&*config).expect("count chunks"),
            0,
            "a config-less adapter must not ingest into the memory tree"
        );
    }

    /// The blank-scope guard: an item whose toolkit is empty would form an
    /// unreachable `":conn"` tree scope, so `ingest_document_into_memory_tree`
    /// skips it — the skill store still receives it, the tree does not. Covers
    /// the early-return branch (a valid toolkit yields chunks, as the retrieval
    /// test proves; a blank one must not).
    #[tokio::test]
    async fn blank_scope_item_is_skipped_for_memory_tree_ingest() {
        use crate::store::{MemoryClient, MemoryClientRef};
        use std::sync::Arc;
        use tinycortex::memory::sync::{SkillDocSink, SkillDocument};
        use tinymemory_api::host::test_support::TestHostConfig;
        use tinymemory_api::host::MemoryHostConfig;

        crate::test_seams::init();
        let workspace = tempfile::tempdir().expect("workspace");
        let workspace_dir = workspace.path().join("workspace");
        let mut host = TestHostConfig::default();
        host.workspace_dir = workspace_dir.clone();
        let config = host.to_arc();
        let client: MemoryClientRef = Arc::new(
            MemoryClient::from_workspace_dir(workspace_dir).expect("memory client initialises"),
        );
        let adapter = super::HostSyncAdapter::with_config(client, config.clone());

        adapter
            .store(SkillDocument {
                namespace_skill_id: "gmail".into(),
                connection_id: "conn-1".into(),
                document_id: "gmail:msg-1".into(),
                title: "Quarterly planning".into(),
                content: "Let's finalise the Q3 roadmap.".into(),
                // Blank after trim — no platform scope can be formed.
                toolkit: "   ".into(),
                metadata: serde_json::json!({}),
            })
            .await
            .expect("store must still succeed for an item without a tree scope");

        assert_eq!(
            crate::store::chunks::store::count_chunks(&*config).expect("count chunks"),
            0,
            "an item without a toolkit/connection scope must be skipped for tree ingest"
        );
    }
}
