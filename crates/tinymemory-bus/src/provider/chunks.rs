//! The chunks family: direct read access to the stored chunk tier.
//!
//! A driver advertising [`Capability::Chunks`](crate::capabilities::Capability::Chunks)
//! can list and fetch individual chunks, and hand back the embedding vectors it
//! holds for them.
//!
//! # Why a caller would want this rather than recall
//!
//! `MemoryRecall` answers "what is relevant to this
//! query" and owns its own ranking. This family answers "give me the rows
//! matching these filters", which is what a host-side search tool needs when it
//! is doing the ranking itself — cosine similarity with its own MMR
//! diversification, say, or a hybrid keyword/vector blend the engine does not
//! implement.
//!
//! That makes it a deliberately lower-level surface than the rest of the
//! contract, and the honest framing is that it leaks a little of the engine's
//! storage model: chunks, source kinds, embedding signatures. The alternative
//! was worse. Without it a host either reaches around the driver into the
//! engine's own tables — which is exactly the split-brain this contract exists
//! to end — or every ranking strategy has to be pushed into the engine and
//! versioned there.
//!
//! # Embeddings are keyed by signature, and the signature must match exactly
//!
//! `MemoryChunks::chunk_embeddings` takes a `model_signature` and returns
//! only vectors stored under it. A caller that computes that string differently
//! from the driver gets an empty result rather than an error — the vectors are
//! there, just filed under a name the caller did not ask for. That is a real
//! failure mode with a real precedent, and it is silent; see
//! `docs/specs/2026-08-13-memory-module-port.md` §3.
//!
//! # Two of these reads are diagnostic rather than retrieval
//!
//! [`ChunkScore`] and [`SourceIngestStatus`] answer "why is this here" and "how
//! far has this got", not "what is relevant". They are in this family because
//! both are keyed by the chunk tier — one by a chunk id, the other by the
//! prefix its ingest key carries — and because a caller that has the chunks
//! surface is exactly the caller with a browser to render them in.
//!
//! Neither is derivable from the reads above it. A score row lives in a table
//! of its own; a pending count spans three. That is the whole reason they are
//! members rather than arithmetic a host does over a chunk page, and each
//! type's own docs give the specific version of the argument.

use serde::{Deserialize, Serialize};

use crate::chunks::{Chunk, SourceKind};

