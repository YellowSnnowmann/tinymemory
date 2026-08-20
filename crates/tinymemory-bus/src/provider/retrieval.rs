//! The retrieval family: the engine's deterministic retrieval primitives.
//!
//! A driver advertising [`Capability::Retrieval`](crate::capabilities::Capability::Retrieval)
//! exposes graph-walk retrieval, time-window coverage, and entity-index search
//! — the LLM-free primitives a host composes an answer from.
//!
//! # Separate from [`MemoryTree`](super::MemoryTree), on purpose
//!
//! The tree family navigates a known node: query one source, drill into
//! children, seal, cascade. These three answer questions about the store as a
//! whole, and they return a different shape — ranked hits with scores and a
//! truncation flag, not a node and its children.
//!
//! They are also, mechanically, why this is a new family rather than three more
//! `MemoryTree` methods: adding a method to a family a driver may already
//! advertise is a **major** contract bump, because negotiation cannot protect a
//! caller from a method an older driver never implemented.
//!
//! # Entity kinds travel as strings, not as an enum
//!
//! The engine's own `EntityKind` is `#[non_exhaustive]` and has grown twice.
//! A closed enum here would mean that the first time an engine emits a kind
//! this build has not heard of, the **response fails to deserialize** — a new
//! entity category would break retrieval outright rather than showing up as an
//! unfamiliar label.
//!
//! So [`EntityMatch::kind`] is an open vocabulary: a snake_case string the
//! caller passes through. Known values today are `email`, `url`, `handle`,
//! `hashtag`, `person`, `organization`, `location`, `event`, `product`,
//! `datetime`, `technology`, `artifact`, `quantity`, `misc`, `topic`.
//!
//! Requests are the opposite case and are validated: an unknown kind in
//! [`MemoryRetrieval::search_entities`]'s filter is a caller mistake the driver
//! reports as [`MemoryError::Invalid`], because silently matching nothing would
//! look identical to a genuine empty result.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::chunks::SourceKind;

/// Whether a hit is a raw leaf or a sealed summary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalNodeKind {
    /// A stored chunk, tree level 0.
    Leaf,
    /// A sealed summary node, tree level ≥ 1.
    Summary,
}

/// One ranked retrieval result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RetrievalHit {
    /// Chunk id for a leaf, summary-node id for a summary. Globally unique.
    pub node_id: String,
    /// Leaf or summary.
    pub node_kind: RetrievalNodeKind,
    /// Provenance tree id; empty for a bare leaf not yet sealed into a tree.
    #[serde(default)]
    pub tree_id: String,
    /// Human-readable tree scope, e.g. `slack:#eng`; empty for a bare leaf.
    #[serde(default)]
    pub tree_scope: String,
    /// Tree level: 0 for a leaf chunk, ≥ 1 for a summary.
    pub level: u32,
    /// Raw chunk text, or sealed summary text.
    pub content: String,
    /// Canonical entity ids referenced by this node; empty on leaves.
    #[serde(default)]
    pub entities: Vec<String>,
    /// Topic tags for this node.
    #[serde(default)]
    pub topics: Vec<String>,
    /// Inclusive start of the node's time coverage.
    pub time_range_start: DateTime<Utc>,
    /// Inclusive end of the node's time coverage.
    pub time_range_end: DateTime<Utc>,
    /// Relevance, higher is better.
    ///
    /// **Not comparable across primitives or across drivers.** A `fast_retrieve`
    /// score and a `cover_window` score are produced by different rankers;
    /// merging two result sets by score would be meaningless.
    pub score: f32,
    /// Ids one level down; empty on leaves.
    #[serde(default)]
    pub child_ids: Vec<String>,
    /// Chunk back-pointer, populated for leaves only.
    #[serde(default)]
    pub source_ref: Option<String>,
}

/// A page of ranked hits.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RetrievalResponse {
    /// The hits, already filtered, ranked and truncated to the caller's limit.
    pub hits: Vec<RetrievalHit>,
    /// Total matches **before** truncation.
    pub total: usize,
    /// `true` when `total > hits.len()`, i.e. a higher limit would return more.
    ///
    /// Carried explicitly rather than left for the caller to derive: it is the
    /// difference between "there is nothing else" and "there is more, ask
    /// again", and a caller that computed it from a page alone could not tell.
    pub truncated: bool,
}

/// Options for [`MemoryRetrieval::fast_retrieve`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FastRetrieveQuery {
    /// Maximum hits to return.
    pub limit: usize,
    /// How many graph hops to expand from the seed entities.
    pub max_hops: u32,
    /// Restrict to the last N days of source time.
    #[serde(default)]
    pub time_window_days: Option<u32>,
}

/// A time window to cover.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverWindowQuery {
    /// Inclusive lower bound, epoch milliseconds.
    pub since_ms: i64,
    /// Inclusive upper bound, epoch milliseconds.
    pub until_ms: i64,
    /// Restrict to one logical source.
    #[serde(default)]
    pub source_id: Option<String>,
    /// Restrict to one source kind.
    #[serde(default)]
    pub source_kind: Option<SourceKind>,
    /// Maximum nodes in the cover.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Filters for [`MemoryRetrieval::retrieve_source`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRetrievalQuery {
    /// Restrict to one logical source (the engine's "scope", e.g. `slack:#eng`).
    #[serde(default)]
    pub source_id: Option<String>,
    /// Restrict to one source kind.
    #[serde(default)]
    pub source_kind: Option<SourceKind>,
    /// Restrict to the last N days of source time.
    #[serde(default)]
    pub time_window_days: Option<u32>,
    /// Free-text query to rank against. `None` returns the newest nodes rather
    /// than ranking — the primitive is a browse as well as a search.
    #[serde(default)]
    pub query: Option<String>,
    /// Maximum hits.
    pub limit: usize,
}

/// One entity-index match.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityMatch {
    /// Canonical id, e.g. `email:alice@example.com` or `topic:phoenix`.
    pub canonical_id: String,
    /// Entity classification. An **open** snake_case vocabulary — see the
    /// module docs for why this is not an enum.
    pub kind: String,
    /// An example surface form that matched, for display.
    pub surface: String,
    /// Rows grouped under this canonical id.
    pub mention_count: u64,
    /// Epoch milliseconds of the newest mention.
    pub last_seen_ms: i64,
}
