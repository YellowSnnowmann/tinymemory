//! [`TinycortexMemory`] — a TinyCortex storage backend, seen through the
//! TinyMemory contract's [`Memory`] trait.
//!
//! Every method is a delegation plus a conversion. The two that are not purely
//! mechanical are called out below; both are cases where getting the
//! delegation "obviously right" would be wrong.

use std::sync::Arc;

use async_trait::async_trait;
use tinymemory_api::recall::OwnedRecallOpts;
use tinymemory_api::traits::Memory;
use tinymemory_api::types::{MemoryCategory, MemoryEntry, MemoryTaint, NamespaceSummary, RecallOpts};

use crate::convert::{
    category_to_tinycortex, entry_to_tinymemory, namespace_summary_to_tinymemory,
    recall_opts_to_tinycortex, taint_to_tinycortex,
};

/// A TinyCortex backend exposed as a TinyMemory [`Memory`].
pub struct TinycortexMemory {
    inner: Arc<dyn tinycortex::memory::Memory>,
}

impl std::fmt::Debug for TinycortexMemory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The backend is not `Debug` and may hold a path or a connection
        // string; its name is the part that is safe to render.
        f.debug_struct("TinycortexMemory")
            .field("backend", &self.inner.name())
            .finish()
    }
}

impl TinycortexMemory {
    /// Wrap a TinyCortex backend.
    #[must_use]
    pub fn new(inner: Arc<dyn tinycortex::memory::Memory>) -> Self {
        Self { inner }
    }

    /// The wrapped backend, for a caller that still needs engine-native access.
    #[must_use]
    pub fn inner(&self) -> &Arc<dyn tinycortex::memory::Memory> {
        &self.inner
    }
}

#[async_trait]
impl Memory for TinycortexMemory {
    fn name(&self) -> &str {
        self.inner.name()
    }

    async fn store(
        &self,
        namespace: &str,
        key: &str,
        content: &str,
        category: MemoryCategory,
        session_id: Option<&str>,
    ) -> anyhow::Result<()> {
        self.inner
            .store(
                namespace,
                key,
                content,
                category_to_tinycortex(category),
                session_id,
            )
            .await
    }

    /// **Overridden, and it must stay overridden.** The trait default degrades
    /// to [`Memory::store`], which drops the taint — so a backend reached
    /// through the default would launder externally-sourced content into
    /// internal-trust content. Forwarding to the engine's own
    /// `store_with_taint` keeps provenance intact end to end.
    async fn store_with_taint(
        &self,
        namespace: &str,
        key: &str,
        content: &str,
        category: MemoryCategory,
        session_id: Option<&str>,
        taint: MemoryTaint,
    ) -> anyhow::Result<()> {
        self.inner
            .store_with_taint(
                namespace,
                key,
                content,
                category_to_tinycortex(category),
                session_id,
                taint_to_tinycortex(taint),
            )
            .await
    }

    /// The engine's `RecallOpts` borrows its string fields, so the owned
    /// conversion has to outlive the borrow taken from it — hence the local
    /// binding rather than a temporary in the call.
    async fn recall(
        &self,
        query: &str,
        limit: usize,
        opts: RecallOpts<'_>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        let owned = OwnedRecallOpts::from(opts);
        let engine_owned = recall_opts_to_tinycortex(&owned);
        let hits = self
            .inner
            .recall(query, limit, (&engine_owned).into())
            .await?;
        Ok(hits.into_iter().map(entry_to_tinymemory).collect())
    }

    async fn recall_relevant_by_vector(
        &self,
        namespace: &str,
        query: &str,
        limit: usize,
        min_vector_similarity: f64,
    ) -> anyhow::Result<Vec<(String, String)>> {
        self.inner
            .recall_relevant_by_vector(namespace, query, limit, min_vector_similarity)
            .await
    }

    async fn get(&self, namespace: &str, key: &str) -> anyhow::Result<Option<MemoryEntry>> {
        Ok(self
            .inner
            .get(namespace, key)
            .await?
            .map(entry_to_tinymemory))
    }

    async fn list(
        &self,
        namespace: Option<&str>,
        category: Option<&MemoryCategory>,
        session_id: Option<&str>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        // `category` is borrowed on both sides, so the converted value needs a
        // binding to borrow from.
        let engine_category = category.cloned().map(category_to_tinycortex);
        let entries = self
            .inner
            .list(namespace, engine_category.as_ref(), session_id)
            .await?;
        Ok(entries.into_iter().map(entry_to_tinymemory).collect())
    }

    async fn forget(&self, namespace: &str, key: &str) -> anyhow::Result<bool> {
        self.inner.forget(namespace, key).await
    }

    async fn namespace_summaries(&self) -> anyhow::Result<Vec<NamespaceSummary>> {
        Ok(self
            .inner
            .namespace_summaries()
            .await?
            .into_iter()
            .map(namespace_summary_to_tinymemory)
            .collect())
    }

    async fn count(&self) -> anyhow::Result<usize> {
        self.inner.count().await
    }

    async fn health_check(&self) -> bool {
        self.inner.health_check().await
    }
}

#[cfg(test)]
#[path = "memory_test.rs"]
mod test;
