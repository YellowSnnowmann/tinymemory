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
// The trait, not only its methods: `composio` is reached as a method on
// `EngineRuntimeConfig` in `composio_sync_can_run`, and without the trait in
// scope rustc points at the struct's `composio_mode` field instead.
use tinymemory_api::host::MemoryHostConfig;
use tinymemory_core::store::MemoryClientRef;

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
    let client: MemoryClientRef = Arc::new(client);

    // After the store, never before: `queue::start` recovers stale locks as its
    // first act, which opens the queue database, and the factory above is what
    // creates the workspace it lives in.
    start_queue_pool(&config);

    // Also after the store, and for a second reason on top of that one: what is
    // published is the client just built, and there is nothing to publish until
    // it exists. The sync loops follow the bind rather than the other way round
    // — every runner in `sync::pipelines::host` opens with
    // `global::client_if_ready()`, so a loop started before this would fail
    // every run.
    if bind_memory_client(&config, &client) {
        start_sync_loops(&config);
    }

    let provider = provider::provider(&config, client);
    service::serve(&connection, Arc::new(provider), config).await
}

/// Publish the store this process just built as the client for its workspace.
///
/// # Why this is a `bind` and not `global::init`
///
/// Everything in `tinymemory_core::sync` resolves its store through
/// `global::client_if_ready()`, which is `None` in this process: the module
/// builds its store through `create_memory_client_with_local_ai` — the only
/// entry point that takes this module's embedding routes, storage provider and
/// workspace — and that factory never touches the global slot.
///
/// The obvious repair, `global::init(workspace)`, is the wrong one and quietly
/// so. It constructs a *second* `MemoryClient` over the same SQLite file, with
/// the host's default routes rather than this module's, and each client owns an
/// ingestion worker: duplicate graph extraction and duplicate embedding work
/// against one store, which `global`'s own comments call out as the hazard its
/// per-workspace cache exists to prevent. `global::bind` publishes the client
/// that already exists instead, into both the global slot and the per-workspace
/// cache, so all three resolution paths converge on it.
///
/// # Which slot this writes
///
/// This module's own. The `cdylib` carries its own compiled copy of
/// `tinymemory-core`, so the slot filled here is the static that *this
/// process's module-side* loops read through `client_if_ready`, and not the one
/// a host still booting an in-process engine fills with `global::init`. That is
/// what makes binding safe to do before that host's engine is deleted: this
/// cannot repoint the host's engine at this client, and the host's `init`
/// cannot make this bind refuse.
///
/// The refusal below therefore means one specific thing — a second
/// `MemoryClient` was built for this workspace *inside this module* — which is
/// the hazard the whole function exists to keep from happening quietly.
///
/// # Returns
///
/// Whether the client is bound. A failure is reported and the caller starts no
/// sync loops: with no client resolvable, every run in both loops would fail on
/// its first line with "memory client is not ready" — a named cause, but a loop
/// that can only fail is not worth the ticks, the Composio list call every 20
/// minutes, or the failed-sync audit rows it would append forever.
fn bind_memory_client(config: &ModuleConfig, client: &MemoryClientRef) -> bool {
    match tinymemory_core::global::bind(config.workspace_dir.clone(), Arc::clone(client)) {
        Ok(_) => true,
        Err(error) => {
            // The path in `error` stays in this module's log, like the factory
            // failure above; nothing here crosses the bus.
            log::error!(
                "[tinymemory:module] could not publish the memory client for this workspace, so \
                 periodic memory sync will not run in this process: {error}"
            );
            false
        }
    }
}