/// Filters for `MemoryChunks::list_chunks`.
///
/// Every field is optional and they compose with AND. The default matches
/// everything the scope allows, bounded by the driver's own safety cap.
///
/// # An empty collection is "no constraint", never "match nothing"
///
/// The collection filters default to empty, and `Default` has to keep meaning
/// "everything the scope allows" — so an empty `Vec` places no constraint at
/// all. A caller that narrowed a list of ids down to none must therefore skip
/// the call rather than send it: an empty [`Self::ids`] reads as "no id
/// filter" and answers with the whole store.
///
/// # A driver that cannot apply a filter refuses the query
///
/// These fields are additive on the wire, which is exactly what makes them
/// invisible to a driver that has not implemented them — and a driver that
/// accepts a filter without applying it returns the rows the caller asked to
/// exclude. On [`Self::content_contains`] or [`Self::entity_ids`] those are
/// the rows a scoped browser was told not to show, so the failure is not a
/// wider page, it is a leak.
///
/// So a driver that cannot honour a filter answers
/// [`MemoryError::Invalid`](crate::error::MemoryError::Invalid) naming it.
/// Refusing is recoverable; silently widening the result is not, because
/// nothing downstream can tell that it happened.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkQuery {
    /// Restrict to one source kind.
    #[serde(default)]
    pub source_kind: Option<SourceKind>,
    /// Restrict to one logical source id.
    #[serde(default)]
    pub source_id: Option<String>,
    /// Restrict to one owner.
    #[serde(default)]
    pub owner: Option<String>,
    /// Inclusive lower bound on source time, epoch milliseconds.
    #[serde(default)]
    pub since_ms: Option<i64>,
    /// Inclusive upper bound on source time, epoch milliseconds.
    #[serde(default)]
    pub until_ms: Option<i64>,
    /// Maximum rows. The driver clamps this to its own cap — a caller cannot
    /// raise the ceiling by asking for more.
    #[serde(default)]
    pub limit: Option<usize>,
    /// Rows to skip, for pagination.
    #[serde(default)]
    pub offset: Option<usize>,
    /// Drop chunks marked dropped by the lifecycle.
    #[serde(default)]
    pub exclude_dropped: bool,
    /// Restrict to this explicit set of chunk ids.
    ///
    /// For a caller that already holds the ids — retrieval hits it wants the
    /// stored rows behind, a selection it is re-reading — rather than a filter
    /// over content. Ids the store does not hold contribute no row, so the
    /// result may be shorter than the input and must not be indexed by
    /// position against it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ids: Vec<String>,
    /// Restrict to any of these source kinds.
    ///
    /// The set form of [`Self::source_kind`] and not a replacement for it:
    /// both are applied, so a scalar naming one kind and a set naming another
    /// match nothing at all. A caller that wants several kinds leaves the
    /// scalar unset.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_kinds: Vec<SourceKind>,
    /// Restrict to any of these logical source ids.
    ///
    /// The set form of [`Self::source_id`], read the same way: both apply.
    /// Exact ids only — a prefix is not a source id, and honouring one here
    /// would quietly make `mem_src:x` select `mem_src:x-archive` too.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_ids: Vec<String>,
    /// Restrict to chunks the entity index has indexed against any of these
    /// entity ids.
    ///
    /// Ids live in [`EntityRef::id`]'s space, so a name resolved through the
    /// entity or retrieval families can be handed straight back here.
    ///
    /// This reads a **derived** index rather than the text: a chunk whose
    /// extraction has not run yet is absent even though its body names the
    /// entity. A caller rendering "chunks about X" against a store that is
    /// still indexing should say so rather than report that there are none.
    ///
    /// A chunk matching several of these entities is still **one** row. The
    /// join multiplies rows and the driver collapses them; a caller must not
    /// have to discover that adding a filter grew its page.
    ///
    /// [`EntityRef::id`]: crate::provider::types::EntityRef::id
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entity_ids: Vec<String>,
    /// Restrict to chunks carrying at least one indexed entity of any of these
    /// kinds (`person`, `organization`, `topic`, …).
    ///
    /// The same open vocabulary as [`EntityRef::kind`] and the same collapse
    /// as [`Self::entity_ids`]. It composes with `entity_ids` by AND like
    /// everything else, which is an intersection and not a union: a query
    /// naming both matches chunks holding one of those entities *and* one of
    /// those kinds, and the two need not be the same observation.
    ///
    /// [`EntityRef::kind`]: crate::provider::types::EntityRef::kind
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entity_kinds: Vec<String>,
    /// Restrict to chunks whose stored text contains this substring.
    ///
    /// A literal substring and not a query language: `%` and `_` match
    /// themselves, and there is no tokenisation, stemming, or ranking. Case is
    /// folded for ASCII only — which is what the stores behind this actually
    /// do, and promising full Unicode folding here would be a promise a
    /// SQLite `LIKE` cannot keep.
    ///
    /// It scans the text the driver holds inline, which for a chunk whose body
    /// was written to the content vault is the stored preview and not the
    /// whole document. So this narrows a browse; it does not replace
    /// `MemoryRecall`, which is what "search my memory" should reach for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_contains: Option<String>,
}

/// One chunk's stored embedding.
///
/// Returned as a list rather than a map because the wire form of a map keyed by
/// chunk id is a JSON object, and an id is caller-supplied text; a list keeps
/// the encoding independent of what an id happens to contain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChunkEmbedding {
    /// The chunk this vector belongs to.
    pub chunk_id: String,
    /// The vector, in the embedding space named by the requested signature.
    pub vector: Vec<f32>,
}

/// One chunk plus the per-chunk facts stored beside it.
///
/// # Why a detail view rather than four accessors
///
/// An inspection caller wants the row, its body, where the body lives, its
/// lifecycle state and whether it has been embedded. Exposing those as four
/// methods would read naturally in-process and cost **four bus round trips per
/// row** out of it — and this is used to render lists. One method, one trip.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChunkDetail {
    /// The chunk row.
    pub chunk: Chunk,
    /// The chunk's body as stored in the content vault, when it could be read.
    ///
    /// `None` means the vault read failed — distinct from an empty body, which
    /// is a legitimately empty chunk. A caller rendering a preview should fall
    /// back to [`Chunk::content`] rather than showing nothing.
    #[serde(default)]
    pub body: Option<String>,
    /// Path of the body in the content vault, when it has one.
    #[serde(default)]
    pub content_path: Option<String>,
    /// Lifecycle state (`active`, `dropped`, …); `None` when unrecorded.
    #[serde(default)]
    pub lifecycle_status: Option<String>,
    /// Whether an embedding vector exists for this chunk in **any** space.
    ///
    /// Not scoped to a signature on purpose: this answers "has this been
    /// embedded at all", which is what an inspection view wants. Asking whether
    /// a *particular* space has it is `MemoryChunks::chunk_embeddings`.
    pub has_embedding: bool,
}

