//! The host side of the engine-free pipelines (#18 §B1): the sink adapter
//! over [`MemoryClient`](crate::store::MemoryClient), the Composio settings
//! mapping, and the runners the
//! rest of `core/src/sync/` calls.
//!
//! This is the piece §B5's acceptance rests on: a pipeline sees three
//! capabilities — events, documents, state — and every one resolves through
//! [`MemoryClient`](crate::store::MemoryClient), so whatever driver the host
//! bound serves the sync.

use std::sync::Arc;

use async_trait::async_trait;

use crate::store::MemoryClientRef;
use crate::sync::composio::providers::sync_state::SyncStateStore;
use crate::sync::pipelines::composio::{
    ClickUpSyncPipeline, ComposioClient, GitHubSyncPipeline, GmailSyncPipeline, LinearSyncPipeline,
    NotionSyncPipeline, SlackSearchBackfillPipeline, SlackSyncPipeline,
};
use crate::sync::pipelines::dispatcher::SyncDispatcher;
use crate::sync::pipelines::traits::{
    ComposioMode, ComposioSyncConfig, PipelineConfig, SecretString, SkillDocSink, SkillDocument,
    SyncContext, SyncEvent, SyncEventSink, SyncOutcome, SyncPipeline, SyncRunError,
};
use crate::Config;

/// A failed pipeline run, with whatever usage it burned before failing.
#[derive(Debug)]
pub struct PipelineFailure {
    pub message: String,
    pub actions_called: u32,
    pub provider_cost_usd: f64,
}

impl std::fmt::Display for PipelineFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PipelineFailure {}

impl PipelineFailure {
    pub fn without_usage(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            actions_called: 0,
            provider_cost_usd: 0.0,
        }
    }
}

/// Adapter giving the pipelines their three capabilities over the bound
/// memory client. The engine's `HostSyncAdapter` remains for the engine's own
/// pipelines; this one exists so a Composio sync never needs the engine.
pub struct PipelineHost {
    memory: MemoryClientRef,
    config: Option<Arc<Config>>,
}

impl PipelineHost {
    /// An adapter that also feeds the memory tree after each stored document
    /// (parity with the engine adapter's #5473 behaviour).
    pub fn new(memory: MemoryClientRef, config: Arc<Config>) -> Self {
        Self {
            memory,
            config: Some(config),
        }
    }

    /// An adapter with no host config: documents are stored, tree ingest is
    /// skipped. This is the shape a non-TinyCortex host uses.
    pub fn without_tree_ingest(memory: MemoryClientRef) -> Self {
        Self {
            memory,
            config: None,
        }
    }

    /// The pipeline context over this adapter.
    pub fn context(self: &Arc<Self>) -> SyncContext {
        SyncContext {
            events: self.clone(),
            documents: self.clone(),
            state: self.clone(),
        }
    }
}

