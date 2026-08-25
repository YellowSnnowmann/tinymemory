//! Loadable `TinyBus` module adapter for `TinyMemory`.
//!
//! This private workspace crate keeps the vendored `TinyBus` dependency out of
//! the published `tinymemory` crates. Its `cdylib` output is the
//! target-specific binary distributed in GitHub releases.
//!
//! # What this module is for, stated honestly
//!
//! It carries the memory **engine** — `tinycortex` and `tinymemory-core` — so a
//! host that loads it compiles neither.
//!
//! It is worth being precise about the benefit, because the obvious guess is
//! wrong. This module sheds **no third-party dependencies** from a host. Every
//! crate the engine uses (`rusqlite`, `reqwest`, `chrono`, `regex`, `uuid`,
//! `walkdir`, `sha2`, `tokio`) is shared with surface a host keeps, and
//! `libsqlite3-sys` in particular has several other parents — `tinyagents`'
//! session store among them — so the native `SQLite` build does not leave. That
//! was measured on `OpenHuman`, on both its kernel and its shipping feature
//! profiles: four crate names leave, and all four are ours.
//!
//! What it does buy is **compile time on the critical path**, and that was
//! measured too. `tinycortex` and `tinymemory-core` compile strictly serially
//! ahead of the host crate — `tinyagents` → `tinycortex` → `tinymemory-core` →
//! host, each starting as the previous one ends — putting 14.7s directly in
//! front of the host's own compilation. Removing them from the host's graph
//! moved a full build from 176s to about 161s.
//!
//! Do not re-justify this module on dependency count. The number is zero and it
//! is written down here so nobody re-derives it optimistically.
//!
//! # It carries no credentials
//!
//! The engine needs embeddings, embeddings need an inference credential, and
//! that credential stays in the host. The module asks the host to embed over the
//! bus instead — see [`embedding`], which is the same split the `tinywallet`
//! module makes with a signing key.
//!
//! [`config::ModuleConfig`]'s own fields cannot hold a key, but that is not
//! sufficient on its own and it is worth saying why: it embeds
//! `tinymemory_api::host::MemoryConfig` **verbatim**, and that struct contains
//! `agentmemory_secret`, a bearer token for a remote memory backend. So the
//! property is *enforced* at setup by
//! [`config::ModuleConfig::strip_host_credentials`], not merely asserted about a
//! field list. "Carried verbatim" carries credentials verbatim too.
//!
//! The claim is about *configuration*, and there is exactly one place it stops
//! there: [`composio`]'s `ApiKey` fetches the user's direct-mode Composio key
//! from the host for the duration of one call. It is stated here rather than
//! buried because the difference matters — the engine's `composio_config`
//! builds its own HTTP client from that key, so unlike an embed there is no
//! host-side call to route the work through, and refusing it would mean
//! direct-mode memory sync simply cannot run. Nothing stores it; there is still
//! no field it could be stored in.
//!
//! # Scope: the complete TinyMemory API
//!
//! The module boundary mirrors every capability family in `tinymemory_api`.
//! Host applications keep policy, scheduling, credentials, and bus/event types;
//! memory storage, retrieval, ingestion, trees, graph operations, goals, source
//! persistence, and maintenance execute inside this compiled module.

// Test code may panic; library code may not. The `[lints]` table cannot be
// scoped to non-test builds, so the exemption is expressed here instead.
#![cfg_attr(
    test,
    allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        clippy::cast_precision_loss
    )
)]

pub mod chat;
pub mod composio;
pub mod config;
pub mod config_loader;
pub mod embedding;
mod host;
mod provider;
mod service;

pub use chat::{CHAT_HOST_BUS_NAME, CHAT_HOST_INTERFACE, CHAT_HOST_OBJECT_PATH};
pub use composio::{
    BusComposioHost, API_KEY_METHOD, COMPOSIO_HOST_BUS_NAME, COMPOSIO_HOST_INTERFACE,
    COMPOSIO_HOST_OBJECT_PATH, EXECUTE_METHOD, IS_AVAILABLE_METHOD, LIST_CONNECTIONS_METHOD,
};
pub use config::ModuleConfig;
pub use config_loader::ModuleConfigLoader;
pub use embedding::{
    BusEmbeddingHost, BusEmbeddingProvider, EMBEDDING_HOST_BUS_NAME, EMBEDDING_HOST_INTERFACE,
    EMBEDDING_HOST_OBJECT_PATH,
};
pub use host::{RUNTIME_HOST_BUS_NAME, RUNTIME_HOST_INTERFACE, RUNTIME_HOST_OBJECT_PATH};
pub use service::{BUS_NAME, OBJECT_PATH};

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use tinybus::{Connection, Error as BusError, Result as BusResult};

