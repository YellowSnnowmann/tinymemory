//! What one provider sync run was for, what it cost, and what it produced.
//!
//! Three shapes, read in three different places: [`SyncReason`] is an *input* a
//! provider branches on (backfill everything, or pull since the cursor),
//! [`ComposioUsage`] is a running tally the execute chokepoint accumulates, and
//! [`SyncOutcome`] is the *report* a finished run hands back for the status
//! panel and the sync audit log.
//!
//! All three are serde shapes with no behaviour beyond arithmetic. The run
//! itself — the HTTP calls, the ingestion, the audit-log write — is the engine
//! crate's.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// Reason a sync was triggered. Providers use this to decide whether to do a
/// full backfill or an incremental pull.
///
/// The serde form is `snake_case` and is mirrored into audit rows, so the
/// variant names are a compatibility surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncReason {
    /// First sync immediately after an OAuth handoff completes.
    ConnectionCreated,
    /// Periodic background sync from the scheduler.
    Periodic,
    /// Explicit user-driven sync from RPC or the UI.
    Manual,
}

impl SyncReason {
    /// Stable lowercase tag, matching the serde representation.
    ///
    /// Callers that stamp the reason into a log line or an audit row want the
    /// string without a serde round-trip; this is that string, and the pin test
    /// holds the two forms equal.
    pub fn as_str(&self) -> &'static str {
        match self {
            SyncReason::ConnectionCreated => "connection_created",
            SyncReason::Periodic => "periodic",
            SyncReason::Manual => "manual",
        }
    }
}

/// Result of a provider sync run. Read by the sync status panel and written to
/// the sync audit log.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncOutcome {
    /// Composio toolkit slug the run covered.
    pub toolkit: String,
    /// The connection that was synced; `None` for toolkit-wide runs.
    pub connection_id: Option<String>,
    /// Why the run happened — normally a [`SyncReason::as_str`] tag, kept as a
    /// `String` because a caller may report a reason the enum does not model.
    pub reason: String,
    /// How many items the run ingested.
    pub items_ingested: usize,
    /// Wall-clock start, epoch milliseconds.
    pub started_at_ms: u64,
    /// Wall-clock finish, epoch milliseconds.
    pub finished_at_ms: u64,
    /// One-line human summary for the status panel.
    pub summary: String,
    /// Provider-specific extras (raw JSON object).
    #[serde(default)]
    pub details: serde_json::Value,
}

impl SyncOutcome {
    /// How long the run took, in milliseconds.
    ///
    /// Saturating rather than panicking: the two timestamps come from separate
    /// clock reads and a backwards system-clock adjustment between them would
    /// otherwise take a status panel down over a cosmetic number.
    pub fn elapsed_ms(&self) -> u64 {
        self.finished_at_ms.saturating_sub(self.started_at_ms)
    }
}

/// Per-sync accumulator for Composio billable-action usage.
///
/// Lives behind a shared handle on the provider context so the single `execute`
/// chokepoint can tally every action a provider fires during one run, whichever
/// provider it is and however many pages it paginates. The finished tally is
/// reported alongside the [`SyncOutcome`] for the sync audit log.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComposioUsage {
    /// Count of `execute` calls that returned a response this run.
    ///
    /// A provider-reported failure still counts — it reached Composio and was
    /// billed. Transport errors do not.
    pub actions_called: u32,
    /// Sum of each response's backend-reported `cost_usd`.
    pub cost_usd: f64,
}

/// Shared, interior-mutable handle to a [`ComposioUsage`] tally.
///
/// Cloning a provider context shares the same underlying counter, so the count
/// is stable no matter how the context is passed around within one sync.
///
/// A `std` `Mutex` rather than an async one on purpose: the lock is taken for a
/// single increment and never held across an `await`, and this crate carries no
/// async runtime to borrow one from.
pub type ComposioUsageHandle = Arc<Mutex<ComposioUsage>>;

#[cfg(test)]
#[path = "runs_tests.rs"]
mod tests;
