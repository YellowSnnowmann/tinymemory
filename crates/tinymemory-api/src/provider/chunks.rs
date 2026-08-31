//! The chunks family: direct read access to the stored chunk tier.
//!
//! A driver advertising [`Capability::Chunks`]
//! can list and fetch individual chunks, and hand back the embedding vectors it
//! holds for them.
//!
//! # Why a caller would want this rather than recall
//!
//! [`MemoryRecall`](super::MemoryRecall) answers "what is relevant to this
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
//! [`MemoryChunks::chunk_embeddings`] takes a `model_signature` and returns
//! only vectors stored under it. A caller that computes that string differently
//! from the driver gets an empty result rather than an error — the vectors are
//! there, just filed under a name the caller did not ask for. That is a real
//! failure mode with a real precedent, and it is silent; see
//! `docs/specs/2026-08-13-memory-module-port.md` §3.

use async_trait::async_trait;

use crate::capabilities::Capability;
use crate::chunks::Chunk;
use crate::error::MemoryError;
use crate::provider::types::SourceScope;

// The value types this family exchanges. They are defined in `tinymemory-bus`
// — they cross the module boundary, and a host that only makes calls must be
// able to name them without compiling this trait — and re-exported here so
// every historical path keeps resolving and the types stay the same types.
pub use tinymemory_bus::provider::chunks::{
    ChunkDetail, ChunkEmbedding, ChunkListRow, ChunkQuery, ChunkScore, ChunkScoreSignals,
    SourceIngestQuery, SourceIngestStatus, SourceTotal, DEFAULT_DROP_THRESHOLD,
};

