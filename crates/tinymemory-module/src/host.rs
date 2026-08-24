//! Host-owned runtime services used by the compiled memory engine.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use tinybus::Connection;
use tinymemory_api::host::{ErrorReporter, MemoryEvent, MemoryEventSink, SpacyResponse};

/// Host callback routing constants.
pub const RUNTIME_HOST_BUS_NAME: &str = "ai.tinyhumans.tinymemory.RuntimeHost";
/// Object path for the host callbacks.
pub const RUNTIME_HOST_OBJECT_PATH: &str = "/ai/tinyhumans/tinymemory/RuntimeHost";
/// Interface exported by the host.
pub const RUNTIME_HOST_INTERFACE: &str = "ai.tinyhumans.tinymemory.RuntimeHost";

#[derive(Clone)]
pub(crate) struct BusRuntimeHost {
    connection: Connection,
}

impl std::fmt::Debug for BusRuntimeHost {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BusRuntimeHost")
            .finish_non_exhaustive()
    }
}

impl BusRuntimeHost {
    pub(crate) fn new(connection: Connection) -> Self {
        Self { connection }
    }

    fn proxy(&self) -> Result<tinybus::Proxy, tinybus::Error> {
        self.connection.proxy(
            RUNTIME_HOST_BUS_NAME,
            RUNTIME_HOST_OBJECT_PATH,
            RUNTIME_HOST_INTERFACE,
        )
    }

    fn notify<T>(&self, method: &'static str, arguments: T)
    where
        T: serde::Serialize + Send + 'static,
    {
        let host = self.clone();
        tokio::spawn(async move {
            let result = match host.proxy() {
                Ok(proxy) => proxy.call::<()>(method, arguments).await,
                Err(error) => Err(error),
            };
            if let Err(error) = result {
                log::debug!("[tinymemory:module] host callback {method} failed: {error}");
            }
        });
    }
}

impl MemoryEventSink for BusRuntimeHost {
    fn publish(&self, event: MemoryEvent) {
        self.notify("PublishEvent", (event,));
    }
}

impl ErrorReporter for BusRuntimeHost {
    fn report_error(&self, rendered: &str, domain: &str, operation: &str, tags: &[(&str, &str)]) {
        self.notify(
            "ReportError",
            (
                false,
                rendered.to_string(),
                domain.to_string(),
                operation.to_string(),
                owned_tags(tags),
            ),
        );
    }

