//! Value types that exist only because the *driver contract* needs them.
//!
//! Everything here is inert data: serde-derived, dependency-light, and free of
//! any engine or host type. They are separated from [`crate::types`] because
//! that module carries the historical engine value types (which the engine
//! crate aliases back into `tinycortex::memory::types`), whereas these are new
//! shapes introduced by the provider contract itself.
//!
//! ## Why these types and not the engine's
//!
//! Several families the contract exposes (diff, entities, sources,
//! maintenance) have richer types inside the `tinycortex` engine — for example
//! `memory::diff::types::DiffResult`. Those types are *implementation* shapes:
//! they carry git commit SHAs, ledger paths, and engine-specific enums. A
//! third-party driver cannot produce them and must not be required to.
//!
//! So the contract defines the narrower shape a *caller* actually needs, with
//! wire strings deliberately identical to the engine's where they overlap
//! (`added`/`removed`/`modified`), so the embedded driver's conversion is a
//! field-for-field map rather than a translation.
//!
//! ## What is deliberately absent
//!
//! No type here names a configuration struct. `MemoryConfig` stayed engine-side
//! in the M0 carve-out and stays there: a driver holds its own configuration
//! and the contract passes only domain arguments. If a future method cannot be
//! expressed without configuration, that is a signal the family was designed
//! wrong, not that the contract should widen.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::chunks::{DataSource, SourceRef};
use crate::types::MemoryTaint;

/// A per-turn allowlist of memory sources, passed **into** the driver as a
/// query predicate.
///
/// ## Why this is a parameter and not a post-filter
///
/// The host computes a per-turn source allowlist from product policy. If that
/// allowlist were applied after the driver returned rows, a `limit` would be
/// consumed by rows the caller is not allowed to see — so a scoped query could
/// return fewer results than it should, or none at all, purely as an artefact
/// of filtering order. Worse, an out-of-process driver would have already been
/// handed a query it should never have answered in full.
///
/// The predicate therefore travels with the call. `None` means unrestricted;
/// `Some(scope)` means the driver must apply it *inside* its query.
///
/// ## Matching rule (fail-closed)
///
/// [`SourceScope::allows_source_id`] encodes the embedded engine's SQL
/// semantics verbatim: a source-attributed id is in scope when it either equals
/// an allowed id outright, or begins with `mem_src:{allowed}:`. An **empty**
/// allow list therefore matches nothing — a scope that lists no sources denies
/// all source-attributed content rather than waving it through.
///
/// Content that is not attributed to a memory source at all (no
/// `memory_sources` provenance) is outside this predicate's remit; the driver
/// decides that, exactly as the engine's SQL does today.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceScope {
    /// Allowed memory-source identifiers. Empty denies all source-attributed
    /// content.
    pub allow: Vec<String>,
}

impl SourceScope {
    /// Builds a scope from any iterator of source identifiers.
    pub fn new(allow: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            allow: allow.into_iter().map(Into::into).collect(),
        }
    }

    /// Whether this scope lists no sources — in which case it denies all
    /// source-attributed content. See the type docs for why that is the
    /// fail-closed reading and not "unrestricted".
    pub fn is_empty(&self) -> bool {
        self.allow.is_empty()
    }

    /// Whether `source_id` is in scope, using the engine's equality-or-prefix
    /// rule.
    ///
    /// ```
    /// use tinymemory_bus::provider::types::SourceScope;
    ///
    /// let scope = SourceScope::new(["src-abc"]);
    /// assert!(scope.allows_source_id("src-abc"));
    /// assert!(scope.allows_source_id("mem_src:src-abc:item-1"));
    /// assert!(!scope.allows_source_id("src-xyz"));
    ///
    /// // An empty scope denies everything.
    /// assert!(!SourceScope::default().allows_source_id("src-abc"));
    /// ```
    pub fn allows_source_id(&self, source_id: &str) -> bool {
        self.allow.iter().any(|allowed| {
            source_id == allowed || source_id.starts_with(&format!("mem_src:{allowed}:"))
        })
    }
}

