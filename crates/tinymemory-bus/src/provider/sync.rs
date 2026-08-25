//! The source-sync family: what the driver reports about a sync it ran itself.
//!
//! A driver advertising
//! [`Capability::SourceSync`](crate::capabilities::Capability::SourceSync) does
//! not merely *accept* items a caller fetched — that is
//! [`Capability::Sources`](crate::capabilities::Capability::Sources) and the
//! sink it names. This family is the other direction: the driver holds the
//! pipelines, walks the connection itself, and answers for what the walk cost.
//!
//! # Why the two are different families
//!
//! [`Capability::Sources`](crate::capabilities::Capability::Sources) documents
//! itself as "accepting synced source items; the host still owns credentials
//! and scheduling", and that premise is still true of the sink. It stopped
//! being true of the *loop*: the periodic Composio and workspace loops now run
//! inside the module beside the queue pool, so the schedule and the credential
//! resolution are the driver's. What the caller kept is the **manual** trigger
//! — a user pressing "sync now" — which is a call it must be able to make and
//! which no member of the sink family can express.
//!
//! Folding these onto the sink instead would advertise them for every driver
//! that can accept a batch. A remote HTTP driver and the null driver both
//! accept batches and neither owns a Composio pipeline, so their callers would
//! get a registered "sync now" button that fails on first press — the
//! registered-but-failing outcome [`crate::capabilities`] exists to avoid.
//!
//! # Money is reported, never recomputed
//!
//! [`SyncAuditEntry::effective_cost_usd`] is arithmetic over fields the row
//! already carries, so it is safe here. The *price* — what a token costs — is
//! deliberately **not** here: it is asked of the driver, because the same
//! constants are what stamped `estimated_cost_usd` onto every row this module
//! hands back. A second copy of those constants in a caller becomes a second
//! price the moment either side is retuned, and the audit rows would then be
//! summed at a rate they were never written with.
//!
//! # No path leaves the driver
//!
//! [`RawArchiveCoverage`] counts pending files and does not name them. The
//! engine's own coverage scan carries absolute paths into the driver's content
//! vault; those describe the driver's storage layout, which no caller may
//! depend on and which is the one thing a bus payload should never teach it.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// What one sync run moved, and what it spent doing it.
///
/// The same five numbers a failed run reports in its error message, so a caller
/// that logs both paths logs the same vocabulary either way.
///
/// # Not `composio::runs::SyncOutcome`
///
/// That one is the *report* a caller assembles about a run — which toolkit,
/// which connection, why it ran, when it started and finished, and a one-line
/// summary for a status panel. This is what the run itself produced: how much
/// landed, whether more is waiting, and what the provider charged. A caller
/// building the first from the second is the normal direction; nothing builds
/// the second from the first, which is why they are two shapes rather than one
/// with half its fields unset on every call.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SyncRunOutcome {
    /// Items the run stored.
    pub records_ingested: u32,
    /// Whether the source has more waiting than this run took.
    ///
    /// A cap was hit — the per-source item limit, the depth window, or the
    /// daily request budget — and another run would fetch more. Distinct from
    /// `records_ingested == 0`, which can equally mean "nothing new".
    pub more_pending: bool,
    /// Provider actions the run called.
    ///
    /// The unit the daily budget is counted in, so a caller showing "requests
    /// used today" adds these rather than counting runs.
    #[serde(default)]
    pub actions_called: u32,
    /// What the provider charged for those actions, in USD.
    ///
    /// The provider's own charge, not the inference cost — that lands on the
    /// audit row as [`SyncAuditEntry::estimated_cost_usd`]. Kept apart because
    /// they are billed by different parties.
    #[serde(default)]
    pub provider_cost_usd: f64,
    /// A short operator-facing note, when the driver has one.
    ///
    /// Never memory content: this is rendered in sync status and written to the
    /// log.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The persisted cursor, dedup and budget state for one connection.
