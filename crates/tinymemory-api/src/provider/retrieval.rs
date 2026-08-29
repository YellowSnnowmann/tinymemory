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

use async_trait::async_trait;

use crate::error::MemoryError;
use crate::provider::types::SourceScope;
use crate::types::NamespaceMemoryHit;

// The value types this family exchanges. They are defined in `tinymemory-bus`
// — they cross the module boundary, and a host that only makes calls must be
// able to name them without compiling this trait — and re-exported here so
// every historical path keeps resolving and the types stay the same types.
pub use tinymemory_bus::provider::retrieval::{
    CoverWindowQuery, EntityMatch, FastRetrieveQuery, RetrievalHit, RetrievalNodeKind,
    RetrievalResponse, SourceRetrievalQuery,
};

/// The engine's deterministic retrieval primitives.
///
/// Reached through [`MemoryProvider::as_retrieval`](super::MemoryProvider::as_retrieval).
#[async_trait]
pub trait MemoryRetrieval: Send + Sync {
    /// Graph-walk retrieval: seed from the query's entities, expand, rank.
    ///
    /// Deterministic and LLM-free — the driver embeds the query and walks, but
    /// it does not synthesise prose. Composing an answer is the host's job.
    ///
    /// # Errors
    ///
    /// Backend and embedding failures. An empty query is
    /// [`MemoryError::Invalid`], not an empty result: retrieval with nothing to
    /// retrieve on is a caller mistake.
    async fn fast_retrieve(
        &self,
        query: &str,
        options: FastRetrieveQuery,
        scope: Option<&SourceScope>,
    ) -> Result<RetrievalResponse, MemoryError>;

    /// The minimum set of nodes covering a time window.
    ///
    /// # Errors
    ///
    /// Backend failures only. A window matching nothing yields an empty
    /// response.
    async fn cover_window(
        &self,
        window: &CoverWindowQuery,
        scope: Option<&SourceScope>,
    ) -> Result<RetrievalResponse, MemoryError>;

    /// Ranked retrieval over one source's summary tree.
    ///
    /// # Not to be confused with [`MemoryTree::query_source`](super::MemoryTree::query_source)
    ///
    /// They answer different questions and return different shapes. The tree
    /// family's returns the raw [`Chunk`](crate::chunks::Chunk)s
    /// filed under a source id, for a caller that wants the content. This one
    /// returns ranked [`RetrievalHit`]s across the source's *summary* tree —
    /// leaves and sealed summaries together, scored. The name differs precisely
    /// so a caller cannot reach for one meaning and get the other.
    ///
    /// # Errors
    ///
    /// Backend failures only; no match yields an empty response.
    async fn retrieve_source(
        &self,
        query: &SourceRetrievalQuery,
        scope: Option<&SourceScope>,
    ) -> Result<RetrievalResponse, MemoryError>;

