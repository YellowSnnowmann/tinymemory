//! Lightweight TinyCortex composition with document ingestion.

use async_trait::async_trait;
use tinymemory_api::capabilities::{Capabilities, Capability};
use tinymemory_api::error::MemoryError;
use tinymemory_api::health::MemoryHealth;
use tinymemory_api::mandatory::MemoryTraitProvider;
use tinymemory_api::provider::types::{
    ExportPage, ExportRecord, ImportOutcome, IngestItem, IngestOutcome, SourceScope,
};
use tinymemory_api::provider::{
    MemoryCore, MemoryDocumentIngest, MemoryPortability, MemoryProvider, MemoryRecall,
};
use tinymemory_api::recall::OwnedRecallOpts;
use tinymemory_api::types::{MemoryCategory, MemoryEntry, MemoryTaint, NamespaceSummary};

/// TinyCortex's lightweight provider: mandatory storage plus document ingest.
pub struct TinycortexDocumentProvider {
    mandatory: MemoryTraitProvider,
}

impl std::fmt::Debug for TinycortexDocumentProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TinycortexDocumentProvider")
            .finish_non_exhaustive()
    }
}

impl TinycortexDocumentProvider {
    pub(crate) fn new(mandatory: MemoryTraitProvider) -> Self {
        Self { mandatory }
    }
}

#[async_trait]
impl MemoryCore for TinycortexDocumentProvider {
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
impl MemoryRecall for TinycortexDocumentProvider {
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
impl MemoryPortability for TinycortexDocumentProvider {
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
impl MemoryDocumentIngest for TinycortexDocumentProvider {
    async fn ingest_document(&self, document: IngestItem) -> Result<IngestOutcome, MemoryError> {
        if document.source_id.trim().is_empty() || document.content.trim().is_empty() {
            return Err(MemoryError::Invalid(
                "document source id and content must not be empty".to_string(),
            ));
        }
        let namespace = document
            .namespace
            .unwrap_or_else(|| format!("document:{}", document.source_id));
        let key = document
            .source_ref
            .map_or_else(|| document.source_id.clone(), |source| source.value);
        self.store(
            &namespace,
            &key,
            &document.content,
            MemoryCategory::Core,
            None,
            document.taint,
        )
        .await?;
        Ok(IngestOutcome {
            written: 1,
            ids: vec![format!("{namespace}/{key}")],
            ..IngestOutcome::default()
        })
    }
}

#[async_trait]
impl MemoryProvider for TinycortexDocumentProvider {
    fn driver_id(&self) -> &str {
        self.mandatory.driver_id()
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::mandatory().with(Capability::DocumentIngest)
    }

    async fn health(&self) -> MemoryHealth {
        self.mandatory.health().await
    }

    fn as_document_ingest(&self) -> Option<&dyn MemoryDocumentIngest> {
        Some(self)
    }
}
