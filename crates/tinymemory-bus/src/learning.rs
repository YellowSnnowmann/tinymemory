//! The learning-candidate taxonomy: what a producer asserts about the user,
//! and how strongly.
//!
//! A *candidate* is one observation — "this user prefers `pnpm`", "this user's
//! timezone is `UTC+5:30`" — emitted by a producer and later aggregated by a
//! stability detector into a durable profile facet. The detector weights each
//! candidate by its [`CueFamily`] and decays it by age; the [`FacetClass`]
//! decides the half-life and the per-class budget it is scored against.
//!
//! These three types moved here from the engine crate for the same reason
//! [`crate::evidence::EvidenceRef`] did, one module over: **the producer and
//! the consumer are on opposite sides of the module boundary**. The Composio
//! provider-profile sync emits an identity candidate on every run and runs
//! inside `tinymemory-module`; the stability detector that consumes it runs in
//! the host. Two structurally identical enums either side of that seam would
//! round-trip through serde and diverge silently on the first added variant —
//! and `FacetClass` is exactly the kind of enum that grows.
//!
//! ## What is deliberately *not* here
//!
//! The **queue** is not. The engine crate keeps the bounded ring buffer and its
//! process-global singleton, because a global is not a payload: this crate is
//! compiled into the host binary *and* into the module `cdylib`, so a `static`
//! declared here would be two statics, and a producer pushing into one while a
//! consumer drains the other is worse than no queue at all. See
//! `tinymemory_core::learning_candidate` for the buffer, and the note there on
//! why crossing the module boundary needs a bus member rather than a shared
//! `static`.
//!
//! Also not here: the stability formula itself (`TAU_*` / `HALF_LIFE_*` /
//! `BUDGET_*`, the aggregation and the promotion rules). That is host policy —
//! it decides what the product is willing to believe about a user — and it has
//! never lived in the memory stack.

use serde::{Deserialize, Serialize};

use crate::evidence::EvidenceRef;

/// Six-class taxonomy of what the learned-facet cache can hold.
///
/// Keys are stored with a class prefix, e.g. `style/verbosity` or
/// `tooling/package_manager`. The class determines the half-life and the class
/// budget the stability detector scores a candidate against, so it is part of
/// the *storage* key, not only a label: renaming a variant strands every facet
/// filed under the old name.
///
/// The serde form is `snake_case` and is persisted; treat each variant name as
/// a compatibility surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FacetClass {
    /// Communication style preferences — verbosity, formality, code format.
    Style,
    /// Stable biographical facts — timezone, name, language, role.
    Identity,
    /// Developer toolchain preferences — package manager, editor, OS, language.
    Tooling,
    /// Hard user vetoes — things the user has explicitly rejected or forbidden.
    Veto,
    /// Active user goals or ongoing projects.
    Goal,
    /// Preferred communication channel or platform.
    Channel,
}

/// How a candidate signal was produced — determines the weight multiplier
/// applied in the stability formula.
///
/// Higher-weight families contribute more strongly per evidence item. The
/// weights are the canonical values the detector was tuned against:
/// `Explicit=1.0`, `Structural=0.9`, `Behavioral=0.7`, `Recurrence=0.6`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CueFamily {
    /// Direct declaration of intent by the user (highest weight — 1.0).
    ///
    /// Examples: "I prefer pnpm", "my timezone is PST", "always use terse replies".
    Explicit,
    /// Inferred from structured file or provider metadata (weight 0.9).
    ///
    /// Examples: `package.json#packageManager`, Gmail display name, Slack workspace.
    Structural,
    /// Inferred by heuristics or an LLM from observed behaviour (weight 0.7).
    ///
    /// Examples: rolling edit-window ratio, correction-repeat signal, reflection hook output.
    Behavioral,
    /// Materialized from recurrence statistics in the memory tree (weight 0.6).
    ///
    /// Examples: tree-topic hotness, `source_weight` per channel.
    Recurrence,
}

impl CueFamily {
    /// Weight multiplier for this cue family in the stability formula.
    ///
    /// Canonical values: `Explicit=1.0`, `Structural=0.9`, `Behavioral=0.7`,
    /// `Recurrence=0.6`. They live on the enum rather than in the detector
    /// because a producer on the far side of the module boundary has to be
    /// able to reason about how much its signal is worth without linking the
    /// detector.
    pub fn weight(self) -> f64 {
        match self {
            CueFamily::Explicit => 1.0,
            CueFamily::Structural => 0.9,
            CueFamily::Behavioral => 0.7,
            CueFamily::Recurrence => 0.6,
        }
    }
}

/// A single unit of learning evidence emitted by a producer and queued for the
/// stability detector.
///
/// Each candidate asserts a specific `(class, key, value)` triple alongside the
/// evidence that backs it. The detector aggregates competing candidates for the
/// same `(class, key)` pair and resolves them into a single cache entry, so two
/// producers disagreeing about the user's timezone is a normal input, not an
/// error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningCandidate {
    /// Which facet class this evidence touches.
    pub class: FacetClass,
    /// Canonical slug key within the class, e.g. `"verbosity"`, `"package_manager"`.
    ///
    /// Convention: `snake_case`, lowercase, no class prefix (the class carries that).
    pub key: String,
    /// Canonical value string, e.g. `"terse"`, `"pnpm"`, `"UTC+5:30"`.
    pub value: String,
    /// How this candidate was produced.
    pub cue_family: CueFamily,
    /// Pointer to the backing evidence in the memory substrate.
    pub evidence: EvidenceRef,
    /// Source-provided confidence hint, `0.0..=1.0`.
    ///
    /// This is an initial hint; the stability detector reweights it using the
    /// cue-family weight and recency decay.
    pub initial_confidence: f64,
    /// When this candidate was observed, as seconds since the Unix epoch.
    pub observed_at: f64,
}

#[cfg(test)]
#[path = "learning_tests.rs"]
mod tests;
