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
use tinymemory_api::mandatory::MemoryTraitProvider;
use tinymemory_api::provider::types::{ExportPage, ExportRecord, ImportOutcome, SourceScope};
use tinymemory_api::provider::{
    MemoryCore, MemoryGraph, MemoryPortability, MemoryProvider, MemoryRecall,
};
use tinymemory_api::recall::OwnedRecallOpts;
use tinymemory_api::types::{MemoryCategory, MemoryEntry, MemoryTaint, NamespaceSummary};

/// A [`MemoryTraitProvider`] augmented with a native [`MemoryGraph`].
pub struct GraphMemoryProvider {
    mandatory: MemoryTraitProvider,
    graph: Arc<dyn MemoryGraph>,
}

impl std::fmt::Debug for GraphMemoryProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `dyn MemoryGraph` is not `Debug`; the mandatory half already renders
        // safely (see `MemoryTraitProvider`'s own impl).
        f.debug_struct("GraphMemoryProvider")
            .field("mandatory", &self.mandatory)
            .finish_non_exhaustive()
    }
}

impl GraphMemoryProvider {
    /// Compose `mandatory` with a native `graph` implementation.
    #[must_use]
    pub fn new(mandatory: MemoryTraitProvider, graph: Arc<dyn MemoryGraph>) -> Self {
        Self { mandatory, graph }
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
        self.mandatory
            .store(namespace, key, content, category, session_id, taint)
            .await
    }

    async fn get(&self, namespace: &str, key: &str) -> Result<Option<MemoryEntry>, MemoryError> {
        self.mandatory.get(namespace, key).await
    }

    async fn forget(&self, namespace: &str, key: &str) -> Result<bool, MemoryError> {
        self.mandatory.forget(namespace, key).await
    }

    async fn list(
        &self,
        namespace: Option<&str>,
        category: Option<&MemoryCategory>,
        session_id: Option<&str>,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        self.mandatory.list(namespace, category, session_id).await
    }

    async fn namespaces(&self) -> Result<Vec<NamespaceSummary>, MemoryError> {
        self.mandatory.namespaces().await
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
        self.mandatory.recall(query, limit, opts, scope).await
    }
}

#[async_trait]
impl MemoryPortability for GraphMemoryProvider {
    async fn export_page(
        &self,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<ExportPage, MemoryError> {
        self.mandatory.export_page(cursor, limit).await
    }

    async fn import_records(
        &self,
        records: Vec<ExportRecord>,
    ) -> Result<ImportOutcome, MemoryError> {
        self.mandatory.import_records(records).await
    }
}

#[async_trait]
impl MemoryProvider for GraphMemoryProvider {
    fn driver_id(&self) -> &str {
        self.mandatory.driver_id()
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::from_iter([
            Capability::Core,
            Capability::Recall,
            Capability::Portability,
            Capability::Graph,
        ])
    }

    async fn health(&self) -> MemoryHealth {
        self.mandatory.health().await
    }

    fn as_graph(&self) -> Option<&dyn MemoryGraph> {
        Some(self.graph.as_ref())
    }
}