///
/// Read-only on this contract. A caller inspects it to render status; it is
/// written by the runs themselves, and a caller that could set a cursor could
/// silently re-fetch or skip a window with no record of having done so.
///
/// # Why counts and not the sets
///
/// The persisted state holds the full set of synced item ids and their content
/// versions. Those are unbounded — a mature Gmail connection carries tens of
/// thousands — and a status row needs the size, not the members. Sending the
/// sets would put an ever-growing payload behind a call whose only consumer
/// renders one number from it, and would leak per-message identifiers to a
/// surface that has no use for them.
///
/// The one caller that genuinely walks the set — a disconnect deciding which
/// per-item documents to forget — reads the persisted row directly through the
/// graph family's `KvGet`, on the sync-state namespace, and decodes it into the
/// shape `composio::state` defines. That path exists, it is not this one, and
/// keeping them apart is what lets a status poll stay small while a disconnect
/// still gets everything.
///
/// # What this adds over reading that row
///
/// Two things a raw read cannot give a caller. The daily budget rolls over on
/// the driver's own day boundary, and the persisted row is only rewritten when
/// a sync runs — so `requests_used` read raw is yesterday's number until the
/// next run, while [`Self::daily_requests_used`] has the rollover applied. And
/// the absence of a row means "never synced", which a caller can only learn by
/// knowing the namespace and key convention the driver writes under; asking
/// here spells neither.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSyncState {
    /// The toolkit this state belongs to (`gmail`, `slack`, …), lowercased.
    pub toolkit: String,
    /// The connection this state belongs to.
    pub connection_id: String,
    /// The provider cursor the next run resumes from, in the provider's own
    /// encoding.
    ///
    /// Opaque: round-trip it, show it, never parse it. Slack's is a JSON map of
    /// per-channel cursors and Gmail's is a page token, and a caller that
    /// learned to read one would break on the other.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// How many item ids the dedup set holds.
    pub synced_item_count: u64,
    /// The newest item id the connection has seen, when it tracks one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_id: Option<String>,
    /// When the last run finished.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_sync_at_ms: Option<u64>,
    /// Provider requests spent today against [`Self::daily_request_limit`].
    ///
    /// Rolls over on the driver's own day boundary. A caller reading a used
    /// count above the limit is reading a limit that was lowered after the
    /// spend, not a budget overrun.
    pub daily_requests_used: u32,
    /// The connection's daily provider-request budget.
    pub daily_request_limit: u32,
}

/// One sync run, as the driver's audit log recorded it.
///
/// The field names are the driver's on-disk format. They are reproduced here
/// rather than renamed so a caller reading a row over the bus and a caller
/// reading the log file directly see the same keys, and so the driver's
/// conversion is a field-for-field map with nothing to get wrong.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SyncAuditEntry {
    /// When the run finished.
    pub timestamp: DateTime<Utc>,
    /// The source the run was for.
    pub source_id: String,
    /// The source's kind (`composio`, `folder`, `github`, …).
    pub source_kind: String,
    /// The tree scope the run wrote under.
    pub scope: String,
    /// Items the run fetched from the provider.
    pub items_fetched: u32,
    /// Summary batches the run sealed.
    pub batches: u32,
    /// Inference input tokens the run spent.
    pub input_tokens: u64,
    /// Inference output tokens the run spent.
    pub output_tokens: u64,
    /// What those tokens were *estimated* to cost, priced by the driver.
    ///
    /// Stamped at write time from the driver's own price table. Two rows
    /// written either side of a retune carry two different rates, which is
    /// correct: each says what it was priced at.
    pub estimated_cost_usd: f64,
    /// Composio actions the run called.
    #[serde(default)]
    pub composio_actions_called: u32,
    /// What Composio charged for those actions.
    #[serde(default)]
    pub composio_cost_usd: f64,
    /// What the inference provider actually billed, when it reported a figure.
    ///
    /// `None` means no charge was reported and the estimate stands — not that
    /// the run was free.
    #[serde(default)]
    pub actual_charged_usd: Option<f64>,
    /// Wall-clock duration of the run.
    pub duration_ms: u64,
    /// Whether the run completed.
    pub success: bool,
    /// Why it did not, when it did not. Never memory content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl SyncAuditEntry {
    /// What the run cost, as the audit views it.
    ///
    /// The real charge when the provider reported one, the estimate otherwise,
    /// plus Composio's own action cost. This is arithmetic over fields the row
    /// already carries and introduces no price of its own — which is why it can
    /// live here while the price behind
    /// [`ESTIMATE_SYNC_COST_USD`](crate::names::methods::ESTIMATE_SYNC_COST_USD)
    /// has to be asked of the driver.
    #[must_use]
    pub fn effective_cost_usd(&self) -> f64 {
        self.actual_charged_usd.unwrap_or(self.estimated_cost_usd) + self.composio_cost_usd
    }
}