/// One row of a chunk *listing*: a [`ChunkDetail`] without its body.
///
/// # Why a listing is not `Vec<ChunkDetail>`
///
/// [`ChunkDetail::body`] carries a contract a list cannot honour. `None` there
/// means **the vault read failed**, not "we did not look". Filling a page of
/// details truthfully would mean opening every row's file in the content vault
/// — fifty to a thousand of them for one screen of a browser — and filling it
/// with `None` instead would report every row as a failed read to a caller
/// whose next move is to tell the user their content is unreadable. Either the
/// list is unusably slow or the field lies; there is no third reading.
///
/// So the body is *absent* rather than empty. Everything else a list renders —
/// where the body lives, whether the row is still active, whether it has been
/// embedded — sits in a column beside the chunk and costs nothing to return,
/// which is the same one-trip argument [`ChunkDetail`] itself is built on.
///
/// The two are deliberately **not** interchangeable on the wire even though
/// this one is a subset of that one: decode a `ChunkListRow` as a
/// [`ChunkDetail`] and `body` defaults to `None`, turning "not read" into
/// "read failed". A caller that wants a body asks `MemoryChunks::chunk_detail`
/// for the single row it is opening.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChunkListRow {
    /// The chunk row, exactly as `MemoryChunks::list_chunks` would return it.
    pub chunk: Chunk,
    /// Path of the body in the content vault, when it has one.
    ///
    /// Where the body *is*, never what it *says* — a caller can show that a
    /// row is backed by a file, or open that one file, without the list having
    /// paid for every read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_path: Option<String>,
    /// Lifecycle state (`active`, `dropped`, …); `None` when unrecorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle_status: Option<String>,
    /// Whether an embedding vector exists for this chunk in **any** space.
    ///
    /// [`ChunkDetail::has_embedding`]'s reading exactly, and signature-blind
    /// for the same reason: a list is asking "has this been embedded at all".
    pub has_embedding: bool,
}

/// One logical source and what the driver holds for it.
///
/// The unit a memory browser lists above the chunks. A source is not a row in
/// any table — it is the `(source_kind, source_id)` group the chunk rows fall
/// into — so this is an aggregate, and a caller cannot assemble it from a
/// chunk page: it would have to list every chunk in the store to group them,
/// which is the query the page limit exists to prevent.
///
/// # What is deliberately not here
///
/// No display name. Turning `gmail:alice@example.com|bob@example.com` into
/// "bob@example.com" requires knowing which address is the user's, and that is
/// host policy resting on host state — the same line redaction and the source
/// safety rules already sit on. A driver that guessed would be guessing about
/// a person.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceTotal {
    /// Which kind of source this group is.
    pub source_kind: SourceKind,
    /// The logical source id the group is keyed by.
    pub source_id: String,
    /// Chunks the driver holds for the group.
    pub chunk_count: u64,
    /// Source time of the newest chunk in the group, epoch milliseconds.
    ///
    /// Not an `Option`, unlike the store-wide `most_recent_chunk_ms` on
    /// [`StoreStats`]: a group exists only because a chunk fell into it, so
    /// there is always a newest one. A source holding nothing is not a zero
    /// row here, it is absent from the list.
    ///
    /// [`StoreStats`]: crate::provider::types::StoreStats
    pub most_recent_ms: i64,
}

/// The admission score below which the engine's default policy tombstones a
/// chunk, so a caller can draw the same line the scorer drew.
///
/// # Why the number crosses at all
///
/// A browser rendering [`ChunkScore::total`] as a bar wants to show where the
/// keep/drop boundary sits, and a host-side copy of `0.3` is the kind of copy
/// that goes wrong silently: the driver retunes, every row on the screen keeps
/// its label, and only the line drawn under them is stale. There is nothing to
/// notice, because the rows still render.
///
/// # It is the default, not the effective threshold
///
/// A driver whose scoring policy was tuned admits at its own number, and this
/// constant does not follow it. [`ChunkScore::dropped`] is the verdict the
/// engine actually reached for that chunk; this is the reference line a gauge
/// is drawn against. A caller that recomputes `dropped` from `total` against
/// this constant will disagree with the driver on a tuned store — and the
/// driver is the one that was there.
pub const DEFAULT_DROP_THRESHOLD: f32 = 0.3;

