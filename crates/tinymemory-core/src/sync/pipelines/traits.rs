//! Host seams and pipeline contracts for live synchronization.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncPipelineKind {
    Composio,
    Workspace,
    Mcp,
}

impl SyncPipelineKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Composio => "composio",
            Self::Workspace => "workspace",
            Self::Mcp => "mcp",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncStage {
    Requested,
    Fetching,
    Stored,
    Ingesting,
    Completed,
    Failed,
}

/// Stable wire name for each stage, shared by every event adapter.
pub fn stage_name(stage: SyncStage) -> &'static str {
    match stage {
        SyncStage::Requested => "requested",
        SyncStage::Fetching => "fetching",
        SyncStage::Stored => "stored",
        SyncStage::Ingesting => "ingesting",
        SyncStage::Completed => "completed",
        SyncStage::Failed => "failed",
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyncEvent {
    pub source_id: String,
    pub toolkit: String,
    pub connection_id: Option<String>,
    pub stage: SyncStage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[async_trait]
pub trait SyncEventSink: Send + Sync {
    async fn emit(&self, event: SyncEvent) -> anyhow::Result<()>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkillDocument {
    pub namespace_skill_id: String,
    pub connection_id: String,
    pub document_id: String,
    pub title: String,
    pub content: String,
    pub toolkit: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[async_trait]
pub trait SkillDocSink: Send + Sync {
    async fn store(&self, document: SkillDocument) -> anyhow::Result<()>;
    async fn delete(&self, namespace_skill_id: &str, document_id: &str) -> anyhow::Result<()>;
}

/// How the Composio client reaches the API: straight at it, or through the
/// backend proxy. The engine's enum, ported with the client — distinct from
/// `tinymemory_api::host::ComposioMode`, which is the *host seam's*
/// string-typed setting; the seam converts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ComposioMode {
    /// Call api.composio.dev with the host's own key.
    Direct,
    /// Route through the backend proxy.
    #[default]
    Proxied,
}

/// The Composio client's connection settings, owned here so the pipelines
/// take no engine config type (#18 §B1). The engine keeps its own copy for
/// its internal pipelines; the host constructs this one from its own config.
#[derive(Clone, Debug, Default)]
pub struct ComposioSyncConfig {
    pub mode: ComposioMode,
    pub base_url: String,
    pub api_key: Option<SecretString>,
    pub bearer_token: Option<SecretString>,
    pub entity_id: Option<String>,
    /// Optional Gmail search query the Gmail pipeline ANDs onto every page
    /// fetch (e.g. `label:brain`) so background sync only ingests matching
    /// messages. `None` = whole inbox window.
    pub gmail_query: Option<String>,
}

/// A string whose `Debug` never prints the value.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl std::fmt::Debug for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretString(redacted)")
    }
}

/// What a pipeline may read of the host's configuration: the Composio
/// connection settings and the sync-depth budget. Deliberately not the
/// host's whole config — a pipeline that needs more must argue for the
/// field here.
#[derive(Clone, Debug, Default)]
pub struct PipelineConfig {
    pub composio: Option<ComposioSyncConfig>,
    pub sync_depth_days: Option<u32>,
    pub max_items: Option<u32>,
    /// Stop the run once this many tokens (estimated from stored content)
    /// have been ingested. `None` = unbounded.
    pub max_tokens_per_sync: Option<u64>,
    /// Stop the run once the provider has charged this much. `None` =
    /// unbounded.
    pub max_cost_per_sync_usd: Option<f64>,
}

/// Host capabilities required by sync pipelines.
#[derive(Clone)]
pub struct SyncContext {
    pub events: Arc<dyn SyncEventSink>,
    pub documents: Arc<dyn SkillDocSink>,
    pub state: Arc<dyn crate::sync::composio::providers::sync_state::SyncStateStore>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SyncOutcome {
    pub records_ingested: u32,
    pub more_pending: bool,
    #[serde(default)]
    pub actions_called: u32,
    #[serde(default)]
    pub provider_cost_usd: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct SyncRunError {
    pub actions_called: u32,
    pub provider_cost_usd: f64,
    message: String,
}

impl SyncRunError {
    pub fn new(message: impl Into<String>, actions_called: u32, provider_cost_usd: f64) -> Self {
        Self {
            actions_called,
            provider_cost_usd,
            message: message.into(),
        }
    }
}

#[async_trait]
pub trait SyncPipeline: Send + Sync {
    fn id(&self) -> &str;
    fn kind(&self) -> SyncPipelineKind;
    async fn init(&self, config: &PipelineConfig, context: &SyncContext) -> anyhow::Result<()>;
    async fn tick(
        &self,
        config: &PipelineConfig,
        context: &SyncContext,
    ) -> anyhow::Result<SyncOutcome>;
}
