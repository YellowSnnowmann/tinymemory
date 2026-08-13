//! The chunks family: direct read access to the stored chunk tier.
//!
//! A driver advertising [`Capability::Chunks`](crate::capabilities::Capability::Chunks)
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
use serde::{Deserialize, Serialize};

use crate::chunks::{Chunk, SourceKind};
use crate::error::MemoryError;
use crate::provider::types::SourceScope;

/// Filters for [`MemoryChunks::list_chunks`].
///
/// Every field is optional and they compose with AND. The default matches
/// everything the scope allows, bounded by the driver's own safety cap.
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

    /// One chunk by id.
    ///
    /// # Errors
    ///
    /// Backend failures only; an unknown id yields `Ok(None)`.
    async fn get_chunk(&self, chunk_id: &str) -> Result<Option<Chunk>, MemoryError>;

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