/// One unit of content handed to `MemoryIngest`.
///
/// The driver owns chunking, embedding, and persistence — this type carries
/// only what the driver cannot know: where the content came from, when, who it
/// belongs to, and how far it may be trusted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestItem {
    /// Target namespace; `None` means the driver's default namespace.
    #[serde(default)]
    pub namespace: Option<String>,
    /// Concrete upstream provider the content came from.
    pub source: DataSource,
    /// Stable logical id for the ingestion group (channel id, thread id, doc
    /// id). This is the dedupe key, not a display value.
    pub source_id: String,
    /// Account or user the content belongs to; empty for anonymous/system
    /// sources.
    #[serde(default)]
    pub owner: String,
    /// Opaque pointer back to the raw source record, for citation and
    /// drill-down.
    #[serde(default)]
    pub source_ref: Option<SourceRef>,
    /// The content itself, already decoded to text.
    pub content: String,
    /// MIME type of [`Self::content`] when the caller knows it.
    #[serde(default)]
    pub mime: Option<String>,
    /// Event time used for ordering and tree placement; the driver substitutes
    /// ingest time when absent.
    #[serde(default)]
    pub timestamp: Option<DateTime<Utc>>,
    /// Labels carried through from the source. Ingest does not interpret them.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Provenance taint. The **host** stamps this; a driver must persist what it
    /// is given and must never assign or upgrade it.
    #[serde(default)]
    pub taint: MemoryTaint,
    /// Overrides `source_id` for on-disk path grouping only; `source_id`
    /// remains the dedupe key.
    #[serde(default)]
    pub path_scope: Option<String>,
}

/// What an ingest call actually persisted.
///
/// Counts rather than content, so the caller can report progress and detect a
/// silently-dropping driver without holding the written material in memory.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestOutcome {
    /// Units the driver newly persisted.
    pub written: u32,
    /// Units the driver recognised as already present and skipped.
    pub skipped: u32,
    /// Driver-assigned ids for the written units, when the driver exposes them.
    /// May be empty even when [`Self::written`] is non-zero — an external
    /// backend is not obliged to surface its internal ids.
    #[serde(default)]
    pub ids: Vec<String>,
}

/// One line of the portability stream.
///
/// Export and import are defined over records rather than bytes so the contract
/// stays free of an async runtime and of any streaming abstraction: the host
/// adapter turns a page of records into NDJSON (and back) at the transport
/// boundary.
///
/// [`Self::kind`] is a driver-defined string rather than an enum. A backend has
/// record kinds this crate has never heard of, and a migration between two
/// backends must round-trip them untouched rather than drop what it cannot
/// classify.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportRecord {
    /// Driver-defined record kind (e.g. `entry`, `document`, `chunk`).
    pub kind: String,
    /// Driver-assigned id, unique within [`Self::kind`].
    pub id: String,
    /// Owning namespace, when the record has one.
    #[serde(default)]
    pub namespace: Option<String>,
    /// Provenance taint of the record's content. Preserved across
    /// export → import; an importing driver must not re-stamp it.
    #[serde(default)]
    pub taint: MemoryTaint,
    /// The record body, in the exporting driver's own shape.
    pub payload: serde_json::Value,
}

/// One page of an export, plus the cursor that continues it.
///
/// Paging (rather than a stream) keeps `MemoryPortability`
/// object-safe and runtime-agnostic while still bounding memory: the caller
/// decides the page size and drives the loop.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ExportPage {
    /// Records in this page. May be empty on the final page.
    pub records: Vec<ExportRecord>,
    /// Opaque cursor to pass to the next call. `None` means the export is
    /// complete — this, not an empty [`Self::records`], is the terminator.
    #[serde(default)]
    pub next_cursor: Option<String>,
}

