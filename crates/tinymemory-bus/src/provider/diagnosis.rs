//! [`Diagnosis`] — the typed, per-stage answer to "why is memory empty?".
//!
//! Returned by the `Diagnose` member of the maintenance family. It sits beside
//! [`crate::provider::types::MaintenanceReport`] rather than replacing it, and
//! the two are not redundant:
//!
//! - [`crate::provider::types::MaintenanceReport`] is what a **scheduler**
//!   reads. Its shape is uniform across reembed, compact, consolidate and
//!   doctor precisely so a caller driving all four on a timer does not
//!   special-case one, and its findings are prose because that is all a log
//!   line needs.
//! - [`Diagnosis`] is what an **operator, an agent, or a status panel** reads.
//!   Every field it adds is one a caller acts on rather than prints: the
//!   remediation key a frontend localises, the class that decides whether to
//!   offer a retry, the degradation flags that say results are reduced rather
//!   than absent, and the counters that distinguish "nothing ingested" from
//!   "ingested and not yet embedded".
//!
//! Widening `MaintenanceReport` to carry all of that was the alternative, and
//! it is the worse one: four of its five producers would leave every new field
//! empty, so the type would stop describing what any single call returns.
//!
//! # The failure vocabulary is the driver's, not this contract's
//!
//! [`DiagnosisFailure::code`] and [`DiagnosisFailure::class`] are strings.
//! Every engine classifies its own pipeline failures, and an enum here would
//! either freeze one engine's taxonomy into the contract or force a second
//! engine to squeeze its causes into someone else's variants — reporting a
//! cause as the nearest wrong one, which is worse than reporting it verbatim.
//! Same reasoning [`crate::provider::types::QueueFailure`] gives for keeping
//! the driver's own words.
//!
//! [`DiagnosisFailure::remediation_key`] is what makes that safe. The caller
//! resolves it to localised text and stays presentational, so an unrecognised
//! code degrades to "we have no localised advice for this" rather than to a
//! mis-rendered one.
//!
//! # Nothing here is memory content
//!
//! Stage notes, details and remediation keys are all operator-facing and are
//! logged. A driver must not put a namespace key, an entry body, a recall
//! query or a credential in any of them.

use serde::{Deserialize, Serialize};

/// One classified reason a stage is not healthy.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosisFailure {
    /// The driver's stable identifier for this cause, in `snake_case`.
    ///
    /// Compared for equality, never parsed. A caller that does not recognise a
    /// code still has [`Self::remediation_key`] and [`Self::detail`] to show.
    pub code: String,
    /// Whether retrying could help, in the driver's vocabulary — conventionally
    /// `transient` or `unrecoverable`.
    ///
    /// Optional because a driver may classify a cause without deciding its
    /// retry policy, and a caller that must guess is better served by an absent
    /// answer than by a defaulted wrong one: defaulting to `transient` invites
    /// a retry loop against a cause that can never clear.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    /// The i18n key a caller resolves to localised remediation text.
    ///
    /// Carried so the caller stays presentational — the driver decides what the
    /// user should be told to do, the caller decides in which language.
    pub remediation_key: String,
    /// A non-localised detail for logs and diagnosis. Never a secret.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// The health of one named stage of the driver's ingest pipeline.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosisStage {
    /// The driver's stable id for the stage (`routing`, `embeddings`, `queue`,
    /// …).
    ///
    /// The set is the driver's: a second engine has different stages, and a
    /// caller renders whatever it is given in order rather than looking for
    /// stages it knows by name.
    pub stage: String,
    /// Whether this stage is healthy.
    pub ok: bool,
    /// Why it is not, when it is not. Always `None` when [`Self::ok`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<DiagnosisFailure>,
    /// A short operator-facing note, healthy or not.
    pub note: String,
}

/// Which capabilities are running in a reduced mode.
///
/// "The pipeline ran, but the output is worse than it looks." Surfaced as its
/// own shape because a degraded result is otherwise indistinguishable from a
/// good one: a recall that fell back to recency because no embedder resolved
/// returns rows, and a caller with no way to know that presents them as
/// semantic hits.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DegradedCapabilities {
    /// Semantic recall is falling back to recency — no usable embedder.
    #[serde(default)]
    pub semantic_recall: bool,
    /// Extraction is producing no structure, so the entity index is empty.
    #[serde(default)]
    pub structure: bool,
    /// The driver's own storage path is unusable.
    ///
    /// The most severe of the three: the others reduce quality, this one stops
    /// the pipeline before it starts.
    #[serde(default)]
    pub storage: bool,
    /// The cause of the most significant degradation, when the driver knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<DiagnosisFailure>,
}

/// The counters a diagnosis is read against.
///
/// Present so "nothing comes back from recall" can be told apart from "nothing
/// was ever ingested" without a second call that could be answered either side
/// of a write.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DiagnosisCounters {
    /// Chunks the driver holds.
    pub total_chunks: u64,
    /// Jobs waiting.
    pub jobs_ready: u64,
    /// Jobs a worker currently holds.
    pub jobs_running: u64,
    /// Jobs in a terminal failure.
    pub jobs_failed: u64,
    /// Fraction of chunks with at least one extracted entity, in `[0.0, 1.0]`.
    ///
    /// `None` when the driver could not measure it — deliberately distinct from
    /// `Some(0.0)`, which is a real measurement of no structure. Collapsing the
    /// two reports a broken read as a broken pipeline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extraction_coverage: Option<f32>,
}

/// A one-shot, read-only diagnosis of the driver's ingest pipeline.
///
/// Read-only in the same sense as
/// [`crate::provider::types::MaintenanceReport`]'s doctor: it inspects
/// configuration, persisted state and counters, and changes nothing. It is
/// specified not to make a live provider call — a network probe would make the
/// diagnosis slow, flaky and order-dependent, and the degradation flags already
/// record what the last real run did.
///
/// # Why this must be asked of the driver rather than computed by the caller
///
/// Two of its four parts exist only in the driver's process.
/// [`Self::degraded`] is set by the embed and extract stages as they run, and
/// [`Self::counters`] is a read of the driver's own database. A caller that
/// hosts no engine has neither — it would report an all-clear degradation over
/// counters of zero, which is not a stale diagnosis but a confidently wrong
/// one.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Diagnosis {
    /// Whether nothing is blocking. Equivalent to
    /// [`Self::first_blocking_cause`] being `None`, carried so a caller can
    /// answer the yes/no question without reasoning about an `Option`.
    pub healthy: bool,
    /// Per-stage health, in the driver's own pipeline order.
    ///
    /// Order is meaningful: the stages run in it, so the first unhealthy one is
    /// the one to fix first. A caller renders the list as given rather than
    /// sorting it.
    #[serde(default)]
    pub stages: Vec<DiagnosisStage>,
    /// The single cause to act on first.
    ///
    /// One cause rather than every failing stage, because a stage that cannot
    /// run makes the ones after it fail too, and a wall of consequences buries
    /// the one thing a user can do about it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_blocking_cause: Option<DiagnosisFailure>,
    /// What is running in a reduced mode even where nothing is blocking.
    #[serde(default)]
    pub degraded: DegradedCapabilities,
    /// The counters the rest of the report is read against.
    #[serde(default)]
    pub counters: DiagnosisCounters,
}

#[cfg(test)]
#[path = "diagnosis_tests.rs"]
mod tests;
