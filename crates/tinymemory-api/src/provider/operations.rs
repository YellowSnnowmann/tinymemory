//! Granular product-facing ingestion and answer operations.
//!
//! These traits deliberately split the legacy [`MemoryIngest`](super::MemoryIngest)
//! family. A connector may be excellent at conversations while having no
//! document, learning, or event model, and capability negotiation must be able
//! to express exactly that.

use async_trait::async_trait;

use crate::error::MemoryError;
use crate::learning::LearningCandidate;
use crate::provider::episodic::EpisodicEvent;
use crate::provider::types::{IngestItem, IngestOutcome};

pub use crate::operations::{AnswerCitation, AnswerRequest, AnswerResponse, AnswerStep};

/// Document ingestion with driver-owned chunking and indexing.
#[async_trait]
pub trait MemoryDocumentIngest: Send + Sync {
    /// Ingest one decoded document.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError::Invalid`] for rejected input and a backend error
    /// when persistence or indexing fails.
    async fn ingest_document(&self, document: IngestItem) -> Result<IngestOutcome, MemoryError>;
}

/// Ordered conversation ingestion.
#[async_trait]
pub trait MemoryConversationIngest: Send + Sync {
    /// Ingest all messages belonging to one conversation.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError::Invalid`] when the batch mixes conversations or
    /// contains invalid content, otherwise backend failures.
    async fn ingest_conversation(
        &self,
        messages: Vec<IngestItem>,
    ) -> Result<IngestOutcome, MemoryError>;
}

/// Ingestion of already-extracted learnings.
#[async_trait]
pub trait MemoryLearningIngest: Send + Sync {
    /// Persist one learning candidate and its evidence pointer.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError::Invalid`] for malformed confidence or keys,
    /// otherwise backend failures.
    async fn ingest_learning(
        &self,
        learning: LearningCandidate,
    ) -> Result<IngestOutcome, MemoryError>;
}

/// Ingestion of raw durable events.
#[async_trait]
pub trait MemoryEventIngest: Send + Sync {
    /// Persist one event.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError::Invalid`] for malformed event data, otherwise
    /// backend failures.
    async fn ingest_event(&self, event: EpisodicEvent) -> Result<IngestOutcome, MemoryError>;
}

/// Agentic retrieval that synthesises a grounded answer.
#[async_trait]
pub trait MemoryAnswer: Send + Sync {
    /// Retrieve evidence and synthesize an answer with citations.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError::Invalid`] for an empty query and backend or
    /// inference errors when retrieval or synthesis fails.
    async fn answer(&self, request: AnswerRequest) -> Result<AnswerResponse, MemoryError>;
}