/// The module refused its configuration or could not bring up a store.
const SETUP_FAILED_ERROR: &str = "ai.tinyhumans.tinymemory.Error.SetupFailed";

/// Bring up the engine and serve it.
///
/// # Order matters
///
/// The bus-backed [`BusEmbeddingHost`] is installed **before** the engine is
/// constructed. `tinymemory-core` resolves its embedder through a process-global
/// during construction, and a store built before the host is installed would
/// either fail or — worse — bind the inert zero-dimension provider and write
/// vectors nobody can search. The global is why this is a `set` and not an
/// argument: the construction sites sit deep inside retrieval and sealing call
/// stacks that already thread a config and a store handle.
///
/// # The empty API key is deliberate
///
/// `create_memory_with_local_ai` is handed `""`. Every embed goes over the bus to
/// the host, which holds the real credential, so there is nothing to pass and
/// nothing here that could leak one.
async fn setup(connection: Connection, mut config: ModuleConfig) -> BusResult<()> {
    config.validate().map_err(setup_error)?;
    claim_process_setup()?;

    // `MemoryConfig` travels verbatim, and it contains a bearer token field for a
    // remote memory backend. Carried credentials are exactly what this module
    // refuses to hold, so it goes before anything else touches the config.
    if config.strip_host_credentials() {
        log::warn!(
            "[tinymemory:module] discarded a remote-backend credential from the \
             supplied config; this module serves the local engine only, so bind a \
             remote memory driver directly instead of through it"
        );
    }

    log::debug!(
        "[tinymemory:module] setup driver_id={} routes={} cloud_dims={}",
        config.driver_id,
        config.embedding_routes.len(),
        config.cloud_embedding_dimensions
    );

    // Install the embedder first. See the doc comment.
    tinymemory_core::embedding_host::set_embedding_host(Arc::new(BusEmbeddingHost::new(
        connection.clone(),
        &config,
    )));
    tinymemory_core::chat_host::set_chat_host(Arc::new(chat::BusChatHost::new(
        connection.clone(),
        &config,
    )));
    // Composio is host state end to end — the connection list, the direct key,
    // and whether any client resolves at all change on an OAuth completion or a
    // `set_api_key` RPC with nothing restarting — so this one is a proxy and
    // holds no snapshot. See `composio` for why the direct-mode key is the one
    // credential that does cross.
    tinymemory_core::composio_host::set_composio_host(Arc::new(composio::BusComposioHost::new(
        connection.clone(),
    )));
    // The config loader is the opposite call, and deliberately: it is answered
    // from `config` — which is this line's whole argument — rather than asking
    // the host to re-read what it already handed over. It goes *after* the
    // credential strip above, because this is the seam that hands the config
    // back out to the engine repeatedly.
    tinymemory_core::config_loader::set_config_loader(Arc::new(ModuleConfigLoader::new(&config)));
    host::install(connection.clone());
    // The two seams no bus interface serves, and no local answer can honestly
    // stand in for. Both degraded in silence rather than with a named cause;
    // see the section comment on `host::install_unserved_seams` for why they
    // are stubbed here rather than proxied or synthesised. Installed with the
    // rest, before the store exists, so nothing can consult a seam this process
    // has not yet decided about.
    host::install_unserved_seams();

    let client = tinymemory_core::store::factories::create_memory_client_with_local_ai(
        &config.memory,
        None,
        "",
        &config.embedding_routes,
        config.storage_provider.as_ref(),
        &config.workspace_dir,
    )
    .map_err(|error| {
        // The factory error names the workspace directory it failed under, and
        // a `MethodFailed.message` crosses the bus to a caller that has no
        // business learning this process's filesystem layout. The detail stays
        // in the module's own log; the wire gets the stage only.
        log::error!("[tinymemory:module] create memory store failed: {error}");
        setup_error("create memory store")
    })?;

    // After the store, never before: `queue::start` recovers stale locks as its
    // first act, which opens the queue database, and the factory above is what
    // creates the workspace it lives in.
    start_queue_pool(&config);

    // ── The periodic sync loops are deliberately NOT started here ───────────
    //
    // This is the obvious next line to write — the queue pool moved in here for
    // exactly the reason the sync loops would, and `composio_host` and
    // `config_loader` are now installed above, which is what a reader would
    // check first. It does not work yet, and it would fail *quietly*, so the
    // reasons are written down rather than left to be rediscovered.
    //
    // `tinymemory_core::sync::composio::start_periodic_sync` dispatches through
    // `sync::pipelines::host::run_composio_connection_with_caps`, and three
    // separate things in that path have no answer in this process:
    //
    // 1. **The pipeline reads credentials off the `Config`, not off the seam.**
    //    `composio_config` takes the direct-mode branch only when
    //    `config.composio().mode == "direct"` and otherwise needs
    //    `config.session_token()`. `EngineRuntimeConfig` answers
    //    `ComposioMode::default()` (mode `""`) and `Ok(None)`, so backend mode
    //    fails with "backend bearer token is not configured" and direct mode is
    //    never selected at all. `ComposioHost::api_key` cannot rescue this: the
    //    seam is consulted *inside* the direct branch that is not taken. The
    //    real fix is to route the pipeline's own HTTP client through
    //    `ComposioHost::execute`, which is a change to the engine's contract.
    //
    // 2. **`crate::global::client_if_ready()` is `None` here.** That is the
    //    first line of every pipeline run. This module builds its store through
    //    `create_memory_client_with_local_ai`, which does not touch the global
    //    slot, and calling `global::init` would build a *second* client via
    //    `MemoryClient::from_workspace_dir` — different embedding routes, a
    //    second ingestion worker over the same SQLite file.
    //
    // 3. **The cadence reads as "manual only".**
    //    `EngineRuntimeConfig::memory_sync_interval_secs()` is `Some(0)`, which
    //    the contract defines as manual-only, so both loops would skip every
    //    source on every tick. This is the one that would be invisible: no
    //    error, no warning, just a sync that never fires. See
    //    `config_loader`'s module docs for why the loader does not invent a
    //    different number.
    //
    // A fourth consequence is worth knowing even once those are fixed: this
    // module's scheduler gate is a stub that always reads `Normal`, so a sync
    // loop in here would not honour the "signed out" and "user disabled" pauses
    // that `periodic_pause_reason` exists to apply.

    let provider = provider::provider(&config, Arc::new(client));
    service::serve(&connection, Arc::new(provider), config).await
}

