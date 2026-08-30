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
// This module serves seven of the host's nine seams. Six cross the bus: the
// embedder and the chat model have host interfaces of their own, `composio_host`
// has one too (see `crate::composio`), and the event sink, the error reporter
// and spaCy share `BusRuntimeHost` above. The seventh, `config_loader`, is
// answered locally from the `ModuleConfig` this module was handed, for the
// reason its own module docs give: proxying it would ask the host to re-read a
// config the module already has, and the interesting case is the one where the
// two answers disagree.
//
// The two below are what is left, and both were quiet on every path. With
// nothing installed, `scheduler_gate::current_policy()` reads `Normal` and
// `wait_for_capacity()` returns instantly — background LLM work runs flat out no
// matter what the user asked for — and `shutdown::register` drops the ingest
// queue's lock-release hook behind a `log::debug!`. That is precisely the
// outcome the host's own `install_memory_host_seams` comment says the seams
// exist to prevent: a sync run that looks empty rather than broken. Silence is
// the bug; these stubs are the fix.
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
// the first time anything actually consults it. The queue worker pool now runs
// in here (`crate::start_queue_pool`) and consults both, so the scheduler-gate
// report fires on every boot that drains a job — which is the honest signal that
// the throttle is not in effect.
//
// # The scheduler-gate stub now also silences two user-visible pauses
//
// `crate::start_sync_loops` moved the two periodic sync loops in here as well,
// and they consult this gate for something the queue pool does not: not a
// throttle but a *stop*. Step 0 of every tick in both loops is
// `sync::composio::periodic::periodic_pause_reason`, which exists to honour two
// states — `PauseReason::UserDisabled`, the user switching Memory Tree off in
// Settings, and `PauseReason::SignedOut`, no live session. It reads them off
// `current_policy()`, which is the stub, which always answers `Policy::Normal`.
// So in module mode both loops tick straight through both pauses: a user who
// switched memory off still gets background fetches, and a signed-out user still
// gets a Composio connection walk every 20 minutes.
//
// The per-source `enabled` toggle is unaffected — that is read from the source
// registry inside the tick, not from the gate — so switching off one source
// still works. It is the two *global* pauses that do not.
//
// The same stub's `resume_notify` hands back a `Notify` nobody fires, so the
// other half of that design is gone too: re-enabling sync no longer wakes the
// loops early, and the user waits out the remaining 20-minute tick instead of
// syncing within seconds. That half is benign; the paragraph above is not, and
// closing it needs the same `SchedulerGate` bus interface named below.
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
                             throttle (user toggle, AC power, CPU pressure, signed-out) — and \
                             the periodic sync loops here therefore also ignore the \
                             \"Memory Tree off\" and \"signed out\" pauses that would stop them";

/// What the missing shutdown host costs.
const SHUTDOWN_UNSERVED: &str = "shutdown host unserved in module mode: a memory shutdown hook \
                                 was dropped, so in-flight queue job locks are not released on a \
                                 clean exit and the next launch waits out the lease instead";

/// Log and report a seam degradation once per process.
///
/// Shared with [`crate::composio`] and [`crate::config_loader`], which have
/// their own latches and their own messages but need exactly this behaviour:
/// one `log::error!` unconditionally, one classified report when a runtime
/// exists to send it on, and nothing at all on every later call. Each caller
/// owns its latch so one seam going quiet never silences another.
pub(crate) fn report_unserved_once(
    latch: &AtomicBool,
    message: &'static str,
    operation: &'static str,
) {
    if latch.swap(true, Ordering::SeqCst) {
        return;
    }
    log::error!("[tinymemory:module] {message}");
    // The error reporter reaches the host by spawning onto the module runtime,
    // and several call sites are sync methods a caller could reach from a plain
    // thread — the scheduler-gate stub below and both Composio probes. The log
    // line above is unconditional; only the telemetry needs a runtime to exist,
    // so the gap is never silent even when the report cannot be sent.
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
        "[tinymemory:module] two host seams are unserved in module mode: scheduler_gate and \
         shutdown are stubs that keep the unwired behaviour and report once when consulted. \
         Background-AI throttling, the \"Memory Tree off\" and \"signed out\" pauses on periodic \
         sync, and graceful queue-lock release are not honoured inside this process"
    );
}

#[cfg(test)]
#[path = "host_test.rs"]
mod test;
