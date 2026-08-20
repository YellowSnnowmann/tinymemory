//! Stable host path for tinycortex-owned namespace memory contracts.

pub use crate::engine::backend::{
    GraphRelationRecord, MemoryItemKind, MemoryKvRecord, NamespaceDocumentInput,
    NamespaceMemoryHit, NamespaceQueryResult, NamespaceRetrievalContext, RetrievalScoreBreakdown,
    StoredMemoryDocument,
};

pub(crate) use crate::engine::backend::types::GLOBAL_NAMESPACE;