/// The workspace whose queue this process's worker pool drains.
///
/// The pool is bound to one workspace — every `queue::store` entry point
/// resolves its database through `engine_config`, which roots at
/// `config.workspace_dir()` — while the `Once` inside `queue::start` is
/// process-global. Those two facts together are the trap this cell exists for:
/// a second `start` under a different workspace is not a second pool, it is a
/// silent no-op leaving that store's queue with nothing draining it. Recording
/// which workspace won makes that case loud instead of invisible.
static QUEUE_POOL_WORKSPACE: OnceLock<PathBuf> = OnceLock::new();

/// What [`claim_queue_pool`] found when asked to start a pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueuePoolClaim {
    /// Nothing had claimed the pool; this caller starts it.
    Start,
    /// A pool is already draining this workspace's queue, so there is nothing
    /// to do and nothing wrong.
    AlreadyDraining,
    /// A pool is running, but rooted somewhere else. This store's queue has
    /// nothing draining it and cannot be given a pool of its own.
    Foreign,
}

/// Decide whether this caller is the one that starts the pool.
///
/// Split out from [`start_queue_pool`] so the decision can be asserted without
/// spawning four job workers and a daily scheduler into a test process, and
/// because `queue::start`'s own `Once` is not observable from here at all — a
/// second call to it is indistinguishable from a first that worked.
pub(crate) fn claim_queue_pool(workspace: &Path) -> QueuePoolClaim {
    match QUEUE_POOL_WORKSPACE.set(workspace.to_path_buf()) {
        Ok(()) => QueuePoolClaim::Start,
        // `set` hands the rejected value back, so the comparison needs no
        // second read and cannot race with a concurrent claim.
        Err(rejected) => {
            if QUEUE_POOL_WORKSPACE.get() == Some(&rejected) {
                QueuePoolClaim::AlreadyDraining
            } else {
                QueuePoolClaim::Foreign
            }
        }
    }
}

