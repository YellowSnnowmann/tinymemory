//! Scratch RED-check: a MemoryProvider double that mangles ':' to '_' in
//! `namespaces()`, exactly the historical bug, to confirm
//! `assert_namespaces_preserve_their_section` actually catches it.
//! DELETE BEFORE FINAL COMMIT.

use std::sync::Arc;

use tinymemory_conformance::InMemoryProvider;

#[tokio::test]
#[should_panic(expected = "namespaces() reported")]
async fn broken_driver_that_mangles_colons_is_caught() {
    // Reuse the real reference driver for storage, but wrap `namespaces()`
    // to simulate the sanitize_namespace bug: report the sectioned namespace
    // back with ':' collapsed to '_', same as the buggy `UnifiedMemory`.
    struct Mangling(InMemoryProvider);

    #[async_trait::async_trait]
    impl tinymemory_api::provider::MemoryCore for Mangling {
        async fn store(
            &self,
            namespace: &str,
            key: &str,
            content: &str,
            category: tinymemory_api::types::MemoryCategory,
            session_id: Option<&str>,
            taint: tinymemory_api::types::MemoryTaint,
        ) -> Result<(), tinymemory_api::error::MemoryError> {
            self.0
                .store(namespace, key, content, category, session_id, taint)
                .await
        }
        async fn get(
            &self,
            namespace: &str,
            key: &str,
        ) -> Result<Option<tinymemory_api::types::MemoryEntry>, tinymemory_api::error::MemoryError>
        {
            self.0.get(namespace, key).await
        }
        async fn forget(
            &self,
            namespace: &str,
            key: &str,
        ) -> Result<bool, tinymemory_api::error::MemoryError> {
            self.0.forget(namespace, key).await
        }
        async fn list(
            &self,
            namespace: Option<&str>,
            category: Option<&tinymemory_api::types::MemoryCategory>,
            session_id: Option<&str>,
        ) -> Result<Vec<tinymemory_api::types::MemoryEntry>, tinymemory_api::error::MemoryError>
        {
            self.0.list(namespace, category, session_id).await
        }
        async fn namespaces(
            &self,
        ) -> Result<Vec<tinymemory_api::types::NamespaceSummary>, tinymemory_api::error::MemoryError>
        {
            let mut summaries = self.0.namespaces().await?;
            for s in &mut summaries {
                s.namespace = s.namespace.replace(':', "_");
            }
            Ok(summaries)
        }
    }

    #[async_trait::async_trait]
    impl tinymemory_api::provider::MemoryRecall for Mangling {
        async fn recall(
            &self,
            query: &str,
            limit: usize,
            opts: &tinymemory_api::recall::OwnedRecallOpts,
            scope: Option<&tinymemory_api::source::SourceScope>,
        ) -> Result<Vec<tinymemory_api::types::MemoryEntry>, tinymemory_api::error::MemoryError>
        {
            self.0.recall(query, limit, opts, scope).await
        }
    }

    #[async_trait::async_trait]
    impl tinymemory_api::provider::MemoryKv for Mangling {
        async fn kv_set(
            &self,
            namespace: Option<&str>,
            key: &str,
            value: serde_json::Value,
        ) -> Result<(), tinymemory_api::error::MemoryError> {
            self.0.kv_set(namespace, key, value).await
        }
        async fn kv_get(
            &self,
            namespace: Option<&str>,
            key: &str,
        ) -> Result<Option<serde_json::Value>, tinymemory_api::error::MemoryError> {
            self.0.kv_get(namespace, key).await
        }
        async fn kv_delete(
            &self,
            namespace: Option<&str>,
            key: &str,
        ) -> Result<bool, tinymemory_api::error::MemoryError> {
            self.0.kv_delete(namespace, key).await
        }
    }

    impl tinymemory_api::provider::MemoryProvider for Mangling {
        fn driver_id(&self) -> &str {
            "mangling-double"
        }
        fn capabilities(&self) -> tinymemory_api::capabilities::CapabilitySet {
            self.0.capabilities()
        }
        fn as_tree(&self) -> Option<&dyn tinymemory_api::provider::TreeMemory> {
            None
        }
        fn as_graph(&self) -> Option<&dyn tinymemory_api::provider::GraphMemory> {
            None
        }
        fn as_ingest(&self) -> Option<&dyn tinymemory_api::provider::IngestMemory> {
            None
        }
    }

    let driver = Mangling(InMemoryProvider::new());
    tinymemory_conformance::assert_namespaces_preserve_their_section(&driver).await;
}
