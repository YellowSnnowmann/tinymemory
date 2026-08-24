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
pub use tinymemory_bus::provider::chunks::{ChunkDetail, ChunkEmbedding, ChunkQuery};

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
}
