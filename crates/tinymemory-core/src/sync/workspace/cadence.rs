//! When the periodic scheduler should fire, and when it should hold off.
//!
//! # Why this is not with the connector
//!
//! None of it is specific to any source. "How often may this sync run" and
//! "should the scheduler run at all right now" are questions about the user's
//! cadence setting and the machine's pause policy, and the answers are the
//! same whether the source is a mailbox, a folder, or an RSS feed.
//!
//! It lived under the Composio tree because Composio was the first source
//! with a periodic loop. The loop is still here; only the fetching left.

use std::time::Duration;

use crate::scheduler_gate::{current_policy, PauseReason};
use tinymemory_api::host::DEFAULT_MEMORY_SYNC_INTERVAL_SECS;

/// Resolve the effective periodic sync interval (seconds) for one connection,
/// combining the provider's own default with the user's global
/// memory-sync cadence ([`Config::memory_sync_interval_secs`], #3302).
///
/// - `global == Some(0)` → `None`: "Manual only" — the scheduler skips this
///   source entirely (manual sync still works).
/// - `global == Some(n)` → `Some(max(n, provider_default))`: the user's
///   cadence overrides the provider default but is floored at it, so we never
///   sync *more* often than the provider intended.
/// - `global == None` → `Some(max(DEFAULT, provider_default))`: no explicit
///   user choice, so fall back to the 24h default cadence (also floored at the
///   provider default).
pub(crate) fn effective_interval_secs(provider_default: u64, global: Option<u64>) -> Option<u64> {
    match global {
        Some(0) => None,
        Some(n) => Some(n.max(provider_default)),
        None => Some(DEFAULT_MEMORY_SYNC_INTERVAL_SECS.max(provider_default)),
    }
}

/// Decide whether a connection is due for a periodic sync right now, given the
/// effective interval and how long ago it last synced this run.
///
/// `since_last_sync == None` means we have no record of a sync this process
/// lifetime, so we fire immediately (the restart-recovery path). Kept pure so
/// the due-check can be simulated without driving the real `Instant` clock.
pub(crate) fn connection_is_due(interval_secs: u64, since_last_sync: Option<Duration>) -> bool {
    match since_last_sync {
        Some(elapsed) => elapsed >= Duration::from_secs(interval_secs),
        None => true,
    }
}

/// Inspect the scheduler-gate policy and decide whether this tick should
/// fire at all. Returns `Some(reason)` for paused states so the caller can
/// log a single, attributable line instead of doing the work and discovering
/// per-LLM-call later that everything's gated.
///
/// Covers two reasons the memory subsystem treats as "do no background
/// work":
/// - [`PauseReason::UserDisabled`] — user flipped the Memory Tree toggle off
///   in Settings (#1856 Part 1). The 20-min Composio fetch loop honouring
///   this flag is the explicit follow-up listed in the #2719 PR body.
/// - [`PauseReason::SignedOut`] — no live session; periodic work would just
///   401-loop against the backend.
///
/// Other [`PauseReason`] variants:
/// - `OnBattery` / `CpuPressure` (future, per #1073) — intentionally **not**
///   gated here; periodic Composio fetch is network-light, so battery / CPU
///   pressure shouldn't stop the user's data flowing in. Those signals
///   already throttle LLM-bound work through the regular gate.
/// - `Unknown` — documented in `scheduler_gate::policy` as a safe fallback;
///   `Policy::pause_reason()` returns it only when the gate state is in a
///   transitional / not-yet-resolved condition. Letting the tick proceed
///   here keeps periodic sync running through brief transitions instead of
///   pausing on stale unresolved state.
pub(crate) fn periodic_pause_reason() -> Option<PauseReason> {
    // Delegate the `Policy::Paused { .. }` → `PauseReason` extraction to
    // the existing `Policy::pause_reason()` helper (avoids re-implementing
    // the same destructure twice). The allow-list below is the only thing
    // this site has to own — future `PauseReason` variants stay opt-in.
    let reason = current_policy().pause_reason()?;
    matches!(reason, PauseReason::UserDisabled | PauseReason::SignedOut).then_some(reason)
}