/// The per-signal breakdown behind a chunk's admission score.
///
/// Every field is one term of the weighted sum that produced
/// [`ChunkScore::total`], carried unweighted: the caller sees what each signal
/// measured, not what the policy paid for it. That is the useful half for
/// "why was this dropped" — a chunk that failed on length and a chunk that
/// failed on a source it does not trust have the same total and different
/// stories.
///
/// # Two fields read back empty, by construction
///
/// [`Self::llm_importance`] is an admission-time input that the engine's score
/// table has no column for, so a stored row answers `0.0` for it however
/// strongly the extractor rated the chunk. It is carried anyway rather than
/// dropped from the shape, because a driver that *does* persist it should have
/// somewhere to put it, and because a field that is absent from the type reads
/// to the next caller as a signal that does not exist rather than as one this
/// store does not keep. The same applies to
/// [`ChunkScore::llm_importance_reason`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChunkScoreSignals {
    /// Length signal derived from the chunk's token count.
    #[serde(default)]
    pub token_count: f32,
    /// Lexical-diversity signal derived from the count of distinct words.
    #[serde(default)]
    pub unique_words: f32,
    /// Contribution from structural or front-matter metadata on the source.
    #[serde(default)]
    pub metadata_weight: f32,
    /// Contribution from the source's provenance or authority.
    #[serde(default)]
    pub source_weight: f32,
    /// Direct-engagement signal from user interaction with the chunk.
    #[serde(default)]
    pub interaction: f32,
    /// Signal proportional to the density of extracted entities in the chunk.
    #[serde(default)]
    pub entity_density: f32,
    /// LLM-derived importance rating in `[0.0, 1.0]`.
    ///
    /// `0.0` both when no LLM signal was available and when the driver does not
    /// persist one — see the type's own note. A caller must not read a zero
    /// here as "the model found this unimportant".
    #[serde(default)]
    pub llm_importance: f32,
}

/// One chunk's admission decision and the signals it was reached from.
///
/// The row the scorer wrote when it decided whether to keep the chunk. It is a
/// **diagnostic** read — "why is this here", or "why is this not" — and not
/// part of any ranking: [`crate::provider::retrieval`] owns relevance, and this
/// owns admission, which happened once, at ingest, against the whole store's
/// policy rather than against a query.
///
/// # Why the whole row crosses when a caller reads three fields
///
/// The first caller wants [`Self::total`], [`Self::dropped`] and the signals.
/// The second wants [`Self::reason`] beside them, because "dropped" with no
/// rationale is a verdict without an argument; the third wants
/// [`Self::computed_at_ms`], because a score from before the policy changed
/// explains a row the current policy would have kept. Each of those is a
/// widening of a shipped wire type, and the alternative — carrying the row the
/// engine already assembled — costs nothing: the columns are read in one
/// `query_row` whether or not they are returned.
///
/// # A missing row is not a zero score
///
/// `MemoryChunks::chunk_score` answers `None` for a chunk the scorer never
/// wrote a row for, which is a different fact from a chunk that scored `0.0`
/// and was kept anyway. A caller rendering "score: 0" for both reports a
/// judgement that was never made.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChunkScore {
    /// The chunk this rationale belongs to.
    pub chunk_id: String,
    /// The aggregate admission score — the single scalar the keep/drop
    /// decision was taken on, and the one [`DEFAULT_DROP_THRESHOLD`] is the
    /// reference line for.
    pub total: f32,
    /// The per-signal breakdown that produced [`Self::total`].
    #[serde(default)]
    pub signals: ChunkScoreSignals,
    /// Whether the chunk failed admission and was tombstoned rather than kept.
    ///
    /// The driver's own verdict, not a re-derivation. See
    /// [`DEFAULT_DROP_THRESHOLD`] for why the two can disagree.
    pub dropped: bool,
    /// The recorded rationale for the keep or drop, when one was written.
    ///
    /// Operator-facing prose in the driver's words, never localised and never
    /// parsed. `None` means no rationale was recorded, not that there was none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// When the score was computed, epoch milliseconds.
    ///
    /// Carried because admission is a decision taken *at a time*, under the
    /// policy in force then. A row scored before a retune is the explanation
    /// for a chunk today's policy would have judged differently, and without
    /// this field that explanation is unavailable.
    pub computed_at_ms: i64,
    /// The LLM's one-line explanation for its importance rating.
    ///
    /// `None` from a driver that does not persist it — which is every driver
    /// backed by the engine's current score table, for the reason
    /// [`ChunkScoreSignals::llm_importance`] gives. Carried so a driver that
    /// keeps it has somewhere to put it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_importance_reason: Option<String>,
}