#[async_trait]
impl SkillDocSink for PipelineHost {
    async fn store(&self, document: SkillDocument) -> anyhow::Result<()> {
        tracing::debug!(
            toolkit = %document.toolkit,
            connection_id = %document.connection_id,
            document_id = %document.document_id,
            "[memory_sync] storing synchronized document"
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

        // #5473: additively reconnect the synced item to the memory tree — a
        // best-effort secondary index; the skill store above is the source of
        // truth and has committed. A failure here must NOT abort the sync (one
        // poisonous item would stall the connection and re-buy the page on
        // every retry). The config-less adapter skips tree ingest entirely.
        if let Some(config) = self.config.as_deref() {
            if let Err(error) = ingest_into_tree(config, &document).await {
                tracing::warn!(
                    %error,
                    document_id = %document.document_id,
                    "[memory_sync] tree ingest failed; skill store remains authoritative"
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
            "[memory_sync] deleting synchronized document"
        );
        self.memory
            .delete_document(&namespace, document_id)
            .await
            .map(|_| ())
            .map_err(anyhow::Error::msg)
    }
}

/// Mirror of the engine adapter's tree reconnect: route the stored document
/// through core's ingest funnel under the same source id scheme.
async fn ingest_into_tree(config: &Config, document: &SkillDocument) -> anyhow::Result<()> {
    let source_id = format!(
        "composio:{}:{}:{}",
        document.toolkit, document.connection_id, document.document_id
    );
    let doc = crate::ingest_pipeline::IngestDocumentInput {
        provider: document.toolkit.clone(),
        title: document.title.clone(),
        body: document.content.clone(),
        modified_at: chrono::Utc::now(),
        source_ref: Some(source_id.clone()),
    };
    let tags = vec!["composio_sync".to_string(), document.toolkit.clone()];
    crate::ingest_pipeline::ingest_document(config, &source_id, "", tags, doc)
        .await
        .map(|_| ())
        .map_err(|error| anyhow::anyhow!("{error}"))
}

#[async_trait]
impl SyncEventSink for PipelineHost {
    async fn emit(&self, event: SyncEvent) -> anyhow::Result<()> {
        crate::events::publish(crate::events::MemoryEvent::SyncStageChanged {
            trigger: "tinycortex".into(),
            stage: super::traits::stage_name(event.stage).into(),
            provider: Some(event.toolkit),
            connection_id: event.connection_id,
            detail: event.message,
            source_id: Some(event.source_id),
        });
        Ok(())
    }
}

#[async_trait]
impl SyncStateStore for PipelineHost {
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

/// The Composio connection settings from the host's config — the same
/// resolution the engine seam performs, onto the local types.
pub fn composio_config(config: &Config) -> Result<ComposioSyncConfig, String> {
    if config.composio().mode.eq_ignore_ascii_case("direct") {
        let api_key = crate::composio_host::api_key(config)
            .or_else(|| config.composio().api_key.clone())
            .ok_or_else(|| "Composio direct API key is not configured".to_string())?;
        Ok(ComposioSyncConfig {
            mode: ComposioMode::Direct,
            base_url: "https://backend.composio.dev/api/v3".into(),
            api_key: Some(SecretString::new(api_key)),
            bearer_token: None,
            entity_id: Some(config.composio().entity_id.clone()),
        })
    } else {
        let bearer = config
            .session_token()?
            .ok_or_else(|| "OpenHuman backend bearer token is not configured".to_string())?;
        Ok(ComposioSyncConfig {
            mode: ComposioMode::Proxied,
            base_url: config.effective_backend_api_url(),
            api_key: None,
            bearer_token: Some(SecretString::new(bearer)),
            entity_id: Some(config.composio().entity_id.clone()),
        })
    }
}

/// The toolkits with a native pipeline here. Kept identical to the engine
/// seam's list; `sync_status` advertising draws from the provider registry.
pub fn syncable_composio_toolkits() -> &'static [&'static str] {
    &["clickup", "github", "gmail", "linear", "notion", "slack"]
}

/// Whether `toolkit` has a native pipeline (case-insensitive).
pub fn is_composio_toolkit_syncable(toolkit: &str) -> bool {
    let slug = toolkit.trim().to_ascii_lowercase();
    syncable_composio_toolkits().contains(&slug.as_str())
}

fn build_composio_pipeline(
    toolkit: &str,
    connection_id: &str,
    composio: ComposioSyncConfig,
) -> Result<Arc<dyn SyncPipeline>, String> {
    // Fail closed before resolving credentials for any toolkit without a
    // native pipeline (#4957) — the gate stays a single testable list.
    if !is_composio_toolkit_syncable(toolkit) {
        return Err(format!("memory sync does not support toolkit '{toolkit}'"));
    }
    let client = ComposioClient::new(composio);
    Ok(match toolkit {
        "gmail" => Arc::new(GmailSyncPipeline::new(client, connection_id)),
        "github" => Arc::new(GitHubSyncPipeline::new(client, connection_id)),
        "notion" => Arc::new(NotionSyncPipeline::new(client, connection_id)),
        "linear" => Arc::new(LinearSyncPipeline::new(client, connection_id)),
        "clickup" => Arc::new(ClickUpSyncPipeline::new(client, connection_id)),
        "slack" => Arc::new(SlackSyncPipeline::new(client, connection_id)),
        _ => unreachable!("gated by is_composio_toolkit_syncable"),
    })
}

/// Run one Composio connection through the engine-free pipelines.
pub async fn run_composio_connection(
    toolkit: &str,
    connection_id: &str,
    config: &Config,
    max_items: Option<u32>,
    sync_depth_days: Option<u32>,
) -> Result<SyncOutcome, PipelineFailure> {
    let memory = crate::global::client_if_ready()
        .ok_or_else(|| PipelineFailure::without_usage("memory client is not ready"))?;
    let composio = composio_config(config).map_err(PipelineFailure::without_usage)?;
    let pipeline = build_composio_pipeline(toolkit, connection_id, composio)
        .map_err(PipelineFailure::without_usage)?;
    let pipeline_config = PipelineConfig {
        composio: None, // the client already holds the connection settings
        sync_depth_days,
        max_items,
    };
    let host = Arc::new(PipelineHost::new(memory, config.to_arc()));
    run_pipeline(pipeline, &pipeline_config, &host.context()).await
}

/// Run a bounded Gmail backfill through the engine-free pipelines.
pub async fn run_gmail_backfill(
    connection_id: &str,
    query: &str,
    max_pages: usize,
    page_size: usize,
    config: &Config,
) -> Result<SyncOutcome, PipelineFailure> {
    let memory = crate::global::client_if_ready()
        .ok_or_else(|| PipelineFailure::without_usage("memory client is not ready"))?;
    let composio = composio_config(config).map_err(PipelineFailure::without_usage)?;
    let pipeline: Arc<dyn SyncPipeline> = Arc::new(
        GmailSyncPipeline::new(ComposioClient::new(composio), connection_id)
            .with_limits(max_pages, page_size)
            .with_query(query),
    );
    let host = Arc::new(PipelineHost::new(memory, config.to_arc()));
    run_pipeline(pipeline, &PipelineConfig::default(), &host.context()).await
}

/// Run the Slack search backfill through the engine-free pipelines.
pub async fn run_slack_search_backfill(
    connection_id: &str,
    backfill_days: i64,
    config: &Config,
) -> Result<SyncOutcome, PipelineFailure> {
    let memory = crate::global::client_if_ready()
        .ok_or_else(|| PipelineFailure::without_usage("memory client is not ready"))?;
    let composio = composio_config(config).map_err(PipelineFailure::without_usage)?;
    let client = ComposioClient::new(composio);
    let pipeline: Arc<dyn SyncPipeline> = Arc::new(SlackSearchBackfillPipeline::new(
        client,
        connection_id,
        backfill_days,
    ));
    let host = Arc::new(PipelineHost::new(memory, config.to_arc()));
    run_pipeline(pipeline, &PipelineConfig::default(), &host.context()).await
}

async fn run_pipeline(
    pipeline: Arc<dyn SyncPipeline>,
    config: &PipelineConfig,
    context: &SyncContext,
) -> Result<SyncOutcome, PipelineFailure> {
    let pipeline_id = pipeline.id().to_owned();
    let mut dispatcher = SyncDispatcher::new();
    dispatcher
        .register(pipeline)
        .map_err(|error| PipelineFailure::without_usage(error.to_string()))?;
    dispatcher
        .tick(&pipeline_id, config, context)
        .await
        .map_err(|error| {
            let usage = error.downcast_ref::<SyncRunError>();
            PipelineFailure {
                message: error.to_string(),
                actions_called: usage.map_or(0, |error| error.actions_called),
                provider_cost_usd: usage.map_or(0.0, |error| error.provider_cost_usd),
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #4957: an unsupported toolkit is rejected *before* credentials are
    /// resolved — moved here with the gate itself from the engine seam.
    #[test]
    fn unsupported_toolkit_is_rejected_before_resolving_credentials() {
        let err =
            build_composio_pipeline("googlecalendar", "conn-1", ComposioSyncConfig::default())
                .err()
                .expect("unsupported toolkit must be rejected");
        assert!(
            err.contains("does not support toolkit 'googlecalendar'"),
            "got: {err}"
        );
    }

    #[test]
    fn the_syncable_set_is_exactly_the_native_pipelines() {
        for toolkit in syncable_composio_toolkits() {
            assert!(
                build_composio_pipeline(toolkit, "conn-1", ComposioSyncConfig::default()).is_ok(),
                "advertised toolkit '{toolkit}' must build"
            );
        }
        assert!(!is_composio_toolkit_syncable("googlecalendar"));
        assert!(is_composio_toolkit_syncable(" Gmail "));
    }
}