/// Start the engine's two periodic sync loops for this process.
///
/// # Why the module has to own these
///
/// The same reason [`start_queue_pool`] does. Both loops are engine code —
/// `tinymemory_core::sync::composio::periodic` and
/// `tinymemory_core::sync::workspace::periodic` — and until now the only calls
/// to them in any tree were the host's, made against the second, in-process
/// engine the host also booted. A host that deletes that engine, which is the
/// entire point of loading this module, is left with two loops it can no longer
/// start and a memory that stops updating: Composio connections stop pulling
/// mail, issues and documents, and registered repos, folders, RSS feeds and web
/// pages go stale. The sync layer reports "no connections", which is
/// indistinguishable from a user who has none.
///
/// # The host must stop starting them in the same change
///
/// Not "should" — this is the one part the module cannot guard. The `cdylib`
/// carries its own copy of `tinymemory-core`, so the `OnceLock` each loop
/// guards itself with is a *different* static from the host's: a host that
/// still calls `start_periodic_sync` while loading this module gets two pairs
/// of loops, neither of which can see the other, both walking the same source
/// registry into the same store. [`claim_sync_loops`] catches only the
/// in-process case. So the host's call site goes in the same change that
/// deletes the engine it was calling against.
///
/// # What they do not get in module mode
///
/// Stated rather than hidden, in the same terms [`start_queue_pool`] states its
/// own two:
///
/// - **Neither loop honours the scheduler-gate pauses.** Both call
///   `periodic_pause_reason` as step 0 of every tick, precisely so a user who
///   switched Memory Tree off, or who is signed out, gets no background fetch.
///   This module serves no scheduler gate — see the section comment on
///   `host::install_unserved_seams` for why it cannot — and the stub in its
///   place always answers `Policy::Normal`, so `periodic_pause_reason` is always
///   `None` and both loops tick straight through both pauses. The per-source
///   `enabled` toggle still applies; the two *global* pauses do not.
/// - **Their resume wake never fires.** The stub's `resume_notify` hands back a
///   `Notify` nobody signals, so a user who re-enables sync waits out the
///   remaining 20-minute tick instead of syncing within seconds. That is the
///   benign half of the same gap.
///
/// # Backend-mode Composio sync cannot run here, so it is not started
///
/// `composio_config` takes its direct branch on `config.composio().mode` and
/// otherwise needs a backend session bearer, which this module holds no field
/// for and refuses to hold — see `ModuleConfig::composio_mode` and
/// `EngineRuntimeConfig::session_token` for that decision in full. Starting the
/// Composio loop under any other mode would list the user's connections every 20
/// minutes and fail every due one with the same named cause, appending a failed
/// row to the sync audit each time, forever. So it is gated, and the gate says
/// so out loud once instead.
///
/// The workspace loop is started either way: it drives repos, folders, RSS and
/// web pages through `sources::sync::sync_source`, which never touches Composio.
///
/// Both the gate and the pipeline read the load-time snapshot, so a user who
/// switches Composio mode after this module loaded is not picked up until the
/// host reloads it — `config_loader`'s documented staleness, and not new here.
/// The gate itself is [`start_composio_periodic_sync`].
fn start_sync_loops(config: &ModuleConfig) {
    match claim_sync_loops(&config.workspace_dir) {
        WorkspaceClaim::Start => {
            // Warn, not debug: it is true on every boot in module mode, and a
            // reader of the log should not have to know which seams are stubbed
            // to find out that the pauses are not in effect.
            log::warn!(
                "[tinymemory:module] starting the periodic memory sync loops in this process. \
                 They do not honour the scheduler gate — it is unserved here, so the \
                 \"Memory Tree off\" and \"signed out\" pauses are ignored and a re-enable is \
                 not woken early — though each source's own enabled toggle still applies"
            );
            // Workspace sources first, because this one runs in every mode.
            tinymemory_core::sync::workspace::start_workspace_periodic_sync();
            start_composio_periodic_sync(config);
        }
        WorkspaceClaim::AlreadyRunning => {
            log::debug!(
                "[tinymemory:module] the periodic memory sync loops for this workspace are \
                 already running"
            );
        }
        WorkspaceClaim::Foreign => {
            log::error!(
                "[tinymemory:module] the periodic memory sync loops are already running for a \
                 different workspace in this process, and both guard themselves process-wide, \
                 so this store gets no periodic sync: Composio connections and registered \
                 sources will not update. One module process serves one workspace"
            );
        }
    }
}

/// Start the Composio half of [`start_sync_loops`], if this host's mode allows.
///
/// Split out so the gate is one readable decision rather than a conditional
/// buried in a match arm, and so the refusal branch has somewhere to explain
/// itself. The decision itself is [`composio_sync_can_run`].
fn start_composio_periodic_sync(config: &ModuleConfig) {
    if composio_sync_can_run(config) {
        tinymemory_core::sync::composio::start_periodic_sync();
        return;
    }

    log::warn!(
        "[tinymemory:module] periodic Composio sync is NOT started: this host did not resolve \
         Composio to direct mode, and backend mode needs a session bearer this module holds no \
         field for and will not carry. Composio-connected sources will not update in this \
         process until the sync client routes through `ComposioHost::execute`"
    );
}