/// Direct read access to the chunk tier.
///
/// Reached through [`MemoryProvider::as_chunks`](super::MemoryProvider::as_chunks).
#[async_trait]
pub trait MemoryChunks: Send + Sync {
    /// Chunks matching `query`, newest first.
    ///
    /// `scope` is applied **before** the row limit, so a disallowed source
    /// cannot starve permitted ones out of the result — filtering after the
    /// limit would let a noisy forbidden source silently empty the page.
    ///
    /// Passing `None` for `scope` means unrestricted, which is only correct for
    /// a caller that has already decided no source gate applies. It is a
    /// separate argument rather than a field of [`ChunkQuery`] to keep that
    /// decision explicit at every call site.
    ///
    /// # Errors
    ///
    /// Backend failures only; no match yields an empty vector.
    async fn list_chunks(
        &self,
        query: &ChunkQuery,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<Chunk>, MemoryError>;

    /// How many chunks `query` matches, ignoring its `limit` and `offset`.
    ///
    /// The predicate is [`Self::list_chunks`]'s, exactly: same filters, same
    /// `scope`, same fail-closed reading of an empty allowlist. Only the page
    /// bounds are dropped, because a total that moved as the caller paged
    /// through it would not be a total.
    ///
    /// # Why this is a member and not the caller's arithmetic
    ///
    /// A caller rendering "showing 20 of 431" cannot derive 431 from a page: it
    /// would have to list the whole match set unbounded, which is the query the
    /// row limit exists to prevent, and it would still be capped by the
    /// driver's own ceiling — silently, so 10,000 would read as the truth. The
    /// count has to be answered where the `WHERE` clause is.
    ///
    /// The two must be built from one predicate driver-side. A count that
    /// disagrees with the list beside it points the caller at pages that hold
    /// nothing, which is worse than not offering a count at all.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Unsupported`] from a driver that implements this family
    /// but predates this member — it is deliberately not derived from
    /// [`Self::list_chunks`] by default, because that default would silently
    /// answer with the driver's row cap instead of the real total. Otherwise
    /// backend failures only; no match yields `0`.
    async fn count_chunks(
        &self,
        _query: &ChunkQuery,
        _scope: Option<&SourceScope>,
    ) -> Result<u64, MemoryError> {
        Err(MemoryError::unsupported(Capability::Chunks))
    }

    /// The same rows [`Self::list_chunks`] returns, each carrying the stored
    /// facts a listing renders beside it.
    ///
    /// Same predicate, same `scope`, same newest-first order, same page
    /// bounds — a caller can swap one for the other without re-sorting, and
    /// [`Self::count_chunks`] labels either.
    ///
    /// # Why this is not `list_chunks` plus a call per row
    ///
    /// A browser page shows a chunk's vault path, its lifecycle state, and
    /// whether it has been embedded. Assembling those from
    /// [`Self::chunk_detail`] is one call per row — fifty to a thousand bus
    /// round trips for one screen, and each of those trips also reads the
    /// chunk's body off disk to fill a field the list will not display. That
    /// is precisely the fan-out [`ChunkDetail`]'s own docs exist to argue
    /// against, reintroduced one level up.
    ///
    /// # Why the rows are not `ChunkDetail`
    ///
    /// [`ChunkListRow`] is [`ChunkDetail`] minus its body, and the missing
    /// field is the point: `ChunkDetail::body` promises that `None` means the
    /// vault read *failed*, which a list can only honour by reading every
    /// file or by lying. That type's docs carry the full argument.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Unsupported`] from a driver that implements this family
    /// but not this member — not defaulted to [`Self::list_chunks`] with empty
    /// detail, which would report every row as unembedded and pathless.
    /// [`MemoryError::Invalid`] for a [`ChunkQuery`] filter the driver cannot
    /// apply, per that type's docs. Otherwise backend failures; no match
    /// yields an empty vector.
    async fn list_chunk_details(
        &self,
        query: &ChunkQuery,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<ChunkListRow>, MemoryError> {
        let _ = (query, scope);
        Err(MemoryError::unsupported(Capability::Chunks))
    }

    /// What the driver holds per logical source, newest source first.
    ///
    /// One row per `(source_kind, source_id)` group, ordered by
    /// [`SourceTotal::most_recent_ms`] descending — the same ordering
    /// [`Self::list_chunks`] uses, so a browser showing sources above chunks
    /// does not flip between two notions of "first". `limit` caps the rows and
    /// is clamped to the driver's own ceiling, exactly as
    /// [`ChunkQuery::limit`] is.
    ///
    /// `scope` filters the chunks the groups are computed *from*, not the
    /// groups afterwards: a scoped caller must not learn a forbidden source
    /// exists by seeing its total, and must not see permitted sources carrying
    /// counts that include rows it cannot read.
    ///
    /// # Why this is a member and not a fold over a chunk page
    ///
    /// A group is not a row in any table, so the only way to derive it is to
    /// list every chunk in the store and group them client-side — the
    /// unbounded query the page limit exists to prevent, and one that would
    /// silently answer from the driver's row cap instead of the whole store.
    /// It is [`Self::count_chunks`]'s argument applied to a `GROUP BY`: the
    /// aggregate has to be computed where the rows are.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Unsupported`] from a driver that implements this family
    /// but not this member. Otherwise backend failures; an empty store yields
    /// an empty vector.
    async fn source_totals(
        &self,
        limit: usize,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<SourceTotal>, MemoryError> {
        let _ = (limit, scope);
        Err(MemoryError::unsupported(Capability::Chunks))
    }

    /// One chunk by id.
    ///
    /// # Errors
    ///
    /// Backend failures only; an unknown id yields `Ok(None)`.
    async fn get_chunk(&self, chunk_id: &str) -> Result<Option<Chunk>, MemoryError>;

    /// One chunk with its stored detail, in a single call.
    ///
    /// # Errors
    ///
    /// Backend failures only; an unknown id yields `Ok(None)`.
    async fn chunk_detail(&self, chunk_id: &str) -> Result<Option<ChunkDetail>, MemoryError>;

    /// The storage-shape catalog this driver persists.
    ///
    /// Stable snake_case identifiers naming the *shapes* the engine stores
    /// (`chunk`, `vector`, `tree`, …), for a caller planning a multi-kind
    /// retrieval fan-out.
    ///
    /// # Why this is asked rather than compiled in
    ///
    /// It is the engine's own vocabulary — a second engine stores different
    /// shapes — so a host-side copy would drift the moment the engine changed
    /// and could never be right for a driver the host was not built against.
    /// It was a host-side copy, and it had already drifted: the tool's
    /// description advertised `content`, `document` and `graph`, none of which
    /// the engine has, and omitted `raw` and `entity`, which it does.
    ///
    /// Open vocabulary, for the same reason [`EntityMatch::kind`] is — a driver
    /// that grows a shape must not break a caller that has not heard of it.
    ///
    /// [`EntityMatch::kind`]: super::retrieval::EntityMatch::kind
    ///
    /// # Errors
    ///
    /// Backend failures only. A driver with a fixed catalog cannot fail here
    /// and should return it unconditionally.
    async fn storage_kinds(&self) -> Result<Vec<String>, MemoryError>;

    /// Stored embeddings for `chunk_ids`, in the space named by
    /// `model_signature`.
    ///
    /// Chunks with no vector under that signature are **omitted**, so the
    /// result may be shorter than the input and callers must not index by
    /// position. See the module docs for why a signature mismatch looks like an
    /// empty result rather than an error.
    ///
    /// # Errors
    ///
    /// Backend failures only.
    async fn chunk_embeddings(
        &self,
        chunk_ids: &[String],
        model_signature: &str,
    ) -> Result<Vec<ChunkEmbedding>, MemoryError>;

    /// One chunk's admission decision, and the signals it was reached from.
    ///
    /// The scorer's own row: what each signal measured, what they summed to,
    /// whether the chunk was kept, and why. A diagnostic read for "this
    /// document is in memory and that one is not" — not an input to ranking,
    /// which [`MemoryRetrieval`](super::MemoryRetrieval) owns and which happens
    /// per query rather than once at ingest.
    ///
    /// # Why the driver has to answer this
    ///
    /// The decision is a row in the driver's own score table, written at
    /// admission time under the policy in force then. Nothing in the chunk tier
    /// records it: [`Self::chunk_detail`] can say a chunk exists and is marked
    /// dropped, and cannot say what it scored or which signal it failed on. A
    /// caller cannot re-derive it either — re-running the scorer today would
    /// answer under today's policy, and produce a number the store never used.
    ///
    /// # `None` is "never scored", not "scored zero"
    ///
    /// A chunk with no score row was not judged, which is a different fact from
    /// a chunk judged uninteresting and kept anyway. Collapsing the two — by
    /// defaulting to a zeroed [`ChunkScore`] — reports a verdict that was never
    /// reached, on a screen whose entire purpose is to explain verdicts.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Unsupported`] from a driver that implements this family
    /// but keeps no admission record. That is the honest answer for a driver
    /// that admits everything, and it is deliberately not defaulted to
    /// `Ok(None)`: "this driver does not score" and "this chunk was not scored"
    /// are different answers, and only the first is true of every chunk.
    ///
    /// Otherwise backend failures; an unknown chunk id yields `Ok(None)`, the
    /// same as a known one with no score row — a caller inspecting a chunk it
    /// just listed cannot tell those apart and does not need to.
    async fn chunk_score(&self, chunk_id: &str) -> Result<Option<ChunkScore>, MemoryError> {
        let _ = chunk_id;
        Err(MemoryError::unsupported(Capability::Chunks))
    }

    /// How far ingest has got for each of the sources named in
    /// `source_prefixes`.
    ///
    /// One row per query, in the order asked, echoing
    /// [`SourceIngestQuery::source_id`] so a caller can pair them by value
    /// rather than by position. **A query whose prefix matches nothing still
    /// gets a row**, zero-filled — see below.
    ///
    /// # Why the caller supplies the prefix
    ///
    /// Because the caller is the only party that can. The prefix is derived
    /// from a configured source's kind, toolkit and connection id, which live
    /// in the host's source registry; a driver asked to derive it would need
    /// that registry, which is precisely the coupling this contract exists to
    /// remove. So the host states the key and the driver counts the rows under
    /// it — each side answering from what it actually holds.
    ///
    /// # Why this is not [`Self::source_totals`]
    ///
    /// Three differences, and a caller that substituted one for the other would
    /// get a result that renders as a healthy store.
    ///
    /// 1. [`SourceTotal`] has no pending count and none can be derived from it.
    ///    The predicate spans the embedding sidecar and the re-embed skip
    ///    ledger as well as the chunk's own lifecycle column, so a caller
    ///    reading only the chunk tier reports nothing in flight — which is what
    ///    a finished sync looks like.
    /// 2. `source_totals` returns the groups that *exist*. A configured source
    ///    that has never synced forms no group, so it vanishes from the answer
    ///    rather than appearing idle — and a source missing from a dashboard
    ///    reads as one that was never set up.
    /// 3. [`SourceTotal::source_id`] is the ingest key the chunk rows carry;
    ///    [`SourceIngestQuery::source_id`] is the registry entry a user
    ///    configured. For a connector source the two share no substring, so
    ///    matching them up is not a formatting difference a caller can paper
    ///    over.
    ///
    /// # Freshness is deliberately absent
    ///
    /// An `Active`/`Recent`/`Idle` label is arithmetic over
    /// [`SourceIngestStatus::last_chunk_at_ms`] and the current time. Answering
    /// it here would freeze the driver's clock into the reply, so a panel
    /// rendering the label a minute later would show how fresh the source was
    /// when the driver looked. The caller has the timestamp and its own clock.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Unsupported`] from a driver that implements this family
    /// but tracks no per-source ingest state — not defaulted to zero-filled
    /// rows, which would report every configured source as never synced.
    ///
    /// Otherwise backend failures, for the whole batch rather than per row: the
    /// counts come from one store, so a read that fails fails for all of them,
    /// and a partial answer would be indistinguishable from a set of genuinely
    /// empty sources. An empty `source_prefixes` yields an empty vector without
    /// touching the store.
    async fn source_ingest_status(
        &self,
        source_prefixes: &[SourceIngestQuery],
    ) -> Result<Vec<SourceIngestStatus>, MemoryError> {
        let _ = source_prefixes;
        Err(MemoryError::unsupported(Capability::Chunks))
    }
}