/// Start the engine's queue worker pool for this process.
///
/// # Why the module has to own this
///
/// Every enqueue this driver makes is inert without a pool draining it, and the
/// enqueues are not incidental: `FlushPending` and `RetryFailed` schedule work
/// rather than doing it, the re-embed backfill is a queued job, and the ingest
/// path's `extract_chunk` is *how ingested content becomes retrievable at all*.
/// Until now the only `queue::start` call in any tree was the host's, made
/// against the second, in-process engine the host also booted. A host that
/// deletes that engine — which is the entire point of loading this module —
/// turns all four into permanent no-ops with no error anywhere: ingestion still
/// reports success and the content is simply never indexed. So the pool moves
/// in here, alongside the engine that needs it.
///
/// # Two things it does not get in module mode
///
/// Stated rather than hidden, because this is a real product degradation the
/// host does not have today. The pool consults
/// [`tinymemory_core::scheduler_gate`] before every claim and registers a
/// [`tinymemory_core::shutdown`] hook to release in-flight job locks. This
/// module serves neither seam — see the section comment on
/// `host::install_unserved_seams` for why neither can be proxied — so both are
/// stubs, and the consequences follow:
///
/// - **It runs unthrottled.** `wait_for_capacity` returns immediately, so
///   background memory work in this process ignores the host's background-AI
///   throttle: the user's toggle, AC power, CPU pressure, signed-out. On a
///   laptop that means the queue drains at full tilt on battery, which the
///   host's in-process engine would not do.
/// - **Its shutdown hook is dropped.** A clean exit therefore leaves `running`
///   rows locked. They are reclaimed by lease expiry at the next start —
///   `recover_stale_locks` is the first thing `queue::start` does, and
///   `queue::worker` documents that as the hard-kill path — so the cost is one
///   lease of latency after a restart, not lost work.
///
/// Closing either properly needs a `SchedulerGate` bus interface this crate
/// owns only one half of, which is separate work. Until then the stubs report
/// once per process the first time the pool consults them.
fn start_queue_pool(config: &ModuleConfig) {
    match claim_queue_pool(&config.workspace_dir) {
        QueuePoolClaim::Start => {
            // Warn, not debug: it is true on every boot in module mode, and a
            // reader of the log should not have to know which seams are stubbed
            // to find out that the throttle is not in effect.
            log::warn!(
                "[tinymemory:module] starting the memory queue worker pool in this process. \
                 It runs unthrottled — the scheduler gate is unserved here, so background \
                 memory work ignores the host's background-AI throttle, AC power and CPU \
                 pressure — and its graceful lock-release hook is dropped, so locks held at \
                 exit are reclaimed by lease expiry on the next start"
            );
            tinymemory_core::queue::start(Arc::new(
                tinymemory_tinycortex::engine::EngineRuntimeConfig::from(config),
            ));
        }
        QueuePoolClaim::AlreadyDraining => {
            log::debug!(
                "[tinymemory:module] the queue worker pool for this workspace is already running"
            );
        }
        QueuePoolClaim::Foreign => {
            log::error!(
                "[tinymemory:module] a queue worker pool is already running for a different \
                 workspace in this process, and `queue::start` is guarded process-wide, so the \
                 store just opened has nothing draining its queue: ingested content will not be \
                 indexed and flushes and retries will not run. One module process serves one \
                 workspace"
            );
        }
    }
}

/// Claim this process's single setup slot.
///
/// `setup` installs **process-global** host callbacks, so it is not
/// re-entrant the way a per-host resource would be. `ModuleHost` rejects a
/// duplicate module name only within one host, and nothing stops a process from
/// building a second host — a test harness is the obvious way it happens. The
/// second `setup` would replace the global embedder while stores built by the
/// first keep the `BusEmbeddingProvider` they captured, so embeds would be split
/// across two connections with no error anywhere.
///
/// Refusing the second setup is the honest outcome: one process serves this
/// module once. tinybus never unloads a library, so there is no release path to
/// pair with this and no state to reset.
///
/// # Errors
///
/// [`SETUP_FAILED_ERROR`], when this process has already run setup.
fn claim_process_setup() -> BusResult<()> {
    static CLAIMED: AtomicBool = AtomicBool::new(false);

    if CLAIMED.swap(true, Ordering::SeqCst) {
        return Err(setup_error(
            "this module is already set up in this process; it installs a \
             process-global host callbacks and cannot be served twice",
        ));
    }
    Ok(())
}

/// A setup failure, carrying no path and no credential.
fn setup_error(message: impl Into<String>) -> BusError {
    BusError::MethodFailed {
        name: SETUP_FAILED_ERROR.to_string(),
        message: message.into(),
    }
}