    /// Walk one summary node's children, ranked.
    ///
    /// Named `retrieve_children` rather than `drill_down` because
    /// [`MemoryTree::drill_down`](super::MemoryTree::drill_down) already exists
    /// with different semantics — it returns a node and its direct children,
    /// where this returns ranked hits several levels deep. They are also two
    /// methods on one bus object, so the names could not collide even if the
    /// ambiguity were acceptable.
    ///
    /// `max_depth` bounds how far down the walk goes; `query` ranks the result
    /// when supplied and orders by the tree's own order when not.
    ///
    /// # Errors
    ///
    /// Backend failures only; an unknown `node_id` yields an empty vector
    /// rather than [`MemoryError::NotFound`] — "no children" and "no such node"
    /// are the same answer to this question.
    /// `scope` restricts which sources may answer, and is explicit for the
    /// reason given on [`Self::fast_retrieve`]: the walk filters by scope, and
    /// a driver reached over a transport has no ambient scope to read.
    async fn retrieve_children(
        &self,
        node_id: &str,
        max_depth: u32,
        query: Option<&str>,
        limit: Option<usize>,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<RetrievalHit>, MemoryError>;

    /// Hydrate specific leaf chunks into ranked-hit form, by chunk id.
    ///
    /// Ids that do not resolve are **omitted**, so the result may be shorter
    /// than the input and callers must not index by position.
    ///
    /// A chunk whose source falls outside `scope` is omitted the same way, so
    /// naming a chunk id directly cannot read around a source restriction.
    ///
    /// # Errors
    ///
    /// Backend failures only.
    async fn retrieve_leaves(
        &self,
        chunk_ids: &[String],
        scope: Option<&SourceScope>,
    ) -> Result<Vec<RetrievalHit>, MemoryError>;

    /// Namespace recall returning **scored** hits with their signal breakdown.
    ///
    /// # Why this exists next to [`MemoryRecall::recall`](super::MemoryRecall::recall)
    ///
    /// [`MemoryRecall`](super::MemoryRecall) returns ranked entries and keeps
    /// its scoring private. A host that wants to re-rank — a weight profile
    /// trading graph proximity against vector similarity, say — needs the
    /// *components*, not the verdict. This returns
    /// [`NamespaceMemoryHit`],
    /// whose `score_breakdown` carries them, so re-ranking is host policy over
    /// engine signals rather than a second retrieval implementation.
    ///
    /// `exclude_session_id` drops documents auto-saved for that session. It
    /// exists so a search issued mid-turn cannot retrieve the very request that
    /// triggered it — a self-echo the caller cannot filter afterwards, because
    /// by then the hit has already displaced a real result under the limit.
    ///
    /// # Errors
    ///
    /// Backend and embedding failures; an unknown namespace yields an empty
    /// vector.
    async fn recall_namespace_scored(
        &self,
        namespace: &str,
        query: &str,
        limit: usize,
        exclude_session_id: Option<&str>,
    ) -> Result<Vec<NamespaceMemoryHit>, MemoryError>;

    /// Namespace recall ordered by **recency**, with no query to rank against.
    ///
    /// # Why this is not [`Self::recall_namespace_scored`] with an empty query
    ///
    /// It looks like the same call with one argument left blank, and it is not.
    /// The two share a prefix — loading the namespace's documents and key-value
    /// records — and diverge after it. The scored path ranks candidates against
    /// the query text; handed an empty string it still runs the ranking, with
    /// nothing to rank against, and returns hits ordered by a similarity signal
    /// computed from nothing.
    ///
    /// This path never ranks. It orders by freshness and priority, which is
    /// what a caller asking "what is in this namespace" means, and what a
    /// context-assembly step needs when there is no user query yet.
    ///
    /// The substitution is dangerous precisely because it compiles, returns
    /// plausible hits, and quietly changes what the user gets back. A caller
    /// that *has* a real query wants the scored path; one that does not wants
    /// this.
    ///
    /// Hits carry the same [`NamespaceMemoryHit`] shape as the scored path, so
    /// a host re-ranking on engine signals treats the two uniformly.
    ///
    /// # Errors
    ///
    /// Backend failures; an unknown namespace yields an empty vector, which is
    /// a true statement about it rather than a fault.
    async fn recall_namespace_recent(
        &self,
        namespace: &str,
        limit: usize,
    ) -> Result<Vec<NamespaceMemoryHit>, MemoryError>;

    /// Free-text search over the entity index.
    ///
    /// `kinds` filters by classification; `None` matches every kind. This is
    /// how a caller resolves a name to a canonical id before a retrieval keyed
    /// on that id.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Invalid`] for an unrecognised kind in `kinds` — see the
    /// module docs. Backend failures otherwise; no match yields an empty
    /// vector.
    async fn search_entities(
        &self,
        query: &str,
        kinds: Option<&[String]>,
        limit: usize,
    ) -> Result<Vec<EntityMatch>, MemoryError>;
}