/// One configured source to ask ingest progress about.
///
/// Two identifiers, because they are two different things and conflating them
/// is the bug this type exists to prevent. [`Self::source_id`] is the id in the
/// *host's* source registry — the thing a user configured, named and can
/// disable. [`Self::chunk_id_prefix`] is the key the ingest path stamps on the
/// chunk rows it writes, which is derived from the first but is not equal to
/// it: the engine's readers key chunks `mem_src:{source id}:{item}`, and its
/// connector sync keys them `{toolkit}:{connection id}:{document id}`, which
/// does not contain the registry id at all.
///
/// # Why the host supplies the prefix rather than the driver deriving it
///
/// The derivation is host policy over host state. It reads the source's kind,
/// its toolkit and its connection id out of the registry the host owns, and a
/// driver asked to redo it would need that registry — which is the coupling
/// this contract exists to remove. So the host, which already holds the entry,
/// states the prefix, and the driver answers only the question it can answer
/// from its own tables: how many rows are under this key.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceIngestQuery {
    /// The configured source's id, echoed back on the matching
    /// [`SourceIngestStatus`] so a caller can pair the rows without relying on
    /// order.
    pub source_id: String,
    /// The literal prefix of the chunk rows' ingest key for this source.
    ///
    /// A **literal** prefix, matched as text: a driver must treat any pattern
    /// metacharacter in it as itself. The convention is the one the engine's
    /// own readers write, `mem_src:{source id}:` and `{toolkit}:{connection
    /// id}:`, including the trailing separator — without it a source keyed
    /// `mem_src:src_a:` also counts the chunks of `mem_src:src_ab:`.
    pub chunk_id_prefix: String,
}

/// How far one configured source's ingest has got.
///
/// # Why this is not [`SourceTotal`]
///
/// [`SourceTotal`] is a `GROUP BY` over the chunk rows: it describes the groups
/// that *exist*. This is a per-question answer about the sources a host has
/// *configured*, and the two differ in exactly the places a status panel
/// depends on.
///
/// A source that has never synced has no chunk rows, so it forms no group and
/// is simply absent from `source_totals` — a dashboard built on that loses the
/// row rather than showing it idle, which reads as "this source is not
/// configured" instead of "this source has done nothing yet". So a row comes
/// back for **every** query, zero-filled when the prefix matches nothing.
///
/// It also carries [`Self::chunks_pending`], which no grouping of the chunk
/// table can produce: the predicate spans the embedding sidecar and the
/// re-embed skip ledger as well as the chunk row's own lifecycle column. A
/// caller that substituted a chunk count for it would report a store with
/// nothing in flight, which is the same answer a healthy store gives.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceIngestStatus {
    /// The configured source id from the query this row answers.
    pub source_id: String,
    /// Chunk rows the driver holds under the source's prefix.
    ///
    /// Every row, whatever its state — this is "how much has landed", and
    /// [`Self::chunks_pending`] is the part of it that is not finished yet, not
    /// a disjoint bucket. A caller showing progress renders
    /// `synced - pending` of `synced`, never `synced + pending`.
    pub chunks_synced: u64,
    /// Chunk rows still in flight: no embedding, not dropped by the lifecycle,
    /// and not recorded as deliberately skipped for re-embedding.
    ///
    /// All three exits are terminal and all three count as resolved. The
    /// negative form matters: "pending" is *not resolved*, so a driver that
    /// checks only for a missing embedding reports every dropped and every
    /// skipped chunk as eternally in flight, and a healthy source then shows
    /// work that never completes.
    pub chunks_pending: u64,
    /// Source time of the newest chunk under the prefix, epoch milliseconds.
    ///
    /// `None` when the source has no chunks at all — unlike
    /// [`SourceTotal::most_recent_ms`], which is not optional precisely because
    /// a group cannot exist without one. Here the row exists because the source
    /// was *asked about*, so "never" is a real answer and has to be
    /// representable.
    ///
    /// # Freshness is deliberately not on the wire
    ///
    /// An `Active` / `Recent` / `Idle` label is arithmetic over this field and
    /// the current time — under 30 seconds, under 5 minutes, anything else —
    /// and putting it here would freeze the driver's clock into the answer. A
    /// caller rendering a label a minute after the call would show one derived
    /// from when the driver looked, not from now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_chunk_at_ms: Option<i64>,
}

#[cfg(test)]
#[path = "chunks_tests.rs"]
mod tests;