// Isolate the generated public C symbols so the lint exception cannot hide
// undocumented Rust API. Their contract is TinyBus ABI v1, and none is a
// Rust-callable export from this crate.
#[allow(
    missing_docs,
    unreachable_pub,
    reason = "generated C ABI symbols are documented by the TinyBus module SDK"
)]
mod exports {
    tinybus_module::module_export! {
        setup = super::setup,
        config = super::ModuleConfig,
        // Eight, derived rather than picked. Two are the floor this module has
        // always needed: a recall that triggers an embed makes an outbound call
        // while still inside its own inbound call, so a single worker would
        // deadlock on the first semantic query. `setup` now also starts the
        // engine's queue pool — four job workers plus the daily scheduler — and
        // those five run the engine's SQLite claim and settle synchronously
        // inside their async loops, so a busy one occupies a runtime thread
        // outright instead of yielding it. Two plus five is seven; the eighth
        // is what drives a job's own outbound embed while the rest are busy. At
        // two, a draining queue would starve inbound dispatch and the module
        // would stop answering recalls until the queue emptied.
        worker_threads = 8,
        provides = ["ai.tinyhumans.tinymemory.Memory"],
        methods = [
            "DriverId",
            "Capabilities",
            "Health",
            "Shutdown",
            "OpenStore",
            "InsertTurn",
            "SessionTurns",
            "OpenSegment",
            "CreateSegment",
            "AppendTurn",
            "CloseSegment",
            "SetSegmentSummary",
            "UpsertSegmentEmbedding",
            "InsertEvent",
            "Store",
            "Get",
            "Forget",
            "List",
            "Namespaces",
            "Recall",
            "ExportPage",
            "ImportRecords",
            // People.
            "ListPeople",
            "GetPerson",
            "ResolveHandle",
            "AddHandleAlias",
            "ScorePerson",
            "RecordInteraction",
            "SeedFromAddressBook",
            // Chunks.
            "ListChunks",
            "GetChunk",
            "ChunkDetail",
            "StorageKinds",
            "ChunkEmbeddings",
            "CountChunks",
            "ListChunkDetails",
            "SourceTotals",
            // Retrieval.
            "FastRetrieve",
            "CoverWindow",
            "RetrieveSource",
            "RetrieveChildren",
            "RetrieveLeaves",
            "RecallNamespaceScored",
            "SearchEntities",
            // Profile.
            "ListActiveFacets",
            "ListAllFacets",
            "GetFacet",
            "FacetsByType",
            "UpsertFacet",
            "UpsertProviderFacet",
            "SetFacetUserState",
            "DeleteFacet",
            "DeleteFacetById",
            "DropFacetsBelow",
            "WorkflowIdentityMatches",
            "IngestDocument",
            "IngestChat",
            "IngestEmail",
            "PutDocument",
            "GetDocument",
            "ListDocuments",
            "ListNamespaces",
            "DeleteDocument",
            "ClearNamespace",
            "QueryDocuments",
            // Predates the five families this port added; it was implemented
            // but never declared, so it was unreachable over the bus too.
            "RecallDocuments",
            "Append",
            "QuerySource",
            "DrillDown",
            "Seal",
            "Cascade",
            "Entities",
            "EntityEdges",
            "TouchEntities",
            "TopEntities",
            "ChunkEntities",
            "EntityChunkIds",
            "KvGet",
            "KvPut",
            "KvDelete",
            "KvList",
            "Relations",
            "PutRelation",
            "CaptureSnapshot",
            "Snapshots",
            "Diff",
            "Goals",
            "SetGoals",
            "ToolRules",
            "PutToolRule",
            "DeleteToolRule",
            "AcceptSourceItems",
            "ForgetSource",
            "ForgetMatching",
            "Reembed",
            "Compact",
            "Consolidate",
            "Doctor",
            "RetryFailed",
            "StoreStats",
            "QueueStats",
            "LatestQueueFailure",
            "BackfillInProgress",
            "FlushPending",
            "ResetDerivedIndex",
            "PurgeAll",
            "RecallNamespaceRecent",
            // Tree, structural: the forest walk and its leaf edge.
            "SummaryForest",
            "RecentLeaves",
        ],
        signals = [],
        // The host's embedder is deliberately NOT declared as `requires`. That
        // field is resolved against already-loaded *modules*, and this dependency
        // is served by the host itself, which would leave the module permanently
        // unresolved. It is dialled lazily on the first embed instead, and a host
        // that has not served it gets a named error rather than a module that
        // never starts.
        requires = [],
        optional = [],
        // Eager: bringing up a store opens a database and may run migrations,
        // and charging that to whichever call happens to be first would make an
        // ordinary recall time out on a cold start.
        lazy = false,
    }
}