/// How recently a provider last landed content.
///
/// Three buckets rather than an age, because the caller renders a badge and the
/// thresholds are the driver's to choose. A caller that computed its own
/// buckets from a timestamp would disagree with the driver's status surface by
/// exactly the drift between the two threshold tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncFreshness {
    /// Content landed within the driver's "just now" window.
    Active,
    /// Content landed recently, but the provider is no longer streaming.
    Recent,
    /// Nothing has landed lately, or ever.
    Idle,
}

/// One provider's share of the store, and how far its last wave got.
///
/// Derived from stored content rather than from the sync machinery, which is
/// what makes it survivable across a restart: a run that died mid-wave still
/// leaves its chunks, so the pending count is real rather than a counter that
/// was never decremented.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSyncStatus {
    /// The provider these counts are for, as the driver names it.
    pub provider: String,
    /// Chunks the provider has contributed in total.
    pub chunks_synced: u64,
    /// Of those, how many still await derived work (embedding or extraction).
    pub chunks_pending: u64,
    /// Chunks in the most recent wave.
    ///
    /// A "wave" is the driver's grouping of one burst of arrivals; with
    /// [`Self::batch_processed`] it is what a progress bar needs. Zero when
    /// nothing is pending — there is no wave in flight to show.
    pub batch_total: u64,
    /// Of that wave, how many are finished.
    pub batch_processed: u64,
    /// When the provider last landed content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_chunk_at_ms: Option<i64>,
    /// The badge [`Self::last_chunk_at_ms`] resolves to, bucketed by the
    /// driver.
    pub freshness: SyncFreshness,
}

/// How much of a raw archive has made it into the tree derived from it.
///
/// The crosscheck behind a "reconcile" control: a sync writes raw files and
/// then derives a summary tree from them, and a run that died between the two
/// leaves an archive the tree does not cover.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawArchiveCoverage {
    /// Raw files the archive holds.
    pub total: u64,
    /// Of those, how many the tree covers.
    pub covered: u64,
    /// How many are still uncovered.
    ///
    /// A count, deliberately, and the one reduction this family makes against
    /// what the engine computes: the engine's scan carries each pending file's
    /// absolute path inside the driver's content vault. A path is the driver's
    /// storage layout, which the contract never hands out — see the module
    /// docs — and no caller needs it: the pending set is not addressable
    /// through any member here, because the repair —
    /// [`REBUILD_FROM_RAW_ARCHIVE`](crate::names::methods::REBUILD_FROM_RAW_ARCHIVE)
    /// — takes the same scope rather than a file list.
    pub pending: u64,
}

/// What rebuilding a tree from its raw archive read, sealed and spent.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RawRebuildOutcome {
    /// Raw files the rebuild read.
    pub files_read: u64,
    /// Summary batches it sealed.
    pub batches: u64,
    /// Inference input tokens it spent.
    pub input_tokens: u64,
    /// Inference output tokens it spent.
    pub output_tokens: u64,
    /// What those tokens were estimated to cost, priced by the driver.
    pub estimated_cost_usd: f64,
    /// What the inference provider actually billed, when it reported a figure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_charged_usd: Option<f64>,
}

#[cfg(test)]
#[path = "sync_tests.rs"]
mod tests;