/// What an import call actually accepted.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportOutcome {
    /// Records written.
    pub imported: u32,
    /// Records recognised as already present and skipped.
    pub skipped: u32,
    /// Records rejected. A non-zero value with an empty [`Self::errors`] is a
    /// driver bug: a rejection the operator cannot diagnose.
    pub failed: u32,
    /// Operator-facing reasons for the failures, bounded by the driver. Must
    /// not contain record content or credentials — this is logged.
    #[serde(default)]
    pub errors: Vec<String>,
}

/// Identity of an entity in the driver's index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityRef {
    /// Canonical, driver-stable entity id.
    pub id: String,
    /// Entity kind as a wire string (`person`, `organization`, `topic`, …).
    /// A string rather than an enum because the taxonomy is the driver's, and a
    /// kind this build does not recognise must still round-trip.
    pub kind: String,
    /// Display name.
    pub name: String,
}

/// An entity together with its recency/frequency signals.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EntityHit {
    /// The entity itself.
    pub entity: EntityRef,
    /// Driver-computed hotness, higher is hotter. Not normalised across
    /// drivers — compare within one driver's results only.
    pub hotness: f64,
    /// Number of times the entity was observed.
    pub mentions: u32,
}

/// Identity of a captured snapshot.
///
/// The engine's own snapshot type additionally carries the git commit SHA and
/// ledger trailers that back it; those are implementation, so the contract
/// exposes only the identity and the counts a caller can act on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotRef {
    /// Driver-stable snapshot id.
    pub id: String,
    /// Logical source this snapshot covers.
    pub source_id: String,
    /// Human-readable source label at capture time.
    #[serde(default)]
    pub label: String,
    /// Number of items materialised into the snapshot.
    pub item_count: u32,
    /// Capture time in milliseconds since the Unix epoch.
    pub taken_at_ms: i64,
}

/// What happened to one item between two snapshots.
///
/// Wire strings are identical to the engine's `memory::diff::types::ChangeKind`
/// so the embedded adapter maps rather than translates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    /// Present in the later snapshot only.
    Added,
    /// Present in the earlier snapshot only.
    Removed,
    /// Present in both, with differing content.
    Modified,
}

impl ChangeKind {
    /// Stable wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Removed => "removed",
            Self::Modified => "modified",
        }
    }
}

/// A single item-level change inside a [`DiffReport`].
///
/// Item identity is the item id, never the title, so a rename reports as a
/// removal plus an addition rather than a modification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceChange {
    /// Stable item id.
    pub item_id: String,
    /// Display title, or the id when the driver has no better label.
    #[serde(default)]
    pub title: String,
    /// What kind of change occurred.
    pub kind: ChangeKind,
    /// Content hash on the earlier side; absent for an addition.
    #[serde(default)]
    pub old_content_hash: Option<String>,
    /// Content hash on the later side; absent for a removal.
    #[serde(default)]
    pub new_content_hash: Option<String>,
}

/// The result of diffing one source between two snapshots.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffReport {
    /// Source this diff covers.
    pub source_id: String,
    /// Baseline snapshot id; `None` for a first-ever diff, where everything is
    /// an addition.
    #[serde(default)]
    pub from_snapshot_id: Option<String>,
    /// Target snapshot id.
    pub to_snapshot_id: String,
    /// Items added.
    pub added: u32,
    /// Items removed.
    pub removed: u32,
    /// Items modified.
    pub modified: u32,
    /// Items present and unchanged.
    pub unchanged: u32,
    /// Per-item changes. May be truncated by the driver; the counts above are
    /// authoritative.
    #[serde(default)]
    pub changes: Vec<SourceChange>,
}