    fn report_error_or_expected(
        &self,
        rendered: &str,
        domain: &str,
        operation: &str,
        tags: &[(&str, &str)],
    ) {
        self.notify(
            "ReportError",
            (
                true,
                rendered.to_string(),
                domain.to_string(),
                operation.to_string(),
                owned_tags(tags),
            ),
        );
    }
}

fn owned_tags(tags: &[(&str, &str)]) -> Vec<(String, String)> {
    tags.iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

#[async_trait]
impl tinymemory_core::nlp_host::NlpHost for BusRuntimeHost {
    async fn extract_spacy(
        &self,
        _config: &tinymemory_core::Config,
        text: &str,
    ) -> Result<SpacyResponse, String> {
        let proxy = self.proxy().map_err(|error| error.to_string())?;
        proxy
            .call("ExtractSpacy", (text.to_string(),))
            .await
            .map_err(|error| error.to_string())
    }
}

pub(crate) fn install(connection: Connection) {
    let host = Arc::new(BusRuntimeHost::new(connection));
    tinymemory_core::events::set_event_sink(Arc::clone(&host) as Arc<dyn MemoryEventSink>);
    tinymemory_core::observability::set_error_reporter(Arc::clone(&host) as Arc<dyn ErrorReporter>);
    tinymemory_core::nlp_host::set_nlp_host(host);
}

// ── Seams no bus interface serves ───────────────────────────────────────────
//
// This module serves five of the host's seams: the embedder and the chat model
// have host interfaces of their own, and the event sink, the error reporter and
// spaCy cross the bus through `BusRuntimeHost` above. The other four —
// `composio_host`, `config_loader`, and the two stubbed below — have no callback
// channel at all.
//
// `config_loader` survives that honestly: every path returns
// `Err("no ConfigLoader installed …")`, so a module-mode loop fails with a named
// cause instead of quietly keeping a stale snapshot. `composio_host` is loud on
// the two paths that do work — `list_connections` and `execute` — and quiet on
// its two probes: unwired, `api_key` reads `None` and `is_available` reads
// `false`, which a caller cannot tell apart from "the user has no Composio
// connections".
//
// **The two below were quiet on every path.** With nothing installed,
// `scheduler_gate::current_policy()` reads `Normal` and `wait_for_capacity()`
// returns instantly — background LLM work runs flat out no matter what the user
// asked for — and `shutdown::register` drops the ingest queue's lock-release
// hook behind a `log::debug!`. That is precisely the outcome the host's own
// `install_memory_host_seams` comment says the seams exist to prevent: a sync
// run that looks empty rather than broken. Silence is the bug; these stubs are
// the fix. The two `composio_host` probes are the same bug one size down, and
// are left alone here only because closing them means inventing a bus interface
// this crate owns one half of.
//
// # Why stubs and not bus proxies
//
// Not a surface-budget choice — the trait shapes rule a proxy out.
// `SchedulerGate::current_policy` is sync and a bus call is not; `resume_notify`
// hands back a `tokio::sync::Notify`, a runtime primitive with no wire form; and
// `ShutdownHost::register` takes a Rust closure, which cannot be serialised at
// all. Mirroring the policy locally would need a signal this module does not
// declare (`signals = []`), a host half this crate does not own, and a cache
// that is wrong between ticks.
//
// # Why not synthesise a policy from the module's own config
//
// `ModuleConfig` carries `SchedulerGateConfig`, so `mode = off` looks
// answerable from here. It is not. The gate is *live* state — the user's toggle,
// AC power, CPU pressure, whether anyone is signed in — and the module holds the
// config it was loaded with. A module that paused itself on a stale `off` would
// stay paused after the user switched background AI back on, with no channel to
// learn otherwise and no way out short of a restart. Guessing is worse than not
// answering.
//
// # So: identical behaviour, no longer silent
//
// Each stub returns exactly what the unwired path returned, so installing them
// changes no scheduling and wedges nothing, and each reports once per process
// the first time anything actually consults it. Nothing in this process consults
// them today — the module starts none of the background loops (`queue::start`,
// `start_periodic_sync` and `start_workspace_periodic_sync` are all called
// host-side) — so the report fires on the day someone moves one in here, which
// is the day the gap stops being theoretical.
//
// Note also what is deliberately *not* built here: a module-local registry that
// banked shutdown hooks and drained them on the module's own `Shutdown` method.
// Nothing calls `MemoryProvider::shutdown()` on the way out of the host process,
// so those hooks would still never run — and they would stop reporting. That
// trades a loud gap for a quiet one.

/// Latched so the gap is reported once per process, not once per job claim —
/// `wait_for_capacity` is consulted before every claim, and an unlatched report
/// would page on every poll. Same guard `queue::worker` puts on its own
/// storage-failure reports.
static GATE_REPORTED: AtomicBool = AtomicBool::new(false);

/// Latched for the same reason: one hook dropped means every later one is too.
static SHUTDOWN_REPORTED: AtomicBool = AtomicBool::new(false);

/// What the missing scheduler gate costs, in the terms a reader of the log needs.
const GATE_UNSERVED: &str = "scheduler gate unserved in module mode: background memory work in \
                             this process runs ungated, ignoring the host's background-AI \
                             throttle (user toggle, AC power, CPU pressure, signed-out)";

/// What the missing shutdown host costs.
const SHUTDOWN_UNSERVED: &str = "shutdown host unserved in module mode: a memory shutdown hook \
                                 was dropped, so in-flight queue job locks are not released on a \
                                 clean exit and the next launch waits out the lease instead";

/// Log and report an unserved seam once per process.
fn report_unserved_once(latch: &AtomicBool, message: &'static str, operation: &'static str) {
    if latch.swap(true, Ordering::SeqCst) {
        return;
    }
    log::error!("[tinymemory:module] {message}");
    // The error reporter reaches the host by spawning onto the module runtime,
    // and two of the three call sites below are sync methods a future caller
    // could reach from a plain thread. The log line above is unconditional;
    // only the telemetry needs a runtime to exist, so the gap is never silent
    // even when the report cannot be sent.
    if tokio::runtime::Handle::try_current().is_ok() {
        tinymemory_core::observability::report_error_or_expected(
            message,
            "memory",
            operation,
            &[("mode", "module")],
        );
    }
}

/// The host's background-AI throttle, which this module cannot observe.
///
/// Answers exactly what an uninstalled gate answered — see the section comment
/// above for why it must not answer anything else — and says so out loud the
/// first time it is asked.
#[derive(Debug)]
pub(crate) struct UnservedSchedulerGate;

#[async_trait]
impl tinymemory_core::scheduler_gate::SchedulerGate for UnservedSchedulerGate {
    fn current_policy(&self) -> tinymemory_core::scheduler_gate::Policy {
        report_unserved_once(&GATE_REPORTED, GATE_UNSERVED, "scheduler_gate");
        tinymemory_core::scheduler_gate::Policy::Normal
    }

    fn resume_notify(&self) -> Arc<tokio::sync::Notify> {
        report_unserved_once(&GATE_REPORTED, GATE_UNSERVED, "scheduler_gate");
        Arc::clone(IDLE_NOTIFY.get_or_init(|| Arc::new(tokio::sync::Notify::new())))
    }

    async fn wait_for_capacity(&self) -> Option<Box<dyn Send>> {
        report_unserved_once(&GATE_REPORTED, GATE_UNSERVED, "scheduler_gate");
        None
    }
}

/// A `Notify` nobody ever fires.
///
/// A `select!` on it simply never takes that arm, so the queue loops fall back
/// on their own tick cadence — which is what they did with no gate installed at
/// all. One per process rather than one per call, because every caller has to
/// receive the same handle for a wait on it to mean anything.
static IDLE_NOTIFY: std::sync::OnceLock<Arc<tokio::sync::Notify>> = std::sync::OnceLock::new();

/// The host's shutdown sequencer, which this module has no way to reach.
#[derive(Debug)]
pub(crate) struct UnservedShutdownHost;

impl tinymemory_core::shutdown::ShutdownHost for UnservedShutdownHost {
    fn register(&self, hook: tinymemory_core::shutdown::ShutdownHook) {
        // Dropped, not banked: there is no moment inside this module at which it
        // could be awaited. The hard-kill path is what remains — leases expire
        // and startup recovery reclaims them — so this is a degradation, not a
        // loss of data, and the report is classified accordingly.
        drop(hook);
        report_unserved_once(&SHUTDOWN_REPORTED, SHUTDOWN_UNSERVED, "shutdown_host");
    }
}

/// Install the two seams this module can only stub, and name the gap at setup.
///
/// Kept separate from [`install`] on purpose: that function wires the seams the
/// host genuinely serves over the bus, and folding these in would blur the
/// difference between "wired" and "wired to nothing".
pub(crate) fn install_unserved_seams() {
    tinymemory_core::scheduler_gate::set_scheduler_gate(Arc::new(UnservedSchedulerGate));
    tinymemory_core::shutdown::set_shutdown_host(Arc::new(UnservedShutdownHost));
    // One line, once per process — `setup` runs exactly once. It is a warning
    // rather than a debug line because in module mode this is true on every
    // boot, and a reader of the log should not have to diff seam lists to find
    // out that the throttle and the graceful lock release are not in effect.
    log::warn!(
        "[tinymemory:module] four host seams are unserved in module mode: composio_host and \
         config_loader are absent and their work paths fail with a named cause; scheduler_gate \
         and shutdown are stubs that keep the unwired behaviour and report once when consulted. \
         Background-AI throttling and graceful queue-lock release are not honoured inside this \
         process"
    );
}

#[cfg(test)]
#[path = "host_test.rs"]
mod test;
