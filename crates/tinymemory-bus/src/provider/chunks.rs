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
    /// a *particular* space has it is [`MemoryChunks::chunk_embeddings`].
    pub has_embedding: bool,
}
