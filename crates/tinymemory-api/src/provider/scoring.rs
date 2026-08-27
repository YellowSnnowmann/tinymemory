//! [`MemoryScoring`] — scoring and NLP operations exposed over the bus.
//!
//! This family carries the three operations that currently keep
//! `tinymemory-core` in the host's build graph: entity extraction, text
//! embedding, and embedder identification. Moving them behind the bus lets
//! every call site that reached the engine directly for these purposes route
//! through the contract instead.
//!
//! ## Design note — why the host requests, not constructs
//!
//! The host previously constructed an embedder from config and called it
//! directly. That pattern cannot cross the bus: config is host-side, the
//! embedder lives in the module. The correct shape is that the host asks the
//! driver to perform the operation by intent (`embed_text`) and to identify
//! which provider is active (`embedder_slug`), delegating both the construction
//! and the execution to the driver.

use async_trait::async_trait;

use crate::error::MemoryError;

/// Scoring and NLP operations exposed over the bus.
#[async_trait]
pub trait MemoryScoring: Send + Sync {
    /// Extract canonical entity strings from a natural-language query.
    ///
    /// Returns `"<kind>:<value>"` strings in the same namespace as the indexed
    /// chunk entities. An empty result means the query is ungrounded — no
    /// entity anchors were found — which routes retrieval toward the global
    /// (dense) branch rather than the entity-indexed branch.
    ///
    /// Never fails: when the NLP backend is unavailable the implementation
    /// degrades to a regex extractor rather than returning an error.
    ///
    /// # Errors
    ///
    /// Only infrastructure failures (e.g. the module bus is down). The NLP
    /// step itself never errors — it degrades gracefully.
    async fn extract_entities(&self, query: &str) -> Result<Vec<String>, MemoryError>;

    /// Embed a text string with the active embedder.
    ///
    /// Returns a float vector; the length matches the active embedding
    /// dimension (currently 1024 for the default bge-m3 model).
    ///
    /// # Errors
    ///
    /// When no embedder is configured (`Unsupported`) or the embedding call
    /// fails (e.g. the Ollama server is unreachable).
    async fn embed_text(&self, text: &str) -> Result<Vec<f32>, MemoryError>;

    /// Stable string identifying which embedder provider is currently active.
    ///
    /// One of: `"ollama"`, `"none"`, `"custom"`, `"cloud"`, `"unconfigured"`.
    /// Used by the host to decide how to attribute embedding costs in the UI.
    ///
    /// # Errors
    ///
    /// Only infrastructure failures. Config resolution itself never errors.
    async fn embedder_slug(&self) -> Result<String, MemoryError>;
}
