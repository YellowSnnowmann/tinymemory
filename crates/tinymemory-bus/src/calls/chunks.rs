//! The persisted chunk model and its embeddings.
//!
//! One [`BusCall`] per member; see [`crate::calls`] for how they are used.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::calls::BusCall;
use crate::error::Error;
use crate::names::methods;
use crate::types;

/// Arguments for `ListChunks`.
///
/// Chunks matching the query, size-checked.
///
/// `ChunkQuery::limit` bounds rows, not bytes, and a chunk carries full
/// content — so this is one of the methods where the ceiling matters most.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListChunks {
    /// The `query` argument — wire position 0.
    pub query: types::ChunkQuery,
    /// The `scope` argument — wire position 1.
    pub scope: Option<types::SourceScope>,
}

impl BusCall for ListChunks {
    const METHOD: &'static str = methods::LIST_CHUNKS;

    type Response = Vec<types::Chunk>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.query, self.scope)).map_err(Error::Encode)
    }
}

/// Arguments for `GetChunk`.
///
/// One chunk, size-checked.
///
/// A single object is checked for the same reason a list is: the ceiling is
/// a property of the frame, not of the row count, and one chunk carries
/// full content with no bound of its own. A list of one that is refused
/// while the singular read of the same chunk succeeds would be an odd
/// contract to explain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetChunk {
    /// The `chunk_id` argument — wire position 0.
    pub chunk_id: String,
}

impl BusCall for GetChunk {
    const METHOD: &'static str = methods::GET_CHUNK;

    type Response = Option<types::Chunk>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.chunk_id,)).map_err(Error::Encode)
    }
}

/// Arguments for `ChunkDetail`.
///
/// One chunk plus its metadata, size-checked.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkDetail {
    /// The `chunk_id` argument — wire position 0.
    pub chunk_id: String,
}

impl BusCall for ChunkDetail {
    const METHOD: &'static str = methods::CHUNK_DETAIL;

    type Response = Option<types::ChunkDetail>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.chunk_id,)).map_err(Error::Encode)
    }
}

/// Arguments for `StorageKinds`.
///
/// Takes no arguments, so it encodes as an empty positional array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageKinds;

impl BusCall for StorageKinds {
    const METHOD: &'static str = methods::STORAGE_KINDS;

    type Response = Vec<String>;

    fn into_args(self) -> crate::Result<Value> {
        Ok(Value::Array(Vec::new()))
    }
}

/// Arguments for `ChunkEmbeddings`.
///
/// Embedding vectors are the largest thing this interface returns.
///
/// A 1536-dimension vector encodes to roughly 10 KiB of JSON, so a few
/// hundred chunks reach the frame ceiling on their own. Checked for the same
/// reason `List` is, and refused by name rather than truncated — a short
/// batch is indistinguishable from "those chunks have no vector".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkEmbeddings {
    /// The `chunk_ids` argument — wire position 0.
    pub chunk_ids: Vec<String>,
    /// The `model_signature` argument — wire position 1.
    pub model_signature: String,
}

impl BusCall for ChunkEmbeddings {
    const METHOD: &'static str = methods::CHUNK_EMBEDDINGS;

    type Response = Vec<types::ChunkEmbedding>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.chunk_ids, self.model_signature)).map_err(Error::Encode)
    }
}