/// Whether the Composio pipelines can resolve a credential in this process.
///
/// True only for direct mode, which is the whole of the gate: the other branch
/// of `sync::pipelines::host::composio_config` needs a backend session bearer,
/// and `EngineRuntimeConfig::session_token` refuses to answer one by design.
///
/// Asked of the *same* `EngineRuntimeConfig` the loop's own ticks will be handed
/// and through the same `MemoryHostConfig::composio` accessor `composio_config`
/// reads, so the two cannot disagree about which host this is. What is left that
/// could drift is the comparison — this side calls `ComposioMode::is_direct`,
/// the pipeline inlines the same case-insensitive test against
/// `COMPOSIO_MODE_DIRECT` — so a host that spells its mode `"Direct"` is either
/// started and served or neither, never started and then failed on every tick.
///
/// A predicate rather than a condition inside its one caller, for the reason
/// [`claim_workspace`] is one: this is the whole of what is worth asserting, and
/// asserting it through the caller would spawn a real 20-minute tick loop into
/// the test binary.
pub(crate) fn composio_sync_can_run(config: &ModuleConfig) -> bool {
    tinymemory_tinycortex::engine::EngineRuntimeConfig::from(config)
        .composio()
        .is_direct()
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

/// The workspace whose periodic sync loops this process drives.
///
/// A separate cell from [`QUEUE_POOL_WORKSPACE`] because they are separate
/// services that can each be claimed or not, but the trap is identical and so
/// is the reasoning: `start_periodic_sync` and `start_workspace_periodic_sync`
/// each guard themselves with a process-global `OnceLock<()>`, which makes a
/// second call a no-op that is indistinguishable from a first that worked, while
/// what each loop actually syncs is rooted at whatever workspace the installed
/// `config_loader` answers for.
static SYNC_LOOPS_WORKSPACE: OnceLock<PathBuf> = OnceLock::new();

/// What a claim on one of this process's workspace-bound background services
/// found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkspaceClaim {
    /// Nothing had claimed the service; this caller starts it.
    Start,
    /// It is already running for this workspace, so there is nothing to do and
    /// nothing wrong.
    AlreadyRunning,
    /// It is running, but rooted somewhere else. This store cannot be given one
    /// of its own, and goes without.
    Foreign,
}

/// Decide whether this caller is the one that starts `cell`'s service.
///
/// Split out from the two `start_*` functions so each decision can be asserted
/// without spawning real workers and tick loops into a test process, and because
/// the guards inside `tinymemory-core` are not observable from here at all — a
/// second call to any of them is indistinguishable from a first that worked.
fn claim_workspace(cell: &OnceLock<PathBuf>, workspace: &Path) -> WorkspaceClaim {
    match cell.set(workspace.to_path_buf()) {
        Ok(()) => WorkspaceClaim::Start,
        // `set` hands the rejected value back, so the comparison needs no
        // second read and cannot race with a concurrent claim.
        Err(rejected) => {
            if cell.get() == Some(&rejected) {
                WorkspaceClaim::AlreadyRunning
            } else {
                WorkspaceClaim::Foreign
            }
        }
    }
}

/// Claim the queue worker pool for `workspace`. See [`start_queue_pool`].
pub(crate) fn claim_queue_pool(workspace: &Path) -> WorkspaceClaim {
    claim_workspace(&QUEUE_POOL_WORKSPACE, workspace)
}

/// Claim the periodic sync loops for `workspace`. See [`start_sync_loops`].
pub(crate) fn claim_sync_loops(workspace: &Path) -> WorkspaceClaim {
    claim_workspace(&SYNC_LOOPS_WORKSPACE, workspace)
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
        WorkspaceClaim::Start => {
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
        WorkspaceClaim::AlreadyRunning => {
            log::debug!(
                "[tinymemory:module] the queue worker pool for this workspace is already running"
            );
        }
        WorkspaceClaim::Foreign => {
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
        //
        // The two periodic sync loops `setup` also starts do not move the
        // number. They sleep on a 20-minute `interval` and yield across every
        // fetch, so they hold no worker between ticks; the one moment they do is
        // `BusComposioHost::probe`, which blocks its caller for one bus round
        // trip and is bounded at twice per tick — see the note on `probe` for
        // why that bridge blocks at all.
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