/// One item handed to `MemorySourceSink` by the host's sync
/// machinery.
///
/// The host owns credentials, scheduling, and fetching; the driver owns storage
/// and indexing. This type is the whole of what crosses that line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceItem {
    /// Stable per-source item id. Dedupe key; not a display value.
    pub item_id: String,
    /// Display title.
    #[serde(default)]
    pub title: String,
    /// Item body, already decoded to text.
    pub content: String,
    /// MIME type of [`Self::content`] when known.
    #[serde(default)]
    pub mime: Option<String>,
    /// Canonical URL back to the item, when it has one.
    #[serde(default)]
    pub url: Option<String>,
    /// Upstream last-modified time in milliseconds since the Unix epoch.
    #[serde(default)]
    pub updated_at_ms: Option<i64>,
    /// Labels carried through from the source.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Outcome of one maintenance operation.
///
/// A single shape covers reembed, compact, consolidate, and doctor because the
/// caller does the same thing with all four: report progress and surface
/// findings. A per-operation result type would multiply the contract surface
/// without giving any caller more to act on.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaintenanceReport {
    /// Which operation ran (`reembed`, `compact`, `consolidate`, `doctor`).
    pub operation: String,
    /// Units the driver examined.
    pub examined: u64,
    /// Units the driver changed. Always `0` for `doctor`, which is read-only.
    pub changed: u64,
    /// Operator-facing findings and notes. Must not contain memory content or
    /// credentials — this is logged and shown in status output.
    #[serde(default)]
    pub findings: Vec<String>,
}

/// Aggregate counts over what the driver has stored.
///
/// Separate from [`MaintenanceReport`] because the caller does something
/// different with it: a report is read by an operator, these are read by code.
/// `findings: Vec<String>` cannot answer "how far behind is the pipeline"
/// without parsing prose back into numbers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreStats {
    /// Chunks the driver holds.
    pub chunks: u64,
    /// Timestamp of the most recently stored chunk, if any.
    ///
    /// `None` for an empty store — distinct from `Some(0)`, which would be a
    /// chunk stamped at the epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub most_recent_chunk_ms: Option<i64>,
}

/// The ingest and re-embed queue's state, as counts rather than rows.
///
/// Every field answers a question an operator or a health probe asks about
/// throughput. A driver with no queue answers all-zero rather than refusing:
/// "nothing is backed up" is true of a driver that cannot back up.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueStats {
    /// Jobs waiting, whatever their scheduled time.
    pub ready: u64,
    /// Jobs a worker currently holds.
    pub running: u64,
    /// Jobs that finished successfully.
    pub done: u64,
    /// Jobs that ended in a terminal failure.
    pub failed: u64,
    /// Ready jobs whose scheduled time has already passed.
    ///
    /// The difference between this and [`Self::ready`] is deferred work, and
    /// conflating them reads a healthy backlog of future jobs as a stall.
    pub eligible_now: u64,
    /// When the queue last settled a job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_completed_ms: Option<i64>,
    /// The scheduled time of the oldest job eligible to run now.
    ///
    /// With [`Self::last_completed_ms`] this is what an idle-time calculation
    /// needs: how long something runnable has been waiting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oldest_eligible_ms: Option<i64>,
}

/// The most recent terminal queue failure.
///
/// Carries the driver's own words rather than a class this contract invents:
/// the caller shows it to an operator, and a re-classification here would lose
/// what the engine actually said.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueFailure {
    /// The failure's own message. Must carry no memory content.
    pub reason: String,
    /// The driver's classification, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    /// When the failing job settled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at_ms: Option<i64>,
    /// When the queue last completed a job *successfully*, read together with
    /// the failure above rather than in a second call.
    ///
    /// A caller deciding whether to show this failure asks whether anything
    /// has succeeded since it — a success after the failure means the queue
    /// recovered and the failure is stale. Answering that from two separate
    /// calls lets a job settle in between and flip the decision, so the two
    /// values are read as one observation.
    ///
    /// This is not [`QueueStats::last_completed_ms`]: that one counts a
    /// failure as progress, because a fast-failing queue is not a stalled
    /// one. Supersession needs the opposite reading — only a success clears a
    /// failure — so it takes the newest *successful* completion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_success_ms: Option<i64>,
}

#[cfg(test)]
#[path = "types_tests.rs"]
mod tests;
