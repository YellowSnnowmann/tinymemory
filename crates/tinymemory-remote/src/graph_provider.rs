//! [`GraphMemoryProvider`] — a mandatory-three provider plus a native
//! [`MemoryGraph`] implementation.
//!
//! [`tinymemory_api::mandatory::MemoryTraitProvider`] advertises exactly Core,
//! Recall, and Portability and cannot advertise more: its `capabilities()` and
//! `as_*` accessors are fixed. An engine whose native API can *also* answer
//! graph queries (Cognee's knowledge graph, Mem0's graph memory once
//! configured with a graph store) needs a provider that advertises Graph too.
//!
//! Rather than duplicate the mandatory-family delegation per engine, this
//! composes any [`MemoryTraitProvider`] with an `Arc<dyn MemoryGraph>`: the
//! mandatory three delegate straight through, `capabilities()` adds
//! [`Capability::Graph`], and `as_graph()` returns the wrapped implementation.

use std::sync::Arc;

use async_trait::async_trait;
use tinymemory_api::capabilities::{Capabilities, Capability};
use tinymemory_api::error::MemoryError;
use tinymemory_api::health::MemoryHealth;
use tinymemory_api::provider::types::{ExportPage, ExportRecord, ImportOutcome, SourceScope};
use tinymemory_api::provider::{
    MemoryAnswer, MemoryConversationIngest, MemoryCore, MemoryDocumentIngest, MemoryEventIngest,
    MemoryGraph, MemoryLearningIngest, MemoryPortability, MemoryProvider, MemoryRecall,
};
use tinymemory_api::recall::OwnedRecallOpts;
use tinymemory_api::types::{MemoryCategory, MemoryEntry, MemoryTaint, NamespaceSummary};

/// A [`MemoryTraitProvider`] augmented with a native [`MemoryGraph`].
pub struct GraphMemoryProvider {
    base: Arc<dyn MemoryProvider>,
    graph: Arc<dyn MemoryGraph>,
}

impl std::fmt::Debug for GraphMemoryProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `dyn MemoryGraph` is not `Debug`; the mandatory half already renders
        // safely (see `MemoryTraitProvider`'s own impl).
        f.debug_struct("GraphMemoryProvider")
            .field("driver_id", &self.base.driver_id())
            .finish_non_exhaustive()
    }
}

impl GraphMemoryProvider {
    /// Compose `mandatory` with a native `graph` implementation.
    #[must_use]
    pub fn new(base: impl MemoryProvider, graph: Arc<dyn MemoryGraph>) -> Self {
        Self {
            base: Arc::new(base),
            graph,
        }
    }
}

#[async_trait]
impl MemoryCore for GraphMemoryProvider {
    async fn store(
        &self,
        namespace: &str,
        key: &str,
        content: &str,
        category: MemoryCategory,
        session_id: Option<&str>,
        taint: MemoryTaint,
    ) -> Result<(), MemoryError> {
        self.base
            .store(namespace, key, content, category, session_id, taint)
            .await
    }

    async fn get(&self, namespace: &str, key: &str) -> Result<Option<MemoryEntry>, MemoryError> {
        self.base.get(namespace, key).await
    }

    async fn forget(&self, namespace: &str, key: &str) -> Result<bool, MemoryError> {
        self.base.forget(namespace, key).await
    }

    async fn list(
        &self,
        namespace: Option<&str>,
        category: Option<&MemoryCategory>,
        session_id: Option<&str>,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        self.base.list(namespace, category, session_id).await
    }

    async fn namespaces(&self) -> Result<Vec<NamespaceSummary>, MemoryError> {
        self.base.namespaces().await
    }
}

#[async_trait]
impl MemoryRecall for GraphMemoryProvider {
    async fn recall(
        &self,
        query: &str,
        limit: usize,
        opts: &OwnedRecallOpts,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        self.base.recall(query, limit, opts, scope).await
    }
}

#[async_trait]
impl MemoryPortability for GraphMemoryProvider {
    async fn export_page(
        &self,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<ExportPage, MemoryError> {
        self.base.export_page(cursor, limit).await
    }

    async fn import_records(
        &self,
        records: Vec<ExportRecord>,
    ) -> Result<ImportOutcome, MemoryError> {
        self.base.import_records(records).await
    }
}

#[async_trait]
impl MemoryProvider for GraphMemoryProvider {
    fn driver_id(&self) -> &str {
        self.base.driver_id()
    }

    fn capabilities(&self) -> Capabilities {
        self.base.capabilities().with(Capability::Graph)
    }

    async fn health(&self) -> MemoryHealth {
        self.base.health().await
    }

    fn as_graph(&self) -> Option<&dyn MemoryGraph> {
        Some(self.graph.as_ref())
    }

    fn as_document_ingest(&self) -> Option<&dyn MemoryDocumentIngest> {
        self.base.as_document_ingest()
    }

    fn as_conversation_ingest(&self) -> Option<&dyn MemoryConversationIngest> {
        self.base.as_conversation_ingest()
    }

    fn as_learning_ingest(&self) -> Option<&dyn MemoryLearningIngest> {
        self.base.as_learning_ingest()
    }

    fn as_event_ingest(&self) -> Option<&dyn MemoryEventIngest> {
        self.base.as_event_ingest()
    }

    fn as_answer(&self) -> Option<&dyn MemoryAnswer> {
        self.base.as_answer()
    }
}
