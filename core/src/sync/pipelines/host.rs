//! The host side of the engine-free pipelines (#18 §B1): the sink adapter
//! over [`MemoryClient`](crate::store::MemoryClient), the Composio settings
//! mapping, and the runners the
//! rest of `core/src/sync/` calls.
//!
//! This is the piece §B5's acceptance rests on: a pipeline sees three
//! capabilities — events, documents, state — and every one resolves through
//! [`MemoryClient`](crate::store::MemoryClient), so whatever driver the host
//! bound serves the sync.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use async_trait::async_trait;
use tokio::sync::OwnedMutexGuard;

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

/// Mirror of the engine adapter's tree reconnect (`engine::sync`'s
/// `ingest_document_into_memory_tree`): route the stored document through
/// core's ingest funnel under the same addressing scheme.
///
/// # The scheme is the contract, not an implementation detail
///
/// The tree scope a chunk seals under is `path_scope`, falling back to
/// `source_id`. Retrieval selects source trees by that scope and classifies
/// them by their **platform prefix** — `gmail:` is email, `slack:` is chat.
/// So the scope has to be `"{toolkit}:{connection_id}"`: one tree per
/// connection, named by a prefix retrieval knows.
///
/// Passing no `path_scope` is not a smaller version of that. It makes each
/// item's own `source_id` the scope, which is a *tree per document*, named by
/// a prefix that matches no platform — the items are stored and then
/// unreachable, which is the #5473 defect this reconnect exists to fix. The
/// `source_id LIKE` prefix the memory-source status and diff snapshots query
/// by is keyed on this same scheme — see `sources::status::source_id_prefix`.
///
/// Tags are a deliberate superset of the engine adapter's: it tags the toolkit
/// alone, this also tags `composio_sync`. Tags feed scoring and filtering, not
/// addressing, so the extra one costs nothing and marks the ingest path.
async fn ingest_into_tree(config: &Config, document: &SkillDocument) -> anyhow::Result<()> {
    let toolkit = document.toolkit.trim().to_ascii_lowercase();
    let connection_id = document.connection_id.trim();
    // A blank toolkit or connection would yield a scope with no platform
    // prefix (`":conn"`, `"gmail:"`), which no retrieval kind matches; skip
    // rather than write an unreachable tree. The skill store still holds the
    // item.
    if toolkit.is_empty() || connection_id.is_empty() {
        tracing::debug!(
            document_id = %document.document_id,
            "[memory_sync] skipping memory-tree ingest: item has no toolkit/connection scope"
        );
        return Ok(());
    }
    let tree_scope = format!("{toolkit}:{connection_id}");
    let source_id = format!("{tree_scope}:{}", document.document_id);
    let owner = format!("{toolkit}-sync:{connection_id}");
    let doc = crate::ingest_pipeline::IngestDocumentInput {
        provider: format!("composio:{toolkit}"),
        title: document.title.clone(),
        body: document.content.clone(),
        modified_at: chrono::Utc::now(),
        source_ref: Some(document.document_id.clone()),
    };
    let tags = vec!["composio_sync".to_string(), toolkit];
    crate::ingest_pipeline::ingest_document_with_scope(
        config,
        &source_id,
        &owner,
        tags,
        doc,
        Some(tree_scope),
    )
    .await
    .map(|_| ())
    .map_err(|error| anyhow::anyhow!("memory-tree ingest failed for source `{source_id}`: {error}"))
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
    //
    // Normalise once and match on the normalised slug: the gate accepts
    // `" Gmail "` (trim + lowercase), so matching on the raw input would let a
    // padded or mixed-case toolkit through the gate and into `unreachable!`.
    let slug = toolkit.trim().to_ascii_lowercase();
    if !syncable_composio_toolkits().contains(&slug.as_str()) {
        return Err(format!("memory sync does not support toolkit '{toolkit}'"));
    }
    let client = ComposioClient::new(composio);
    Ok(match slug.as_str() {
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
    run_composio_connection_with_caps(
        toolkit,
        connection_id,
        config,
        SourceCaps {
            max_items,
            sync_depth_days,
            ..SourceCaps::default()
        },
    )
    .await
}

/// The per-source limits a run honours. All `None` = the source's defaults.
#[derive(Clone, Copy, Debug, Default)]
pub struct SourceCaps {
    pub max_items: Option<u32>,
    pub sync_depth_days: Option<u32>,
    pub max_tokens_per_sync: Option<u64>,
    pub max_cost_per_sync_usd: Option<f64>,
}

impl SourceCaps {
    /// The caps a registry entry carries.
    pub fn from_source(source: &tinymemory_sources::MemorySourceEntry) -> Self {
        Self {
            max_items: source.max_items,
            sync_depth_days: source.sync_depth_days,
            max_tokens_per_sync: source.max_tokens_per_sync,
            max_cost_per_sync_usd: source.max_cost_per_sync_usd,
        }
    }
}

/// Run one Composio connection through the engine-free pipelines, honouring
/// every per-source cap.
pub async fn run_composio_connection_with_caps(
    toolkit: &str,
    connection_id: &str,
    config: &Config,
    caps: SourceCaps,
) -> Result<SyncOutcome, PipelineFailure> {
    let memory = crate::global::client_if_ready()
        .ok_or_else(|| PipelineFailure::without_usage("memory client is not ready"))?;
    let composio = composio_config(config).map_err(PipelineFailure::without_usage)?;
    let pipeline = build_composio_pipeline(toolkit, connection_id, composio)
        .map_err(PipelineFailure::without_usage)?;
    let pipeline_config = PipelineConfig {
        composio: None, // the client already holds the connection settings
        sync_depth_days: caps.sync_depth_days,
        max_items: caps.max_items,
        max_tokens_per_sync: caps.max_tokens_per_sync,
        max_cost_per_sync_usd: caps.max_cost_per_sync_usd,
    };
    let host = Arc::new(PipelineHost::new(memory, config.to_arc()));
    run_pipeline(
        pipeline,
        toolkit,
        connection_id,
        &pipeline_config,
        &host.context(),
    )
    .await
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
    // The backfill drives the Gmail pipeline, which keys its `SyncState` on
    // `"gmail"`; naming the same toolkit here puts it behind the same guard as
    // a periodic or RPC Gmail sync of this connection.
    run_pipeline(
        pipeline,
        "gmail",
        connection_id,
        &PipelineConfig::default(),
        &host.context(),
    )
    .await
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
    // `SlackSearchBackfillPipeline` loads and saves the same
    // `("slack", connection_id)` state the Slack sync pipeline does, so the two
    // must share one guard or they clobber each other's cursor and budget.
    run_pipeline(
        pipeline,
        "slack",
        connection_id,
        &PipelineConfig::default(),
        &host.context(),
    )
    .await
}

/// The note a run carries when another run already holds its connection.
///
/// Callers that distinguish "nothing to sync" from "did not sync" match on
/// this rather than on a message they would have to keep in step by hand.
pub const SYNC_ALREADY_RUNNING: &str = "sync already running for this connection";

/// One guard per connection, so two runs cannot clobber each other's state.
type ConnectionLock = Arc<tokio::sync::Mutex<()>>;

/// The process-wide guard table.
///
/// `run_incremental_sync` loads the connection's `SyncState` once, mutates it
/// in memory for the whole run, and saves at the end; the Slack search
/// backfill does the same over the same `("slack", connection_id)` record. Two
/// runs of one connection therefore race on the cursor, the dedup set and the
/// daily budget, and whichever saves last wins — losing either the dedup set
/// (re-fetch, re-spend) or the budget count (overspend past the cap). The
/// periodic loop, the sync RPC and a trigger can each fire the same
/// connection, so the race is reachable as the code stands.
///
/// This is the single-process answer, which is how the loop and the RPC paths
/// actually run. An optimistic version stamp on the KV record is what a
/// multi-process host would need instead.
///
/// The table only ever grows, bounded by the number of connections the host
/// has seen — the same shape, and the same bound, as the periodic scheduler's
/// last-fired map. An entry is one `Arc` and an unlocked mutex.
fn connection_locks() -> &'static Mutex<HashMap<(String, String), ConnectionLock>> {
    static LOCKS: OnceLock<Mutex<HashMap<(String, String), ConnectionLock>>> = OnceLock::new();
    LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The guard key for a connection.
///
/// Normalised exactly as [`build_composio_pipeline`] normalises the toolkit
/// gate, so `" Gmail "` and `gmail` name one connection rather than two — and
/// so the Slack sync pipeline and the Slack search backfill, which share one
/// `SyncState` record, share one guard.
fn connection_key(toolkit: &str, connection_id: &str) -> (String, String) {
    (
        toolkit.trim().to_ascii_lowercase(),
        connection_id.trim().to_owned(),
    )
}

/// Take the guard for one connection, or `None` if a run already holds it.
///
/// Deliberately non-blocking. Queueing behind the running sync would stall the
/// periodic loop's whole tick — it walks connections sequentially — and then
/// run a second sync of a connection that has just been synced, which is the
/// Composio spend this guard exists to avoid.
fn try_hold_connection(toolkit: &str, connection_id: &str) -> Option<OwnedMutexGuard<()>> {
    let lock = {
        // A panic inside a run cannot corrupt the table: it holds `Arc`s, and
        // the async guard is released by its own `Drop`. Recovering from the
        // poison keeps one panicking sync from disabling every later one.
        let mut locks = connection_locks()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Arc::clone(
            locks
                .entry(connection_key(toolkit, connection_id))
                .or_default(),
        )
    };
    lock.try_lock_owned().ok()
}

async fn run_pipeline(
    pipeline: Arc<dyn SyncPipeline>,
    toolkit: &str,
    connection_id: &str,
    config: &PipelineConfig,
    context: &SyncContext,
) -> Result<SyncOutcome, PipelineFailure> {
    let Some(_connection) = try_hold_connection(toolkit, connection_id) else {
        tracing::debug!(
            toolkit,
            connection_id,
            "[memory_sync] a sync of this connection is already running; skipping"
        );
        return Ok(SyncOutcome {
            note: Some(SYNC_ALREADY_RUNNING.to_owned()),
            ..SyncOutcome::default()
        });
    };
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

    /// The gate normalises; the build must match on the same normalised
    /// slug, or a padded/mixed-case toolkit passes the gate and panics.
    #[test]
    fn a_padded_or_mixed_case_toolkit_builds_rather_than_panicking() {
        for toolkit in [" Gmail ", "GMAIL", "gmail\t", " Slack"] {
            assert!(
                build_composio_pipeline(toolkit, "conn-1", ComposioSyncConfig::default()).is_ok(),
                "{toolkit:?} passes the gate and must build"
            );
        }
    }

    /// The guard table is process-global and shared by every test in this
    /// binary, so each test names connections nothing else touches.
    #[test]
    fn one_connection_admits_one_run_at_a_time() {
        let held =
            try_hold_connection("gmail", "guard-single").expect("the first run takes the guard");
        assert!(
            try_hold_connection("gmail", "guard-single").is_none(),
            "a second run of the same connection must be refused, not queued"
        );
        drop(held);
        assert!(
            try_hold_connection("gmail", "guard-single").is_some(),
            "the guard must be released when the run ends"
        );
    }

    /// The guard is per connection, not per toolkit: one slow Gmail sync must
    /// not stop every other Gmail connection from syncing.
    #[test]
    fn different_connections_hold_independent_guards() {
        let first = try_hold_connection("gmail", "guard-independent-a")
            .expect("the first connection takes its guard");
        let second = try_hold_connection("gmail", "guard-independent-b")
            .expect("a different connection has its own guard");
        drop((first, second));
    }

    /// `build_composio_pipeline` accepts `" Gmail "` by normalising it. The
    /// guard key must normalise identically, or a padded toolkit syncs the
    /// same connection concurrently with an unpadded one and they clobber each
    /// other's state — the defect the guard exists to prevent.
    #[test]
    fn the_guard_key_normalises_the_toolkit_like_the_gate() {
        assert_eq!(
            connection_key(" Gmail ", " conn-1 "),
            connection_key("gmail", "conn-1")
        );
        let held = try_hold_connection("gmail", "guard-normalised")
            .expect("the first run takes the guard");
        assert!(
            try_hold_connection(" GMAIL\t", "guard-normalised").is_none(),
            "a padded, mixed-case toolkit names the same connection"
        );
        drop(held);
    }

    /// The Slack sync pipeline and the Slack search backfill load and save the
    /// same `("slack", connection_id)` state, so they must contend.
    #[test]
    fn the_slack_backfill_shares_the_slack_sync_guard() {
        assert_eq!(
            connection_key("slack", "guard-slack"),
            connection_key("Slack", "guard-slack")
        );
        let held = try_hold_connection("slack", "guard-slack").expect("the sync takes the guard");
        assert!(
            try_hold_connection("slack", "guard-slack").is_none(),
            "the backfill must not run while a Slack sync of this connection is running"
        );
        drop(held);
    }

    /// The engine adapter's tree reconnect has this test
    /// (`engine::sync`'s `composio_sync_document_reaches_memory_tree`); the
    /// engine-free host that replaced it on the live path did not, and drifted
    /// — it wrote a `composio:`-prefixed source id and no `path_scope`, so
    /// every synced item became its own tree under a scope no platform prefix
    /// matches. Chunks existed, recall could not reach them. Asserting the
    /// addressing, not merely the row count, is what catches that.
    #[tokio::test]
    async fn a_synced_document_is_keyed_by_its_connection_scope() {
        use tinymemory_api::host::test_support::TestHostConfig;
        use tinymemory_api::host::MemoryHostConfig;

        crate::test_seams::init();
        let workspace = tempfile::tempdir().expect("workspace");
        let workspace_dir = workspace.path().join("workspace");
        let mut host_config = TestHostConfig::default();
        host_config.workspace_dir = workspace_dir.clone();
        let config = host_config.to_arc();
        let client: MemoryClientRef = Arc::new(
            crate::store::MemoryClient::from_workspace_dir(workspace_dir)
                .expect("memory client initialises against a fresh workspace"),
        );
        let host = PipelineHost::new(client, config.clone());

        // A fresh tree is empty, so a non-zero count after the store is
        // attributable to this sync rather than to pre-existing state.
        assert_eq!(
            crate::store::chunks::store::count_chunks(&*config).expect("count chunks"),
            0,
            "fresh workspace must start with an empty memory tree"
        );

        host.store(SkillDocument {
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

        let scoped = crate::store::chunks::store::list_chunks(
            &*config,
            &crate::store::chunks::store::ListChunksQuery {
                source_id: Some("gmail:conn-1:gmail:msg-1".into()),
                limit: Some(8),
                ..Default::default()
            },
        )
        .expect("list chunks by source id");
        assert!(
            !scoped.is_empty(),
            "ingested chunks must be keyed by `{{toolkit}}:{{connection_id}}:{{document_id}}` — \
             the scheme the memory-source status and diff snapshots query by"
        );
        assert!(
            scoped
                .iter()
                .all(|chunk| chunk.metadata.path_scope.as_deref() == Some("gmail:conn-1")),
            "connector chunks must carry the `{{toolkit}}:{{connection_id}}` tree scope so \
             query_source resolves them (gmail → email)"
        );
        assert!(
            scoped
                .iter()
                .all(|chunk| chunk.metadata.owner == "gmail-sync:conn-1"),
            "connector chunks must be owned by the connection that synced them"
        );
    }

    /// A blank toolkit or connection cannot produce a scope any retrieval kind
    /// matches, so the tree half is skipped rather than writing an unreachable
    /// tree. The skill store, which committed first, still holds the item.
    #[tokio::test]
    async fn an_item_without_a_connection_scope_skips_the_tree_but_not_the_store() {
        use tinymemory_api::host::test_support::TestHostConfig;
        use tinymemory_api::host::MemoryHostConfig;

        crate::test_seams::init();
        let workspace = tempfile::tempdir().expect("workspace");
        let workspace_dir = workspace.path().join("workspace");
        let mut host_config = TestHostConfig::default();
        host_config.workspace_dir = workspace_dir.clone();
        let config = host_config.to_arc();
        let client: MemoryClientRef = Arc::new(
            crate::store::MemoryClient::from_workspace_dir(workspace_dir)
                .expect("memory client initialises against a fresh workspace"),
        );
        let host = PipelineHost::new(client.clone(), config.clone());

        host.store(SkillDocument {
            namespace_skill_id: "gmail".into(),
            connection_id: "   ".into(),
            document_id: "gmail:msg-2".into(),
            title: "No connection".into(),
            content: "This item has no connection scope.".into(),
            toolkit: "gmail".into(),
            metadata: serde_json::Value::Null,
        })
        .await
        .expect("a scopeless item must not fail the sync");

        assert_eq!(
            crate::store::chunks::store::count_chunks(&*config).expect("count chunks"),
            0,
            "a scopeless item must not write a tree no retrieval can reach"
        );
        let stored = client
            .list_documents(Some("skill-gmail"))
            .await
            .expect("list skill documents");
        let documents = stored
            .get("documents")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        assert_eq!(
            documents.len(),
            1,
            "the skill store is the source of truth and must still hold the item"
        );
    }

    /// A pipeline that records whether it was ticked, so the refusal path can
    /// be shown to skip the run rather than to run and discard the result.
    struct RecordingPipeline(Arc<std::sync::atomic::AtomicBool>);

    #[async_trait]
    impl SyncPipeline for RecordingPipeline {
        fn id(&self) -> &str {
            "test:recording"
        }

        fn kind(&self) -> crate::sync::pipelines::traits::SyncPipelineKind {
            crate::sync::pipelines::traits::SyncPipelineKind::Composio
        }

        async fn init(&self, _: &PipelineConfig, _: &SyncContext) -> anyhow::Result<()> {
            Ok(())
        }

        async fn tick(&self, _: &PipelineConfig, _: &SyncContext) -> anyhow::Result<SyncOutcome> {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(SyncOutcome {
                records_ingested: 7,
                ..SyncOutcome::default()
            })
        }
    }

    /// End to end: with the connection held, `run_pipeline` returns the note
    /// without ticking the pipeline — no fetch, no Composio spend, and no
    /// second writer of the connection's `SyncState`.
    #[tokio::test]
    async fn a_held_connection_short_circuits_the_run() {
        crate::test_seams::init();
        let workspace = tempfile::tempdir().expect("workspace");
        let client: MemoryClientRef = Arc::new(
            crate::store::MemoryClient::from_workspace_dir(workspace.path().join("store"))
                .expect("memory client initialises against a fresh workspace"),
        );
        let host = Arc::new(PipelineHost::without_tree_ingest(client));
        let ticked = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let held = try_hold_connection("gmail", "guard-short-circuit")
            .expect("the first run takes the guard");
        let outcome = run_pipeline(
            Arc::new(RecordingPipeline(ticked.clone())),
            "gmail",
            "guard-short-circuit",
            &PipelineConfig::default(),
            &host.context(),
        )
        .await
        .expect("a refused run is not a failure");

        assert_eq!(outcome.note.as_deref(), Some(SYNC_ALREADY_RUNNING));
        assert_eq!(outcome.records_ingested, 0);
        assert!(
            !ticked.load(std::sync::atomic::Ordering::SeqCst),
            "the refused run must not tick the pipeline"
        );

        // Released, the same call runs normally — the guard skips a concurrent
        // run, it does not disable the connection.
        drop(held);
        let outcome = run_pipeline(
            Arc::new(RecordingPipeline(ticked.clone())),
            "gmail",
            "guard-short-circuit",
            &PipelineConfig::default(),
            &host.context(),
        )
        .await
        .expect("the run succeeds once the guard is free");
        assert_eq!(outcome.records_ingested, 7);
        assert!(ticked.load(std::sync::atomic::Ordering::SeqCst));
    }
}
