//! [`MemorySourceSync`] — the family for syncs the *driver* runs.
//!
//! [`crate::provider::MemorySourceSink`] is the seam a caller writes fetched
//! items through. This is the other half of the same subject and deliberately
//! not the same family: here the driver owns the pipelines, walks the
//! connection, holds the cursor and the budget, and can price what it spent.
//!
//! ## What changed, and what did not
//!
//! The contract's fifth rule — "the host owns the loop" — was written when
//! every sync was driven from outside the driver. It still holds for
//! *sealing, cascading and maintenance*, and it no longer describes source
//! sync: the periodic Composio and workspace loops run inside the module, next
//! to the queue pool, because a host that stops compiling the engine has no
//! loop left to run. What stayed with the caller is the part a loop cannot
//! provide — the **manual** trigger. A user pressing "sync now" is not a
//! schedule, and no member of the sink family can express it.
//!
//! That is why this is a family and not four more methods on
//! [`crate::provider::MemorySourceSink`]. A driver that accepts a batch is not
//! thereby a driver that can walk an OAuth connection: a remote HTTP backend
//! and [`crate::null::NullMemoryProvider`] both do the first and neither can do
//! the second. Advertising them together would put a "sync now" control in
//! front of a driver that fails on first press — the registered-but-failing
//! outcome [`crate::capabilities`] exists to avoid — and, because a new method
//! on an already-advertised family is a **major** contract bump while a new
//! family is a minor one, it would also break every existing driver.
//!
//! ## Credentials still do not cross this contract
//!
//! No signature here names a token, a key, or a session. The driver resolves
//! whatever it needs through its own host seam, at call time — which is the
//! only way that works, since a connection can be authorised in a browser
//! minutes after the driver was bound.
//!
//! ## Neither does configuration
//!
//! [`MemorySourceSync::run_connection_sync`] takes no budget arguments. The
//! per-source caps — item limits, depth windows, token and cost ceilings — live
//! in the registry the driver already reads, so passing them would be a caller
//! restating something the driver knows, with two sources of truth for a limit
//! that costs money when it is wrong.

use async_trait::async_trait;

use crate::error::MemoryError;

// The value types this family exchanges. They are defined in `tinymemory-bus`
// — they cross the module boundary, and a host that only makes calls must be
// able to name them without compiling this trait — and re-exported here so the
// two trees stay the same shape and the types stay the same types.
pub use tinymemory_bus::provider::sync::{
    RawArchiveCoverage, RawRebuildOutcome, SourceSyncState, SourceSyncStatus, SyncAuditEntry,
    SyncFreshness, SyncRunOutcome,
};

/// Running a source sync on demand, and reporting what past runs cost.
#[async_trait]
pub trait MemorySourceSync: Send + Sync {
    /// Sync one connection now, and report what the run moved and spent.
    ///
    /// `toolkit` is the provider slug (`gmail`, `slack`, `github`, …) and
    /// `connection_id` the authorised connection under it. Both are wire
    /// strings rather than enums, for the reason the sink family's
    /// `source_kind` is one: the set belongs to whoever integrates providers
    /// and grows without a contract change.
    ///
    /// # This is the manual path, and it is not idempotent
    ///
    /// It is what a user's "sync now" reaches. Calling it twice runs the
    /// pipeline twice — the cursor makes the second run cheap rather than
    /// free, and both runs append an audit row. A driver that is already
    /// syncing this connection should serialise rather than run a second walk
    /// concurrently; two walks sharing one cursor lose items.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Invalid`] for a toolkit the driver has no pipeline for
    /// or a connection it cannot resolve — deliberately not an outcome of
    /// zero, which a caller would render as "nothing new" over a source that
    /// can never sync.
    ///
    /// [`MemoryError::BudgetExceeded`] when a per-source token or cost ceiling
    /// stops the run.
    ///
    /// Otherwise backend and provider failures. A run that failed **after**
    /// spending is still a failure: what it burned belongs in the error the
    /// driver returns and in the audit row it writes, not in an `Ok` that
    /// reports partial progress as success.
    async fn run_connection_sync(
        &self,
        toolkit: &str,
        connection_id: &str,
    ) -> Result<SyncRunOutcome, MemoryError>;

