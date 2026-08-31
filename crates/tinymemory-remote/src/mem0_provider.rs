//! Capability-accurate Mem0 provider composition.

use std::sync::Arc;

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use tinymemory_api::capabilities::{Capabilities, Capability};
use tinymemory_api::error::MemoryError;
use tinymemory_api::health::MemoryHealth;
use tinymemory_api::mandatory::MemoryTraitProvider;
use tinymemory_api::provider::types::{
    ExportPage, ExportRecord, ImportOutcome, IngestItem, IngestOutcome, SourceScope,
};
use tinymemory_api::provider::{
    MemoryConversationIngest, MemoryCore, MemoryPortability, MemoryProvider, MemoryRecall,
};
use tinymemory_api::recall::OwnedRecallOpts;
use tinymemory_api::traits::Memory;
use tinymemory_api::types::{MemoryCategory, MemoryEntry, MemoryTaint, NamespaceSummary};

use crate::common::encode;
use crate::mem0::Mem0Memory;
use crate::MEM0_DRIVER_ID;

/// Mem0 exposed as mandatory storage/recall plus conversation ingestion.
pub struct Mem0Provider {
    mandatory: MemoryTraitProvider,
}

impl std::fmt::Debug for Mem0Provider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Mem0Provider")
            .finish_non_exhaustive()
    }
}

impl Mem0Provider {
    /// Wrap a native Mem0 client.
    #[must_use]
    pub fn new(memory: Mem0Memory) -> Self {
        Self::from_memory(Arc::new(memory))
    }

    pub(crate) fn from_memory(memory: Arc<dyn Memory>) -> Self {
        Self {
            mandatory: MemoryTraitProvider::new(memory, MEM0_DRIVER_ID),
        }
    }
}

#[async_trait]
impl MemoryCore for Mem0Provider {
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
impl MemoryRecall for Mem0Provider {
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
impl MemoryPortability for Mem0Provider {
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
impl MemoryConversationIngest for Mem0Provider {
    async fn ingest_conversation(
        &self,
        messages: Vec<IngestItem>,
    ) -> Result<IngestOutcome, MemoryError> {
        let Some(first) = messages.first() else {
            return Ok(IngestOutcome::default());
        };
        let conversation_id = first.source_id.clone();
        if conversation_id.trim().is_empty() {
            return Err(MemoryError::Invalid(
                "conversation id must not be empty".to_string(),
            ));
        }
        if messages
            .iter()
            .any(|item| item.source_id != conversation_id || item.content.trim().is_empty())
        {
            return Err(MemoryError::Invalid(
                "conversation batches must contain one conversation and non-empty messages"
                    .to_string(),
            ));
        }

        let mut ids = Vec::with_capacity(messages.len());
        for (index, message) in messages.into_iter().enumerate() {
            let namespace = message
                .namespace
                .unwrap_or_else(|| format!("conversation:{conversation_id}"));
            let mut digest = Sha256::new();
            digest.update(conversation_id.as_bytes());
            digest.update(index.to_le_bytes());
            digest.update(message.content.as_bytes());
            if let Some(timestamp) = message.timestamp {
                digest.update(timestamp.to_rfc3339().as_bytes());
            }
            let key = format!("message-{}", encode(digest.finalize()));
            self.store(
                &namespace,
                &key,
                &message.content,
                MemoryCategory::Conversation,
                Some(&conversation_id),
                message.taint,
            )
            .await?;
            ids.push(key);
        }

        Ok(IngestOutcome {
            written: u32::try_from(ids.len()).unwrap_or(u32::MAX),
            ids,
            ..IngestOutcome::default()
        })
    }
}

#[async_trait]
impl MemoryProvider for Mem0Provider {
    fn driver_id(&self) -> &str {
        MEM0_DRIVER_ID
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::mandatory().with(Capability::ConversationIngest)
    }

    async fn health(&self) -> MemoryHealth {
        self.mandatory.health().await
    }

    fn as_conversation_ingest(&self) -> Option<&dyn MemoryConversationIngest> {
        Some(self)
    }
}