    /// The persisted cursor, dedup and budget state for one connection.
    ///
    /// `Ok(None)` for a connection that has never synced — a valid state, not
    /// a missing record, and the reason this is not
    /// [`MemoryError::NotFound`]: a status surface listing every connection
    /// would otherwise turn "never synced" into an error row.
    ///
    /// # This is the status read, not the whole row
    ///
    /// [`SourceSyncState`] carries counts where the persisted row carries
    /// sets, and says why. A caller that needs the dedup set itself — a
    /// disconnect walking it to decide which per-item documents to forget —
    /// reads the row through [`crate::provider::MemoryGraph::kv_get`] instead.
    /// What this member adds over that read is the driver's own day-rollover
    /// rule applied to the budget, and the absence of a row reported as
    /// `Ok(None)` rather than as a namespace-and-key convention the caller has
    /// to know.
    ///
    /// # Errors
    ///
    /// Backend failures only.
    async fn source_sync_state(
        &self,
        toolkit: &str,
        connection_id: &str,
    ) -> Result<Option<SourceSyncState>, MemoryError>;

    /// Past sync runs, newest first.
    ///
    /// `limit` caps the rows and the driver clamps it to its own ceiling — a
    /// caller cannot raise it by asking for more, the same rule
    /// [`crate::provider::ChunkQuery::limit`] carries. `None` means "the
    /// driver's own cap", **not** unbounded: the log is append-only for the
    /// life of a workspace, so an unbounded read is a response that grows
    /// without limit and eventually cannot cross a frame at all.
    ///
    /// A caller totalling a period therefore reads the newest rows and stops
    /// when it passes the period's start. That is the one reduction against
    /// reading the log file directly, and it is the shape a total wants
    /// anyway: newest-first ordering means the rows a period needs are the
    /// first ones returned.
    ///
    /// # Errors
    ///
    /// Backend failures only. A driver that has never synced returns an empty
    /// log, which is true of it.
    async fn sync_audit_log(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<SyncAuditEntry>, MemoryError>;

    /// Price a token count at the same rate the driver stamped onto its audit
    /// rows.
    ///
    /// # Why this is a call and not a constant a caller could hold
    ///
    /// It looks like arithmetic, and copying it is the mistake this member
    /// exists to prevent. The same constants produce
    /// [`SyncAuditEntry::estimated_cost_usd`] on every row this driver writes.
    /// A caller holding its own copy has a second price the moment either side
    /// is retuned, and it would then present a projected cost and a historical
    /// total computed at two different rates, on the same screen, with nothing
    /// to say which.
    ///
    /// So the price stays where the rows are written, and a caller that wants
    /// to quote one asks.
    ///
    /// # Errors
    ///
    /// Backend failures only; a driver that prices nothing answers `0.0`
    /// rather than refusing, which is true of a driver whose sync costs the
    /// user nothing.
    async fn estimate_sync_cost_usd(
        &self,
        input_tokens: u64,
        output_tokens: u64,
    ) -> Result<f64, MemoryError>;

    /// Per-provider sync progress, derived from stored content.
    ///
    /// Not from the sync machinery's own counters, and the difference is what
    /// makes it survive a restart: a run killed mid-wave leaves its chunks
    /// behind, so a count taken from the content is real where a counter that
    /// was never decremented is not.
    ///
    /// # Errors
    ///
    /// Backend failures only; a store with no synced content returns an empty
    /// list.
    async fn sync_statuses(&self) -> Result<Vec<SourceSyncStatus>, MemoryError>;

    /// How much of one raw archive the tree derived from it covers.
    ///
    /// `tree_scope` names the summary tree and `archive_source_id` the raw
    /// archive beneath it; a sync writes both, and a run that died between
    /// them leaves an archive the tree does not cover. This is the read behind
    /// a "reconcile" control, and [`Self::rebuild_from_raw_archive`] is its
    /// repair.
    ///
    /// # Errors
    ///
    /// Backend failures only. An archive the driver has never written is a
    /// coverage of zero over a total of zero, not [`MemoryError::NotFound`]:
    /// the caller is asking whether anything is missing, and "there is nothing
    /// there" answers that.
    async fn raw_archive_coverage(
        &self,
        tree_scope: &str,
        archive_source_id: &str,
    ) -> Result<RawArchiveCoverage, MemoryError>;

    /// Re-derive a summary tree from its raw archive.
    ///
    /// The repair [`Self::raw_archive_coverage`] diagnoses. It re-reads the
    /// archive and re-summarises what the tree is missing, so it costs
    /// inference and can be slow; a caller runs it in the background and
    /// reports progress from the outcome rather than blocking a user on it.
    ///
    /// Safe to repeat: a file the tree already covers is not summarised twice,
    /// so a rebuild interrupted halfway resumes rather than starting over.
    ///
    /// # Errors
    ///
    /// [`MemoryError::BudgetExceeded`] when an inference budget stops the
    /// rebuild mid-run — what it managed is in the error, not in an `Ok` that
    /// would read as a completed repair.
    ///
    /// Otherwise backend failures.
    async fn rebuild_from_raw_archive(
        &self,
        tree_scope: &str,
        archive_source_id: &str,
    ) -> Result<RawRebuildOutcome, MemoryError>;
}
