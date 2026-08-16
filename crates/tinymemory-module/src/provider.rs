//! Complete TinyMemory provider backed by the module-owned engine.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use tinymemory::mandatory::MemoryTraitProvider;
use tinymemory_api::capabilities::Capabilities;
use tinymemory_api::chunks::Chunk;
use tinymemory_api::error::MemoryError;
use tinymemory_api::goals::GoalsDoc;
use tinymemory_api::health::MemoryHealth;
use tinymemory_api::host::{
    CloudProviderCreds, ComposioMode, LocalAiConfig, MemoryConfig, MemoryHostConfig,
    MemoryTreeConfig, SchedulerGateConfig,
};
use tinymemory_api::provider::types::{
    ChangeKind, DiffReport, EntityHit, EntityRef, ExportPage, ExportRecord, ImportOutcome,
    IngestItem, IngestOutcome, MaintenanceReport, SnapshotRef, SourceChange, SourceItem,
    SourceScope,
};
use tinymemory_api::provider::{
    AddressBookSeedOutcome, ChunkDetail, ChunkEmbedding, ChunkQuery, ConversationSegment,
    CoverWindowQuery, EntityMatch, EpisodicTurn, FacetType, FastRetrieveQuery, MemoryChunks,
    MemoryCore, MemoryDiff, MemoryDocuments, MemoryEntities, MemoryEpisodic, MemoryGoals,
    MemoryGraph, MemoryIngest, MemoryMaintenance, MemoryPeople, MemoryPortability, MemoryProfile,
    MemoryProvider, MemoryRecall, MemoryRetrieval, MemorySourceSink, MemoryToolMemory, MemoryTree,
    PersonHandle, PersonInteraction, PersonRecord, PersonScore, ProfileFacet, RankedPerson,
    ResolvedPerson, RetrievalHit, RetrievalResponse, SourceRetrievalQuery, UserState,
};
use tinymemory_api::recall::OwnedRecallOpts;
use tinymemory_api::tool_memory::ToolMemoryRule;
use tinymemory_api::tree::{IngestRequest, QueryResult, TreeStatus};
use tinymemory_api::types::{
    GraphRelationRecord, MemoryCategory, MemoryEntry, MemoryKvRecord, MemoryTaint,
    NamespaceDocumentInput, NamespaceMemoryHit, NamespaceRetrievalContext, NamespaceSummary,
    StoredMemoryDocument,
};
use tinymemory_core::store::{MemoryClient, MemoryClientRef};
use tinymemory_tinycortex::TinycortexMemory;

use crate::ModuleConfig;

/// The concrete, credential-free host configuration available inside a module.
#[derive(Debug, Clone)]
struct ModuleRuntimeConfig {
    workspace_dir: PathBuf,
    config_path: PathBuf,
    memory: MemoryConfig,
    memory_tree: MemoryTreeConfig,
    scheduler_gate: SchedulerGateConfig,
    local_ai: LocalAiConfig,
    embeddings_provider: Option<String>,
    memory_provider: Option<String>,
    default_model: Option<String>,
    default_temperature: f64,
    output_language: Option<String>,
    memory_sources: serde_json::Value,
}

impl From<&ModuleConfig> for ModuleRuntimeConfig {
    fn from(config: &ModuleConfig) -> Self {
        Self {
            workspace_dir: config.workspace_dir.clone(),
            config_path: config.workspace_dir.join("config.toml"),
            memory: config.memory.clone(),
            memory_tree: config.memory_tree.clone(),
            scheduler_gate: config.scheduler_gate.clone(),
            local_ai: config.local_ai.clone(),
            embeddings_provider: config.embeddings_provider.clone(),
            memory_provider: config.memory_provider.clone(),
            default_model: config.default_model.clone(),
            default_temperature: config.default_temperature,
            output_language: config.output_language.clone(),
            memory_sources: config.memory_sources.clone(),
        }
    }
}

#[async_trait]
impl MemoryHostConfig for ModuleRuntimeConfig {
    fn workspace_dir(&self) -> &PathBuf {
        &self.workspace_dir
    }
    fn config_path(&self) -> &PathBuf {
        &self.config_path
    }
    fn memory_tree_content_root(&self) -> PathBuf {
        self.memory_tree
            .content_dir
            .clone()
            .unwrap_or_else(|| self.workspace_dir.join("memory_tree/content"))
    }
    fn memory(&self) -> &MemoryConfig {
        &self.memory
    }
    fn memory_tree(&self) -> &MemoryTreeConfig {
        &self.memory_tree
    }
    fn scheduler_gate(&self) -> &SchedulerGateConfig {
        &self.scheduler_gate
    }
    fn local_ai(&self) -> &LocalAiConfig {
        &self.local_ai
    }
    fn cloud_providers(&self) -> &Vec<CloudProviderCreds> {
        static NONE: Vec<CloudProviderCreds> = Vec::new();
        &NONE
    }
    fn embeddings_provider(&self) -> Option<&str> {
        self.embeddings_provider.as_deref()
    }
    fn memory_provider(&self) -> Option<&str> {
        self.memory_provider.as_deref()
    }
    fn workload_local_model(&self, workload: &str) -> Option<String> {
        let route = match workload {
            "memory" => self.memory_provider.as_deref(),
            "embeddings" => self.embeddings_provider.as_deref(),
            _ => None,
        }?;
        route
            .strip_prefix("ollama:")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn to_arc(&self) -> Arc<dyn MemoryHostConfig> {
        Arc::new(self.clone())
    }
    fn api_url(&self) -> Option<&str> {
        None
    }
    fn effective_backend_api_url(&self) -> String {
        String::new()
    }
    fn session_token(&self) -> Result<Option<String>, String> {
        Ok(None)
    }
    fn default_model(&self) -> Option<&str> {
        self.default_model.as_deref()
    }
    fn default_temperature(&self) -> f64 {
        self.default_temperature
    }
    fn output_language(&self) -> Option<&str> {
        self.output_language.as_deref()
    }
    fn memory_sync_interval_secs(&self) -> Option<u64> {
        Some(0)
    }
    fn onboarding_completed(&self) -> bool {
        true
    }
    fn secrets_encrypt(&self) -> bool {
        false
    }
    fn composio(&self) -> ComposioMode {
        ComposioMode::default()
    }
    fn memory_sources_json(&self) -> anyhow::Result<serde_json::Value> {
        Ok(self.memory_sources.clone())
    }
    fn set_memory_sources_json(&mut self, value: serde_json::Value) -> anyhow::Result<()> {
        self.memory_sources = value;
        Ok(())
    }
    fn composio_source_caps_migration_version(&self) -> u32 {
        0
    }
    fn set_composio_source_caps_migration_version(&mut self, _version: u32) {}
    fn apply_env_overrides(&mut self) {}
    async fn save(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

/// The module-owned implementation of every TinyMemory capability family.
pub(crate) struct ModuleMemoryProvider {
    driver_id: String,
    mandatory: MemoryTraitProvider,
    client: MemoryClientRef,
    config: ModuleRuntimeConfig,
}

impl ModuleMemoryProvider {
    pub(crate) fn new(config: &ModuleConfig, client: Arc<MemoryClient>) -> Self {
        let memory = client.memory_handle();
        let mandatory = MemoryTraitProvider::new(
            Arc::new(TinycortexMemory::new(memory)),
            config.driver_id.clone(),
        );
        Self {
            driver_id: config.driver_id.clone(),
            mandatory,
            client,
            config: ModuleRuntimeConfig::from(config),
        }
    }

    fn other(context: &'static str, error: impl std::fmt::Display) -> MemoryError {
        MemoryError::Other(anyhow::anyhow!("{context}: {error}"))
    }

    fn cross<A: serde::Serialize, B: serde::de::DeserializeOwned>(
        value: &A,
        context: &'static str,
    ) -> Result<B, MemoryError> {
        let value = serde_json::to_value(value).map_err(|error| Self::other(context, error))?;
        serde_json::from_value(value).map_err(|error| Self::other(context, error))
    }
}

fn validate_ingest_item(item: &IngestItem) -> Result<(), MemoryError> {
    if item.taint != MemoryTaint::default() {
        return Err(MemoryError::Invalid(
            "ingest cannot preserve a non-default taint in the chunk tier".to_string(),
        ));
    }
    if item.content.trim().is_empty() {
        return Err(MemoryError::Invalid(
            "ingest content must not be empty".to_string(),
        ));
    }
    if let Some(mime) = item.mime.as_deref() {
        let mime = mime.trim().to_ascii_lowercase();
        let base = mime.split(';').next().unwrap_or("").trim();
        if !(base.starts_with("text/")
            || base.ends_with("+json")
            || base.ends_with("+xml")
            || matches!(
                base,
                "application/json" | "application/xml" | "application/x-ndjson"
            ))
        {
            return Err(MemoryError::Invalid(format!(
                "unsupported MIME '{mime}': ingest accepts decoded text only"
            )));
        }
    }
    Ok(())
}

async fn blocking<T, F>(
    config: ModuleRuntimeConfig,
    context: &'static str,
    run: F,
) -> Result<T, MemoryError>
where
    T: Send + 'static,
    F: FnOnce(&ModuleRuntimeConfig) -> anyhow::Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(move || run(&config))
        .await
        .map_err(|error| ModuleMemoryProvider::other(context, error))?
        .map_err(|error| ModuleMemoryProvider::other(context, error))
}

#[async_trait]
impl MemoryCore for ModuleMemoryProvider {
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
impl MemoryRecall for ModuleMemoryProvider {
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
impl MemoryPortability for ModuleMemoryProvider {
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
impl MemoryDocuments for ModuleMemoryProvider {
    async fn put_document(&self, input: NamespaceDocumentInput) -> Result<String, MemoryError> {
        let input = Self::cross(&input, "convert document input")?;
        self.client
            .put_doc(input)
            .await
            .map_err(|error| Self::other("put_document", error))
    }
    async fn get_document(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<StoredMemoryDocument>, MemoryError> {
        let document = self
            .client
            .get_document(namespace, key)
            .await
            .map_err(|error| Self::other("get_document", error))?;
        document
            .map(|document| Self::cross(&document, "convert stored document"))
            .transpose()
    }

    async fn list_documents(
        &self,
        namespace: Option<&str>,
    ) -> Result<serde_json::Value, MemoryError> {
        self.client
            .list_documents(namespace)
            .await
            .map_err(|error| Self::other("list_documents", error))
    }

    async fn list_namespaces(&self) -> Result<Vec<String>, MemoryError> {
        self.client
            .list_namespaces()
            .await
            .map_err(|error| Self::other("list_namespaces", error))
    }

    async fn delete_document(
        &self,
        namespace: &str,
        document_id: &str,
    ) -> Result<serde_json::Value, MemoryError> {
        self.client
            .delete_document(namespace, document_id)
            .await
            .map_err(|error| Self::other("delete_document", error))
    }

    async fn clear_namespace(&self, namespace: &str) -> Result<(), MemoryError> {
        self.client
            .clear_namespace(namespace)
            .await
            .map_err(|error| Self::other("clear_namespace", error))
    }
    async fn query_documents(
        &self,
        namespace: &str,
        query: &str,
        limit: usize,
    ) -> Result<NamespaceRetrievalContext, MemoryError> {
        let limit = u32::try_from(limit).unwrap_or(u32::MAX);
        let context = self
            .client
            .query_namespace_context_data(namespace, query, limit)
            .await
            .map_err(|error| Self::other("query_documents", error))?;
        Self::cross(&context, "convert document query result")
    }

    async fn recall_documents(
        &self,
        namespace: &str,
        limit: usize,
    ) -> Result<NamespaceRetrievalContext, MemoryError> {
        let limit = u32::try_from(limit).unwrap_or(u32::MAX);
        let context = self
            .client
            .recall_namespace_context_data(namespace, limit)
            .await
            .map_err(|error| Self::other("recall_documents", error))?;
        Self::cross(&context, "convert document recall result")
    }
}

#[async_trait]
impl MemoryIngest for ModuleMemoryProvider {
    async fn ingest_document(&self, item: IngestItem) -> Result<IngestOutcome, MemoryError> {
        validate_ingest_item(&item)?;
        let document = tinycortex::memory::ingest::canonicalize::document::DocumentInput {
            provider: item.source.as_str().to_string(),
            title: String::new(),
            body: item.content,
            modified_at: item.timestamp.unwrap_or_else(Utc::now),
            source_ref: item.source_ref.map(|source_ref| source_ref.value),
        };
        let result = tinymemory_core::ingest_pipeline::ingest_document_with_scope(
            &self.config,
            &item.source_id,
            &item.owner,
            item.tags,
            document,
            item.path_scope,
        )
        .await
        .map_err(|error| Self::other("ingest document", error))?;
        Ok(IngestOutcome {
            written: u32::try_from(result.chunks_written).unwrap_or(u32::MAX),
            skipped: if result.already_ingested {
                1
            } else {
                u32::try_from(result.chunks_dropped).unwrap_or(u32::MAX)
            },
            ids: result.chunk_ids,
        })
    }

    async fn ingest_chat(&self, messages: Vec<IngestItem>) -> Result<IngestOutcome, MemoryError> {
        let Some(first) = messages.first() else {
            return Ok(IngestOutcome::default());
        };
        let source_id = first.source_id.clone();
        let owner = first.owner.clone();
        let tags = first.tags.clone();
        let platform = first.source.as_str().to_string();
        for item in &messages {
            validate_ingest_item(item)?;
            if item.source_id != source_id {
                return Err(MemoryError::Invalid(
                    "ingest_chat batches must contain one conversation".to_string(),
                ));
            }
        }
        let batch = tinycortex::memory::ingest::canonicalize::chat::ChatBatch {
            platform,
            channel_label: source_id.clone(),
            messages: messages
                .into_iter()
                .map(
                    |item| tinycortex::memory::ingest::canonicalize::chat::ChatMessage {
                        author: item.owner,
                        timestamp: item.timestamp.unwrap_or_else(Utc::now),
                        text: item.content,
                        source_ref: item.source_ref.map(|source_ref| source_ref.value),
                    },
                )
                .collect(),
        };
        let result = tinymemory_core::ingest_pipeline::ingest_chat(
            &self.config,
            &source_id,
            &owner,
            tags,
            batch,
        )
        .await
        .map_err(|error| Self::other("ingest chat", error))?;
        Ok(IngestOutcome {
            written: u32::try_from(result.chunks_written).unwrap_or(u32::MAX),
            skipped: if result.already_ingested {
                1
            } else {
                u32::try_from(result.chunks_dropped).unwrap_or(u32::MAX)
            },
            ids: result.chunk_ids,
        })
    }
}

#[async_trait]
impl MemoryGraph for ModuleMemoryProvider {
    async fn kv_get(
        &self,
        namespace: Option<&str>,
        key: &str,
    ) -> Result<Option<MemoryKvRecord>, MemoryError> {
        let record = self
            .client
            .kv_records(namespace)
            .await
            .map_err(|error| Self::other("kv_get", error))?
            .into_iter()
            .find(|record| record.key == key);
        record
            .map(|record| Self::cross(&record, "convert key/value record"))
            .transpose()
    }
    async fn kv_put(
        &self,
        namespace: Option<&str>,
        key: &str,
        value: serde_json::Value,
    ) -> Result<(), MemoryError> {
        self.client
            .kv_set(namespace, key, &value)
            .await
            .map_err(|error| Self::other("kv_put", error))
    }

    async fn kv_delete(&self, namespace: Option<&str>, key: &str) -> Result<bool, MemoryError> {
        self.client
            .kv_delete(namespace, key)
            .await
            .map_err(|error| Self::other("kv_delete", error))
    }
    async fn kv_list(
        &self,
        namespace: Option<&str>,
        prefix: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryKvRecord>, MemoryError> {
        let mut records = self
            .client
            .kv_records(namespace)
            .await
            .map_err(|error| Self::other("kv_list", error))?;
        if let Some(prefix) = prefix {
            records.retain(|record| record.key.starts_with(prefix));
        }
        records.truncate(limit);
        Self::cross(&records, "convert key/value records")
    }
    async fn relations(
        &self,
        namespace: Option<&str>,
        subject: Option<&str>,
        predicate: Option<&str>,
        limit: usize,
    ) -> Result<Vec<GraphRelationRecord>, MemoryError> {
        let mut records = self
            .client
            .graph_relations(namespace, subject, predicate)
            .await
            .map_err(|error| Self::other("relations", error))?;
        records.truncate(limit);
        Self::cross(&records, "convert graph relations")
    }
    async fn put_relation(&self, relation: GraphRelationRecord) -> Result<(), MemoryError> {
        self.client
            .graph_upsert(
                relation.namespace.as_deref(),
                &relation.subject,
                &relation.predicate,
                &relation.object,
                &relation.attrs,
            )
            .await
            .map_err(|error| Self::other("put_relation", error))
    }
}

#[async_trait]
impl MemoryGoals for ModuleMemoryProvider {
    async fn goals(&self) -> Result<GoalsDoc, MemoryError> {
        let workspace = self.config.workspace_dir.clone();
        let document =
            tokio::task::spawn_blocking(move || tinycortex::memory::goals::store::load(&workspace))
                .await
                .map_err(|error| Self::other("join goals read", error))?
                .map_err(|error| Self::other("read goals", error))?;
        Self::cross(&document, "convert goals")
    }

    async fn set_goals(&self, goals: GoalsDoc) -> Result<(), MemoryError> {
        let workspace = self.config.workspace_dir.clone();
        let mut goals = Self::cross(&goals, "convert goals")?;
        tokio::task::spawn_blocking(move || {
            tinycortex::memory::goals::store::save(&workspace, &mut goals)
        })
        .await
        .map_err(|error| Self::other("join goals write", error))?
        .map_err(|error| Self::other("write goals", error))
    }
}

#[async_trait]
impl MemoryToolMemory for ModuleMemoryProvider {
    async fn tool_rules(&self, tool_name: &str) -> Result<Vec<ToolMemoryRule>, MemoryError> {
        let rules = tinymemory_core::tool_memory::tool_memory_store(self.client.memory_handle())
            .list_rules(tool_name)
            .await
            .map_err(|error| Self::other("list tool rules", error))?;
        Self::cross(&rules, "convert tool rules")
    }

    async fn put_tool_rule(&self, rule: ToolMemoryRule) -> Result<(), MemoryError> {
        let rule = Self::cross(&rule, "convert tool rule")?;
        tinymemory_core::tool_memory::tool_memory_store(self.client.memory_handle())
            .put_rule(rule)
            .await
            .map(|_| ())
            .map_err(|error| Self::other("put tool rule", error))
    }

    async fn delete_tool_rule(&self, tool_name: &str, rule_id: &str) -> Result<bool, MemoryError> {
        tinymemory_core::tool_memory::tool_memory_store(self.client.memory_handle())
            .delete_rule(tool_name, rule_id)
            .await
            .map_err(|error| Self::other("delete tool rule", error))
    }
}

#[async_trait]
impl MemoryTree for ModuleMemoryProvider {
    async fn append(&self, request: IngestRequest) -> Result<(), MemoryError> {
        tinycortex::memory::tree::runtime::store::validate_namespace(&request.namespace)
            .map_err(MemoryError::Invalid)?;
        if request.content.trim().is_empty() {
            return Err(MemoryError::Invalid(
                "content must not be empty".to_string(),
            ));
        }
        let namespace = request.namespace.trim().to_string();
        let content = request.content;
        let timestamp = request.timestamp.unwrap_or_else(Utc::now);
        let metadata = request.metadata;
        blocking(self.config.clone(), "append tree content", move |config| {
            tinymemory_core::tree::tree_runtime::store::buffer_write(
                config,
                &namespace,
                &content,
                &timestamp,
                metadata.as_ref(),
            )
            .map(|_| ())
        })
        .await
    }

    async fn query_source(
        &self,
        namespace: &str,
        source_id: &str,
        limit: usize,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<Chunk>, MemoryError> {
        tinycortex::memory::tree::runtime::store::validate_namespace(namespace)
            .map_err(MemoryError::Invalid)?;
        let query = tinymemory_core::store::chunks::ListChunksQuery {
            source_id: Some(source_id.to_string()),
            source_scope: scope.map(|scope| scope.allow.iter().cloned().collect::<HashSet<_>>()),
            limit: Some(limit),
            exclude_dropped: true,
            ..Default::default()
        };
        let chunks = blocking(self.config.clone(), "query source", move |config| {
            tinymemory_core::store::chunks::list_chunks(config, &query)
        })
        .await?;
        Self::cross(&chunks, "convert source chunks")
    }

    async fn drill_down(&self, namespace: &str, node_id: &str) -> Result<QueryResult, MemoryError> {
        tinycortex::memory::tree::runtime::store::validate_namespace(namespace)
            .map_err(MemoryError::Invalid)?;
        tinycortex::memory::tree::runtime::store::validate_node_id(node_id)
            .map_err(MemoryError::Invalid)?;
        let namespace = namespace.trim().to_string();
        let node_id = node_id.to_string();
        let lookup_namespace = namespace.clone();
        let lookup_node = node_id.clone();
        let result = blocking(self.config.clone(), "drill down", move |config| {
            let Some(node) = tinymemory_core::tree::tree_runtime::store::read_node(
                config,
                &lookup_namespace,
                &lookup_node,
            )?
            else {
                return Ok(None);
            };
            let children = tinymemory_core::tree::tree_runtime::store::read_children(
                config,
                &lookup_namespace,
                &lookup_node,
            )?;
            Ok(Some((node, children)))
        })
        .await?
        .ok_or_else(|| {
            MemoryError::NotFound(format!("tree node '{node_id}' not found in '{namespace}'"))
        })?;
        Self::cross(&result, "convert tree drill-down")
            .map(|(node, children)| QueryResult { node, children })
    }

    async fn seal(&self, namespace: &str) -> Result<TreeStatus, MemoryError> {
        tinycortex::memory::tree::runtime::store::validate_namespace(namespace)
            .map_err(MemoryError::Invalid)?;
        let namespace = namespace.trim().to_string();
        let read_namespace = namespace.clone();
        let buffered = blocking(self.config.clone(), "read tree buffer", move |config| {
            tinymemory_core::tree::tree_runtime::store::buffer_read(config, &read_namespace)
        })
        .await?;
        if !buffered.is_empty() {
            let (model, _) = tinymemory_core::chat_host::create_chat_model_with_model_id(
                "summarization",
                &self.config,
                self.config.default_temperature,
            )
            .map_err(|error| Self::other("create summarizer", error))?;
            tinymemory_core::tree::tree_runtime::engine::run_summarization(
                &self.config,
                model.as_ref(),
                &namespace,
                Utc::now(),
            )
            .await
            .map_err(|error| Self::other("seal tree", error))?;
        }
        let status = blocking(self.config.clone(), "read tree status", move |config| {
            tinymemory_core::tree::tree_runtime::store::get_tree_status(config, &namespace)
        })
        .await?;
        Self::cross(&status, "convert tree status")
    }

    async fn cascade(&self, namespace: &str) -> Result<TreeStatus, MemoryError> {
        tinycortex::memory::tree::runtime::store::validate_namespace(namespace)
            .map_err(MemoryError::Invalid)?;
        let namespace = namespace.trim().to_string();
        let read_namespace = namespace.clone();
        let status = blocking(self.config.clone(), "read tree status", move |config| {
            tinymemory_core::tree::tree_runtime::store::get_tree_status(config, &read_namespace)
        })
        .await?;
        if status.total_nodes == 0 {
            return Self::cross(&status, "convert tree status");
        }
        let (model, _) = tinymemory_core::chat_host::create_chat_model_with_model_id(
            "summarization",
            &self.config,
            self.config.default_temperature,
        )
        .map_err(|error| Self::other("create summarizer", error))?;
        let status = tinymemory_core::tree::tree_runtime::engine::rebuild_tree(
            &self.config,
            model.as_ref(),
            &namespace,
        )
        .await
        .map_err(|error| Self::other("cascade tree", error))?;
        Self::cross(&status, "convert tree status")
    }
}

#[async_trait]
impl MemoryEntities for ModuleMemoryProvider {
    async fn entities(
        &self,
        namespace: &str,
        query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<EntityHit>, MemoryError> {
        let namespace = namespace.to_string();
        let query_namespace = namespace.clone();
        let query = query.map(str::to_string);
        let rows = blocking(
            self.config.clone(),
            "list namespace entities",
            move |config| {
                tinymemory_core::store::entities::namespace_entities(
                    config,
                    &query_namespace,
                    query.as_deref(),
                    limit,
                )
            },
        )
        .await?
        .into_iter()
        .map(|hit| (hit.id, hit.kind, hit.name, hit.mentions))
        .collect::<Vec<_>>();

        let config = self.config.clone();
        blocking(config, "attach entity hotness", move |config| {
            Ok(rows
                .into_iter()
                .map(|(id, kind, name, mentions)| {
                    let hotness_key = format!("{namespace}:{id}");
                    let hotness = tinymemory_core::store::trees::hotness::get(config, &hotness_key)
                        .ok()
                        .flatten()
                        .map_or(0.0, |counters| {
                            f64::from(
                                tinymemory_core::tree_policy::TreePolicy::topic().topic_hotness(
                                    &id,
                                    &counters.stats(),
                                    Utc::now().timestamp_millis(),
                                ),
                            )
                        });
                    EntityHit {
                        entity: EntityRef { id, kind, name },
                        hotness,
                        mentions,
                    }
                })
                .collect())
        })
        .await
    }

    async fn entity_edges(
        &self,
        namespace: &str,
        entity_id: &str,
        limit: usize,
    ) -> Result<Vec<GraphRelationRecord>, MemoryError> {
        let subject = entity_id.to_string();
        let lookup = subject.clone();
        let namespace = namespace.to_string();
        let query_namespace = namespace.clone();
        let neighbours = blocking(self.config.clone(), "read entity edges", move |config| {
            tinymemory_core::store::entities::namespace_entity_edges(
                config,
                &query_namespace,
                &lookup,
                limit,
            )
        })
        .await?;
        Ok(neighbours
            .into_iter()
            .map(|(object, weight)| GraphRelationRecord {
                namespace: Some(namespace.clone()),
                subject: subject.clone(),
                predicate: "co_occurs_with".to_string(),
                object,
                attrs: serde_json::Value::Null,
                updated_at: 0.0,
                evidence_count: weight,
                order_index: None,
                document_ids: Vec::new(),
                chunk_ids: Vec::new(),
            })
            .collect())
    }

    async fn touch_entities(
        &self,
        namespace: &str,
        entity_ids: &[String],
    ) -> Result<(), MemoryError> {
        let entity_ids = entity_ids.to_vec();
        let namespace = namespace.to_string();
        blocking(self.config.clone(), "touch entities", move |config| {
            let now = Utc::now().timestamp_millis();
            for entity_id in entity_ids {
                let entity_id = format!("{namespace}:{entity_id}");
                let mut counters =
                    tinymemory_core::store::trees::hotness::get_or_fresh(config, &entity_id)?;
                counters.mention_count_30d = counters.mention_count_30d.saturating_add(1);
                counters.last_seen_ms = Some(now);
                counters.last_updated_ms = now;
                tinymemory_core::store::trees::hotness::upsert(config, &counters)?;
            }
            Ok(())
        })
        .await
    }
}

#[async_trait]
impl MemoryDiff for ModuleMemoryProvider {
    async fn capture_snapshot(&self, source_id: &str) -> Result<SnapshotRef, MemoryError> {
        let source = tinymemory_core::sources::registry::decode_memory_sources(&self.config)
            .into_iter()
            .find(|source| source.id == source_id)
            .ok_or_else(|| MemoryError::NotFound(source_id.to_string()))?;
        let snapshot = tinymemory_core::diff::ops::take_snapshot(
            &source,
            &self.config,
            tinymemory_core::diff::SnapshotTrigger::Manual,
        )
        .await
        .map_err(|error| Self::other("capture snapshot", error))?;
        Ok(SnapshotRef {
            id: snapshot.id,
            source_id: snapshot.source_id,
            label: snapshot.label,
            item_count: snapshot.item_count,
            taken_at_ms: snapshot.taken_at_ms,
        })
    }

    async fn snapshots(
        &self,
        source_id: &str,
        limit: usize,
    ) -> Result<Vec<SnapshotRef>, MemoryError> {
        let snapshots = tinymemory_core::diff::ops::list_snapshots(
            &self.config,
            Some(source_id),
            u32::try_from(limit).unwrap_or(u32::MAX),
        )
        .await
        .map_err(|error| Self::other("list snapshots", error))?;
        Ok(snapshots
            .into_iter()
            .map(|snapshot| SnapshotRef {
                id: snapshot.id,
                source_id: snapshot.source_id,
                label: snapshot.label,
                item_count: snapshot.item_count,
                taken_at_ms: snapshot.taken_at_ms,
            })
            .collect())
    }

    async fn diff(
        &self,
        source_id: &str,
        from: Option<&str>,
        to: &str,
    ) -> Result<DiffReport, MemoryError> {
        let result = tinymemory_core::diff::ops::compute_diff(&self.config, from, to, false)
            .await
            .map_err(|error| Self::other("compute diff", error))?;
        if result.source_id != source_id {
            return Err(MemoryError::Invalid(format!(
                "snapshot '{to}' belongs to a different source"
            )));
        }
        let changes = result
            .changes
            .into_iter()
            .map(|change| SourceChange {
                item_id: change.item_id,
                title: change.title,
                kind: match change.kind {
                    tinymemory_core::diff::ChangeKind::Added => ChangeKind::Added,
                    tinymemory_core::diff::ChangeKind::Removed => ChangeKind::Removed,
                    tinymemory_core::diff::ChangeKind::Modified => ChangeKind::Modified,
                },
                old_content_hash: change.old_content_hash,
                new_content_hash: change.new_content_hash,
            })
            .collect();
        Ok(DiffReport {
            source_id: result.source_id,
            from_snapshot_id: result.from_snapshot_id,
            to_snapshot_id: result.to_snapshot_id,
            added: result.summary.added,
            removed: result.summary.removed,
            modified: result.summary.modified,
            unchanged: result.summary.unchanged,
            changes,
        })
    }
}

#[async_trait]
impl MemorySourceSink for ModuleMemoryProvider {
    async fn accept_source_items(
        &self,
        source_id: &str,
        source_kind: &str,
        items: Vec<SourceItem>,
        taint: MemoryTaint,
    ) -> Result<IngestOutcome, MemoryError> {
        let namespace = format!("source:{source_id}");
        let mut outcome = IngestOutcome::default();
        for item in items {
            if item.item_id.trim().is_empty() {
                return Err(MemoryError::Invalid(
                    "source item_id must not be empty".to_string(),
                ));
            }
            let title = if item.title.trim().is_empty() {
                item.item_id.clone()
            } else {
                item.title.clone()
            };
            let input = NamespaceDocumentInput {
                namespace: namespace.clone(),
                key: item.item_id,
                title,
                content: item.content,
                source_type: source_kind.to_string(),
                priority: "medium".to_string(),
                tags: item.tags,
                metadata: serde_json::json!({
                    "sourceId": source_id,
                    "sourceKind": source_kind,
                    "url": item.url,
                    "mime": item.mime,
                    "updatedAtMs": item.updated_at_ms,
                }),
                category: "core".to_string(),
                session_id: None,
                document_id: None,
                taint,
            };
            let input = Self::cross(&input, "convert source document")?;
            match self.client.put_doc(input).await {
                Ok(id) => {
                    outcome.written = outcome.written.saturating_add(1);
                    outcome.ids.push(id);
                }
                Err(_) => {
                    outcome.skipped = outcome.skipped.saturating_add(1);
                }
            }
        }
        Ok(outcome)
    }

    async fn forget_source(&self, source_id: &str) -> Result<u64, MemoryError> {
        let namespace = format!("source:{source_id}");
        let listed = self
            .client
            .list_documents(Some(&namespace))
            .await
            .map_err(|error| Self::other("list source documents", error))?;
        let documents = listed
            .get("documents")
            .and_then(serde_json::Value::as_array)
            .map_or(0, Vec::len);
        if documents > 0 {
            self.client
                .clear_namespace(&namespace)
                .await
                .map_err(|error| Self::other("clear source documents", error))?;
        }
        let source_id = source_id.to_string();
        let chunks = blocking(self.config.clone(), "clear source chunks", move |config| {
            use tinymemory_core::store::chunks::{
                delete_chunks_by_source, delete_orphaned_source_tree, SourceKind,
            };
            let removed = delete_chunks_by_source(config, SourceKind::Document, &source_id)?;
            delete_orphaned_source_tree(config, SourceKind::Document, &source_id)?;
            Ok(removed)
        })
        .await?;
        Ok(u64::try_from(documents.saturating_add(chunks)).unwrap_or(u64::MAX))
    }
}

#[async_trait]
impl MemoryMaintenance for ModuleMemoryProvider {
    async fn reembed(&self) -> Result<MaintenanceReport, MemoryError> {
        let (examined, changed) =
            blocking(self.config.clone(), "enqueue re-embedding", move |config| {
                let total = tinymemory_core::queue::count_total(config).unwrap_or(0);
                let before = tinymemory_core::queue::count_by_status(
                    config,
                    tinymemory_core::queue::JobStatus::Ready,
                )
                .unwrap_or(0);
                tinymemory_core::queue::ensure_reembed_backfill(config);
                let after = tinymemory_core::queue::count_by_status(
                    config,
                    tinymemory_core::queue::JobStatus::Ready,
                )
                .unwrap_or(0);
                Ok((total, after.saturating_sub(before)))
            })
            .await?;
        Ok(MaintenanceReport {
            operation: "reembed".to_string(),
            examined,
            changed,
            findings: vec![format!("enqueued {changed} re-embedding job(s)")],
        })
    }

    async fn compact(&self) -> Result<MaintenanceReport, MemoryError> {
        let (examined, changed) =
            blocking(self.config.clone(), "compact memory queue", move |config| {
                Ok((
                    tinymemory_core::queue::count_total(config).unwrap_or(0),
                    u64::try_from(tinymemory_core::queue::recover_stale_locks(config).unwrap_or(0))
                        .unwrap_or(u64::MAX),
                ))
            })
            .await?;
        Ok(MaintenanceReport {
            operation: "compact".to_string(),
            examined,
            changed,
            findings: vec![format!("released {changed} stale queue lock(s)")],
        })
    }

    async fn consolidate(&self) -> Result<MaintenanceReport, MemoryError> {
        let (examined, enqueued) = blocking(
            self.config.clone(),
            "enqueue consolidation",
            move |config| {
                Ok((
                    tinymemory_core::queue::count_total(config).unwrap_or(0),
                    tinymemory_core::queue::scheduler::enqueue_flush_stale_job(config)
                        .map_err(anyhow::Error::msg)?,
                ))
            },
        )
        .await?;
        Ok(MaintenanceReport {
            operation: "consolidate".to_string(),
            examined,
            changed: u64::from(enqueued),
            findings: vec![if enqueued {
                "enqueued a stale-buffer flush".to_string()
            } else {
                "a stale-buffer flush is already queued".to_string()
            }],
        })
    }

    async fn doctor(&self) -> Result<MaintenanceReport, MemoryError> {
        let report = tinymemory_core::tree::health::async_run_doctor(&self.config).await;
        Ok(MaintenanceReport {
            operation: "doctor".to_string(),
            examined: report.counters.total_chunks,
            changed: 0,
            findings: report
                .stages
                .into_iter()
                .filter(|stage| !stage.ok)
                .map(|stage| format!("{}: {}", stage.stage, stage.note))
                .collect(),
        })
    }
}

#[async_trait]
impl MemoryProvider for ModuleMemoryProvider {
    fn driver_id(&self) -> &str {
        &self.driver_id
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::all()
    }
    async fn health(&self) -> MemoryHealth {
        if self.client.memory_handle().health_check().await {
            MemoryHealth::Ready
        } else {
            MemoryHealth::down("memory store is unavailable")
        }
    }
    fn as_documents(&self) -> Option<&dyn MemoryDocuments> {
        Some(self)
    }
    fn as_ingest(&self) -> Option<&dyn MemoryIngest> {
        Some(self)
    }
    fn as_graph(&self) -> Option<&dyn MemoryGraph> {
        Some(self)
    }
    fn as_goals(&self) -> Option<&dyn MemoryGoals> {
        Some(self)
    }
    fn as_tool_memory(&self) -> Option<&dyn MemoryToolMemory> {
        Some(self)
    }
    fn as_tree(&self) -> Option<&dyn MemoryTree> {
        Some(self)
    }
    fn as_entities(&self) -> Option<&dyn MemoryEntities> {
        Some(self)
    }
    fn as_diff(&self) -> Option<&dyn MemoryDiff> {
        Some(self)
    }
    fn as_sources(&self) -> Option<&dyn MemorySourceSink> {
        Some(self)
    }
    fn as_maintenance(&self) -> Option<&dyn MemoryMaintenance> {
        Some(self)
    }
    fn as_people(&self) -> Option<&dyn MemoryPeople> {
        Some(self)
    }
    fn as_chunks(&self) -> Option<&dyn MemoryChunks> {
        Some(self)
    }
    fn as_retrieval(&self) -> Option<&dyn MemoryRetrieval> {
        Some(self)
    }
    fn as_profile(&self) -> Option<&dyn MemoryProfile> {
        Some(self)
    }
    fn as_episodic(&self) -> Option<&dyn MemoryEpisodic> {
        Some(self)
    }
}

// ── People ───────────────────────────────────────────────────────────────────
//
// The conversions below destructure both sides exhaustively rather than
// round-tripping through `Self::cross`. That is deliberate. `cross` is a serde
// value round-trip, so it agrees only while the two crates' field *names* agree
// — and they already do not: the engine's `Interaction` names its timestamp
// `ts` where the contract names it `at`. A round-trip would compile and then
// fail at runtime on the first call.
//
// Destructuring makes the opposite trade: a field added or renamed on either
// side is a compile error here, which is the same rule
// `tinymemory-tinycortex::convert` follows and the same reasoning that governs
// the two copies of the contract itself.

/// The engine's people store for this module's workspace.
///
/// `for_workspace` caches per workspace directory, so this is a map lookup
/// after the first call rather than a database open.
fn people_store(
    workspace: &std::path::Path,
) -> Result<Arc<tinycortex::memory::people::store::PeopleStore>, MemoryError> {
    tinycortex::memory::people::store::for_workspace(workspace)
        .map_err(|error| MemoryError::Other(anyhow::anyhow!("open people store: {error}")))
}

fn handle_to_engine(handle: &PersonHandle) -> tinycortex::memory::people::types::Handle {
    use tinycortex::memory::people::types::Handle as EngineHandle;
    match handle {
        PersonHandle::IMessage(value) => EngineHandle::IMessage(value.clone()),
        PersonHandle::Email(value) => EngineHandle::Email(value.clone()),
        PersonHandle::DisplayName(value) => EngineHandle::DisplayName(value.clone()),
    }
}

fn handle_to_contract(handle: tinycortex::memory::people::types::Handle) -> PersonHandle {
    use tinycortex::memory::people::types::Handle as EngineHandle;
    match handle {
        EngineHandle::IMessage(value) => PersonHandle::IMessage(value),
        EngineHandle::Email(value) => PersonHandle::Email(value),
        EngineHandle::DisplayName(value) => PersonHandle::DisplayName(value),
    }
}

fn person_to_contract(person: tinycortex::memory::people::types::Person) -> PersonRecord {
    let tinycortex::memory::people::types::Person {
        id,
        display_name,
        primary_email,
        primary_phone,
        handles,
        created_at,
        updated_at,
    } = person;
    PersonRecord {
        id: id.to_string(),
        display_name,
        primary_email,
        primary_phone,
        handles: handles.into_iter().map(handle_to_contract).collect(),
        created_at: created_at.to_rfc3339(),
        updated_at: updated_at.to_rfc3339(),
    }
}

fn score_to_contract(
    score: tinycortex::memory::people::types::ScoreComponents,
    interaction_count: usize,
) -> PersonScore {
    let tinycortex::memory::people::types::ScoreComponents {
        recency,
        frequency,
        reciprocity,
        depth,
        score,
    } = score;
    PersonScore {
        recency,
        frequency,
        reciprocity,
        depth,
        score,
        interaction_count,
    }
}

/// Parse a caller-supplied person id.
///
/// `PersonRef` is opaque to the caller by contract, so an unparseable one is a
/// caller mistake — `Invalid`, not `NotFound`. Reporting `NotFound` would tell
/// a caller the id was well-formed but absent, which would send them looking
/// for a deleted person rather than at the id they built.
fn parse_person_id(
    person_id: &str,
) -> Result<tinycortex::memory::people::types::PersonId, MemoryError> {
    person_id
        .parse::<uuid::Uuid>()
        .map(tinycortex::memory::people::types::PersonId)
        .map_err(|_| MemoryError::Invalid(format!("malformed person id: {person_id}")))
}

#[async_trait]
impl MemoryPeople for ModuleMemoryProvider {
    async fn list_people(&self, limit: Option<usize>) -> Result<Vec<RankedPerson>, MemoryError> {
        let store = people_store(&self.config.workspace_dir)?;
        let people = store
            .list()
            .await
            .map_err(|error| Self::other("list people", error))?;

        let ids: Vec<_> = people.iter().map(|person| person.id).collect();
        let interactions = store
            .batch_interactions_for(&ids)
            .await
            .map_err(|error| Self::other("load interactions", error))?;

        let now = Utc::now();
        let mut ranked: Vec<RankedPerson> = people
            .into_iter()
            .map(|person| {
                let observed = interactions.get(&person.id).map_or(&[][..], Vec::as_slice);
                let closeness = tinycortex::memory::people::scorer::score(observed, now);
                RankedPerson {
                    person: person_to_contract(person),
                    score: score_to_contract(closeness, observed.len()),
                }
            })
            .collect();

        // Descending by composite score. `total_cmp` rather than `partial_cmp`:
        // a NaN from a degenerate score would make `partial_cmp` return `None`,
        // and an ordering that is not total is undefined behaviour's
        // well-behaved cousin — `sort_by` may panic or produce garbage order.
        ranked.sort_by(|a, b| b.score.score.total_cmp(&a.score.score));
        if let Some(limit) = limit {
            ranked.truncate(limit);
        }
        Ok(ranked)
    }

    async fn get_person(&self, person_id: &str) -> Result<Option<PersonRecord>, MemoryError> {
        let store = people_store(&self.config.workspace_dir)?;
        let id = parse_person_id(person_id)?;
        Ok(store
            .get(id)
            .await
            .map_err(|error| Self::other("get person", error))?
            .map(person_to_contract))
    }

    async fn resolve_handle(
        &self,
        handle: &PersonHandle,
        create_if_missing: bool,
    ) -> Result<Option<ResolvedPerson>, MemoryError> {
        let store = people_store(&self.config.workspace_dir)?;
        let resolver = tinycortex::memory::people::resolver::HandleResolver::new(&store);
        let engine_handle = handle_to_engine(handle);

        if create_if_missing {
            let (id, created) = resolver
                .resolve_or_create_with_status(&engine_handle)
                .await
                .map_err(|error| Self::other("resolve or create handle", error))?;
            return Ok(Some(ResolvedPerson {
                id: id.to_string(),
                created,
            }));
        }

        Ok(resolver
            .resolve(&engine_handle)
            .await
            .map_err(|error| Self::other("resolve handle", error))?
            .map(|id| ResolvedPerson {
                id: id.to_string(),
                created: false,
            }))
    }

    async fn add_handle_alias(
        &self,
        person_id: &str,
        handle: &PersonHandle,
    ) -> Result<(), MemoryError> {
        let store = people_store(&self.config.workspace_dir)?;
        let id = parse_person_id(person_id)?;
        if store
            .get(id)
            .await
            .map_err(|error| Self::other("look up person", error))?
            .is_none()
        {
            return Err(MemoryError::NotFound(format!("person {person_id}")));
        }
        store
            .add_alias(id, handle_to_engine(handle).canonicalize())
            .await
            .map_err(|error| Self::other("add handle alias", error))
    }

    async fn score_person(&self, person_id: &str) -> Result<Option<PersonScore>, MemoryError> {
        let store = people_store(&self.config.workspace_dir)?;
        let id = parse_person_id(person_id)?;
        if store
            .get(id)
            .await
            .map_err(|error| Self::other("look up person", error))?
            .is_none()
        {
            return Ok(None);
        }
        let interactions = store
            .interactions_for(id)
            .await
            .map_err(|error| Self::other("load interactions", error))?;
        Ok(Some(score_to_contract(
            tinycortex::memory::people::scorer::score(&interactions, Utc::now()),
            interactions.len(),
        )))
    }

    async fn record_interaction(&self, interaction: &PersonInteraction) -> Result<(), MemoryError> {
        let store = people_store(&self.config.workspace_dir)?;
        let PersonInteraction {
            person_id,
            at,
            is_outbound,
            length,
        } = interaction;
        let id = parse_person_id(person_id)?;
        let ts = chrono::DateTime::parse_from_rfc3339(at)
            .map_err(|error| MemoryError::Invalid(format!("malformed interaction time: {error}")))?
            .with_timezone(&Utc);
        if store
            .get(id)
            .await
            .map_err(|error| Self::other("look up person", error))?
            .is_none()
        {
            return Err(MemoryError::NotFound(format!("person {person_id}")));
        }
        store
            .record_interaction(tinycortex::memory::people::types::Interaction {
                person_id: id,
                ts,
                is_outbound: *is_outbound,
                length: *length,
            })
            .await
            .map_err(|error| Self::other("record interaction", error))
    }

    async fn seed_from_address_book(&self) -> Result<AddressBookSeedOutcome, MemoryError> {
        let store = people_store(&self.config.workspace_dir)?;
        let resolver = tinycortex::memory::people::resolver::HandleResolver::new(&store);
        let source = tinycortex::memory::people::address_book::SystemContactsSource;
        let (seeded, skipped) = resolver
            .seed_from_address_book(&source)
            .await
            .map_err(|error| Self::other("seed from address book", error))?;
        Ok(AddressBookSeedOutcome { seeded, skipped })
    }
}

// ── Chunks and Retrieval ─────────────────────────────────────────────────────
//
// Both families take the source scope as an **argument** and never read the
// ambient one. `tinymemory_core`'s in-process entry points resolve it from a
// task-local, which the host sets on its own side of the bus — it is simply not
// present in this process. Reading it here would yield `None`, and `None` means
// *unrestricted*, so a per-profile source gate would fail open. That is why the
// `*_scoped` variants exist and why these call them.

/// Convert a contract scope into the engine's allowlist form.
fn scope_to_engine(scope: Option<&SourceScope>) -> Option<HashSet<String>> {
    scope.map(|scope| scope.allow.iter().cloned().collect())
}

#[async_trait]
impl MemoryChunks for ModuleMemoryProvider {
    async fn list_chunks(
        &self,
        query: &ChunkQuery,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<Chunk>, MemoryError> {
        let ChunkQuery {
            source_kind,
            source_id,
            owner,
            since_ms,
            until_ms,
            limit,
            offset,
            exclude_dropped,
        } = query.clone();
        let engine_query = tinymemory_core::store::chunks::ListChunksQuery {
            source_kind: source_kind
                .map(|kind| Self::cross(&kind, "convert source kind"))
                .transpose()?,
            source_id,
            owner,
            since_ms,
            until_ms,
            limit,
            offset,
            source_scope: scope_to_engine(scope),
            exclude_dropped,
        };
        let chunks = blocking(self.config.clone(), "list chunks", move |config| {
            tinymemory_core::store::chunks::list_chunks(config, &engine_query)
        })
        .await?;
        Self::cross(&chunks, "convert chunks")
    }

    async fn get_chunk(&self, chunk_id: &str) -> Result<Option<Chunk>, MemoryError> {
        let id = chunk_id.to_string();
        let chunk = blocking(self.config.clone(), "get chunk", move |config| {
            tinymemory_core::store::chunks::get_chunk(config, &id)
        })
        .await?;
        match chunk {
            Some(chunk) => Ok(Some(Self::cross(&chunk, "convert chunk")?)),
            None => Ok(None),
        }
    }

    async fn chunk_detail(&self, chunk_id: &str) -> Result<Option<ChunkDetail>, MemoryError> {
        let id = chunk_id.to_string();
        let detail = blocking(self.config.clone(), "chunk detail", move |config| {
            let Some(chunk) = tinymemory_core::store::chunks::get_chunk(config, &id)? else {
                return Ok(None);
            };
            // The vault read is best-effort: a missing body is reported as
            // `None` so the caller can fall back to the row's own content,
            // rather than failing the whole detail view over a preview.
            let body = tinymemory_core::store::content::read::read_chunk_body(config, &id).ok();
            let has_embedding =
                tinymemory_core::store::chunks::get_chunk_embedding(config, &id)?.is_some();
            let lifecycle_status =
                tinymemory_core::store::chunks::get_chunk_lifecycle_status(config, &id)?;
            let content_path = tinymemory_core::store::chunks::get_chunk_content_path(config, &id)?;
            Ok(Some((
                chunk,
                body,
                has_embedding,
                lifecycle_status,
                content_path,
            )))
        })
        .await?;

        let Some((chunk, body, has_embedding, lifecycle_status, content_path)) = detail else {
            return Ok(None);
        };
        Ok(Some(ChunkDetail {
            chunk: Self::cross(&chunk, "convert chunk")?,
            body,
            content_path,
            lifecycle_status,
            has_embedding,
        }))
    }

    async fn storage_kinds(&self) -> Result<Vec<String>, MemoryError> {
        Ok(tinymemory_core::store::MemoryKind::ALL
            .iter()
            .map(|kind| kind.as_str().to_string())
            .collect())
    }

    async fn chunk_embeddings(
        &self,
        chunk_ids: &[String],
        model_signature: &str,
    ) -> Result<Vec<ChunkEmbedding>, MemoryError> {
        let ids = chunk_ids.to_vec();
        let signature = model_signature.to_string();
        let vectors = blocking(
            self.config.clone(),
            "load chunk embeddings",
            move |config| {
                tinymemory_core::store::chunks::get_chunk_embeddings_for_signature_batch(
                    config, &ids, &signature,
                )
            },
        )
        .await?;
        // Sorted so the response is deterministic: the engine returns a
        // `HashMap`, whose iteration order varies per process and would make an
        // otherwise-identical call return a differently-ordered list.
        let mut embeddings: Vec<ChunkEmbedding> = vectors
            .into_iter()
            .map(|(chunk_id, vector)| ChunkEmbedding { chunk_id, vector })
            .collect();
        embeddings.sort_by(|a, b| a.chunk_id.cmp(&b.chunk_id));
        Ok(embeddings)
    }
}

#[async_trait]
impl MemoryRetrieval for ModuleMemoryProvider {
    async fn fast_retrieve(
        &self,
        query: &str,
        options: FastRetrieveQuery,
        scope: Option<&SourceScope>,
    ) -> Result<RetrievalResponse, MemoryError> {
        if query.trim().is_empty() {
            return Err(MemoryError::Invalid("query must not be empty".to_string()));
        }
        let engine_options = tinymemory_core::tree::retrieval::FastRetrieveOptions {
            limit: options.limit,
            max_hops: options.max_hops,
            time_window_days: options.time_window_days,
        };
        let response = tinymemory_core::tree::retrieval::fast_retrieve_scoped(
            &self.config,
            query,
            engine_options,
            scope_to_engine(scope),
        )
        .await
        .map_err(|error| Self::other("fast retrieve", error))?;
        Self::cross(&response, "convert retrieval response")
    }

    async fn cover_window(
        &self,
        window: &CoverWindowQuery,
        scope: Option<&SourceScope>,
    ) -> Result<RetrievalResponse, MemoryError> {
        let CoverWindowQuery {
            since_ms,
            until_ms,
            source_id,
            source_kind,
            limit,
        } = window.clone();
        let engine_kind = source_kind
            .map(|kind| Self::cross(&kind, "convert source kind"))
            .transpose()?;
        let response = tinymemory_core::tree::retrieval::cover_window_scoped(
            &self.config,
            since_ms,
            until_ms,
            source_id.as_deref(),
            engine_kind,
            // 0 is the engine's "no caller preference" sentinel, not a request
            // for zero rows: `cover_window_scoped` substitutes its own
            // DEFAULT_LIMIT for it. Mapping `None` to 0 therefore asks for the
            // default, which is what an absent limit means.
            limit.unwrap_or(0),
            scope_to_engine(scope),
        )
        .await
        .map_err(|error| Self::other("cover window", error))?;
        Self::cross(&response, "convert retrieval response")
    }

    async fn retrieve_source(
        &self,
        query: &SourceRetrievalQuery,
        scope: Option<&SourceScope>,
    ) -> Result<RetrievalResponse, MemoryError> {
        let SourceRetrievalQuery {
            source_id,
            source_kind,
            time_window_days,
            query: text,
            limit,
        } = query.clone();
        let engine_kind = source_kind
            .map(|kind| Self::cross(&kind, "convert source kind"))
            .transpose()?;
        let response = tinymemory_core::tree::retrieval::source::query_source_scoped(
            &self.config,
            source_id.as_deref(),
            engine_kind,
            time_window_days,
            text.as_deref(),
            limit,
            scope_to_engine(scope),
        )
        .await
        .map_err(|error| Self::other("retrieve source", error))?;
        Self::cross(&response, "convert retrieval response")
    }

    async fn retrieve_children(
        &self,
        node_id: &str,
        max_depth: u32,
        query: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<RetrievalHit>, MemoryError> {
        let hits = tinymemory_core::tree::retrieval::drill_down::drill_down(
            &self.config,
            node_id,
            max_depth,
            query,
            limit,
        )
        .await
        .map_err(|error| Self::other("drill down", error))?;
        Self::cross(&hits, "convert retrieval hits")
    }

    async fn retrieve_leaves(
        &self,
        chunk_ids: &[String],
    ) -> Result<Vec<RetrievalHit>, MemoryError> {
        let hits = tinymemory_core::tree::retrieval::fetch::fetch_leaves(&self.config, chunk_ids)
            .await
            .map_err(|error| Self::other("fetch leaves", error))?;
        Self::cross(&hits, "convert retrieval hits")
    }

    async fn recall_namespace_scored(
        &self,
        namespace: &str,
        query: &str,
        limit: usize,
        exclude_session_id: Option<&str>,
    ) -> Result<Vec<NamespaceMemoryHit>, MemoryError> {
        let hits = self
            .client
            .unified_handle()
            .query_namespace_hits_excluding_session(
                namespace,
                query,
                u32::try_from(limit).unwrap_or(u32::MAX),
                exclude_session_id,
            )
            .await
            .map_err(|error| Self::other("recall namespace scored", error))?;
        Self::cross(&hits, "convert namespace hits")
    }

    async fn search_entities(
        &self,
        query: &str,
        kinds: Option<&[String]>,
        limit: usize,
    ) -> Result<Vec<EntityMatch>, MemoryError> {
        // Request kinds are validated, unlike response kinds which pass through
        // as an open vocabulary. An unknown filter that silently matched nothing
        // would be indistinguishable from a genuine empty result.
        let engine_kinds = match kinds {
            Some(kinds) => Some(
                kinds
                    .iter()
                    .map(|kind| {
                        tinymemory_core::tree::score::extract::EntityKind::parse(kind).map_err(
                            |_| MemoryError::Invalid(format!("unknown entity kind: {kind}")),
                        )
                    })
                    .collect::<Result<Vec<_>, MemoryError>>()?,
            ),
            None => None,
        };
        let matches = tinymemory_core::tree::retrieval::search_entities(
            &self.config,
            query,
            engine_kinds,
            limit,
        )
        .await
        .map_err(|error| Self::other("search entities", error))?;
        Self::cross(&matches, "convert entity matches")
    }
}

// ── Profile ──────────────────────────────────────────────────────────────────
//
// `ProfileStore`'s methods are synchronous and hold a `parking_lot::Mutex`
// across a SQLite call, so each one goes through `spawn_blocking` rather than
// being awaited on the runtime thread. The store is cheap to obtain — it is a
// handle over the client's connection, not an open — so it is fetched inside
// the blocking closure rather than held across an await.

fn facet_type_to_engine(
    facet_type: FacetType,
) -> tinymemory_core::store::namespace_store::profile::FacetType {
    use tinymemory_core::store::namespace_store::profile::FacetType as Engine;
    match facet_type {
        FacetType::Preference => Engine::Preference,
        FacetType::Workflow => Engine::Workflow,
        FacetType::Role => Engine::Role,
        FacetType::Personality => Engine::Personality,
        FacetType::Context => Engine::Context,
    }
}

#[async_trait]
impl MemoryProfile for ModuleMemoryProvider {
    async fn list_active_facets(&self) -> Result<Vec<ProfileFacet>, MemoryError> {
        let client = Arc::clone(&self.client);
        let facets = tokio::task::spawn_blocking(move || client.profile_store().list_active())
            .await
            .map_err(|e| Self::other("join list_active_facets", e))?
            .map_err(|e| Self::other("list_active_facets", e))?;
        Self::cross(&facets, "convert facets")
    }

    async fn list_all_facets(&self) -> Result<Vec<ProfileFacet>, MemoryError> {
        let client = Arc::clone(&self.client);
        let facets = tokio::task::spawn_blocking(move || client.profile_store().list_all())
            .await
            .map_err(|e| Self::other("join list_all_facets", e))?
            .map_err(|e| Self::other("list_all_facets", e))?;
        Self::cross(&facets, "convert facets")
    }

    async fn get_facet(&self, key: &str) -> Result<Option<ProfileFacet>, MemoryError> {
        let client = Arc::clone(&self.client);
        let key = key.to_string();
        let facet = tokio::task::spawn_blocking(move || client.profile_store().get(&key))
            .await
            .map_err(|e| Self::other("join get_facet", e))?
            .map_err(|e| Self::other("get_facet", e))?;
        match facet {
            Some(facet) => Ok(Some(Self::cross(&facet, "convert facet")?)),
            None => Ok(None),
        }
    }

    async fn facets_by_type(
        &self,
        facet_type: FacetType,
    ) -> Result<Vec<ProfileFacet>, MemoryError> {
        let client = Arc::clone(&self.client);
        let engine = facet_type_to_engine(facet_type);
        let facets =
            tokio::task::spawn_blocking(move || client.profile_store().facets_by_type(&engine))
                .await
                .map_err(|e| Self::other("join facets_by_type", e))?
                .map_err(|e| Self::other("facets_by_type", e))?;
        Self::cross(&facets, "convert facets")
    }

    async fn upsert_facet(&self, facet: &ProfileFacet) -> Result<(), MemoryError> {
        let client = Arc::clone(&self.client);
        let engine: tinymemory_core::store::namespace_store::profile::ProfileFacet =
            Self::cross(facet, "convert facet")?;
        tokio::task::spawn_blocking(move || client.profile_store().upsert_full(&engine))
            .await
            .map_err(|e| Self::other("join upsert_facet", e))?
            .map_err(|e| Self::other("upsert_facet", e))
    }

    async fn upsert_provider_facet(
        &self,
        facet_id: &str,
        facet_type: FacetType,
        key: &str,
        value: &str,
        confidence: f64,
        segment_id: Option<&str>,
        observed_at: f64,
    ) -> Result<(), MemoryError> {
        let client = Arc::clone(&self.client);
        let engine = facet_type_to_engine(facet_type);
        let (facet_id, key, value) = (facet_id.to_string(), key.to_string(), value.to_string());
        let segment_id = segment_id.map(str::to_string);
        tokio::task::spawn_blocking(move || {
            client.profile_store().upsert_provider_facet(
                &facet_id,
                &engine,
                &key,
                &value,
                confidence,
                segment_id.as_deref(),
                observed_at,
            )
        })
        .await
        .map_err(|e| Self::other("join upsert_provider_facet", e))?
        .map_err(|e| Self::other("upsert_provider_facet", e))
    }

    async fn set_facet_user_state(
        &self,
        key: &str,
        user_state: UserState,
    ) -> Result<bool, MemoryError> {
        use tinymemory_core::store::namespace_store::profile::UserState as Engine;
        let client = Arc::clone(&self.client);
        let key = key.to_string();
        let engine = match user_state {
            UserState::Auto => Engine::Auto,
            UserState::Pinned => Engine::Pinned,
            UserState::Forgotten => Engine::Forgotten,
        };
        tokio::task::spawn_blocking(move || client.profile_store().set_user_state(&key, engine))
            .await
            .map_err(|e| Self::other("join set_facet_user_state", e))?
            .map_err(|e| Self::other("set_facet_user_state", e))
    }

    async fn delete_facet(&self, key: &str) -> Result<bool, MemoryError> {
        let client = Arc::clone(&self.client);
        let key = key.to_string();
        tokio::task::spawn_blocking(move || client.profile_store().delete(&key))
            .await
            .map_err(|e| Self::other("join delete_facet", e))?
            .map_err(|e| Self::other("delete_facet", e))
    }

    async fn delete_facet_by_id(&self, facet_id: &str) -> Result<bool, MemoryError> {
        let client = Arc::clone(&self.client);
        let facet_id = facet_id.to_string();
        tokio::task::spawn_blocking(move || client.profile_store().delete_by_facet_id(&facet_id))
            .await
            .map_err(|e| Self::other("join delete_facet_by_id", e))?
            .map_err(|e| Self::other("delete_facet_by_id", e))
    }

    async fn drop_facets_below(&self, threshold: f64) -> Result<usize, MemoryError> {
        let client = Arc::clone(&self.client);
        tokio::task::spawn_blocking(move || client.profile_store().drop_below_threshold(threshold))
            .await
            .map_err(|e| Self::other("join drop_facets_below", e))?
            .map_err(|e| Self::other("drop_facets_below", e))
    }

    async fn workflow_identity_matches(&self, key_pattern: &str, canonical_value: &str) -> bool {
        let client = Arc::clone(&self.client);
        let (pattern, value) = (key_pattern.to_string(), canonical_value.to_string());
        tokio::task::spawn_blocking(move || {
            client
                .profile_store()
                .skill_identity_matches(&pattern, &value)
        })
        .await
        // A join failure reads as "no", like every other error on this
        // predicate — see the trait docs.
        .unwrap_or(false)
    }
}

/// Episodic capture: the turn-by-turn record and its segment lifecycle.
///
/// Every method hops to `spawn_blocking` for the same reason the profile family
/// does — these are synchronous `rusqlite` calls behind a `parking_lot::Mutex`,
/// and blocking a tinybus executor thread on a database lock would stall every
/// other call the module is serving.
///
/// The boundary-detection and summary-composition halves of the archivist are
/// **not** here: they touch no database and are host policy. See the family's
/// contract docs.
#[async_trait]
impl MemoryEpisodic for ModuleMemoryProvider {
    async fn insert_turn(&self, turn: &EpisodicTurn) -> Result<i64, MemoryError> {
        let conn = self.client.profile_conn();
        let entry = tinymemory_core::store::fts5::EpisodicEntry {
            id: None,
            session_id: turn.session_id.clone(),
            timestamp: turn.timestamp,
            role: turn.role.clone(),
            content: turn.content.clone(),
            lesson: turn.lesson.clone(),
            tool_calls_json: turn.tool_calls_json.clone(),
            // The contract carries this signed because a cost is a plain number
            // on the wire; the engine column is unsigned. A negative value is
            // not meaningful, so it clamps rather than wrapping.
            cost_microdollars: u64::try_from(turn.cost_microdollars).unwrap_or(0),
        };
        tokio::task::spawn_blocking(move || {
            tinymemory_core::store::fts5::episodic_insert(&conn, &entry)
        })
        .await
        .map_err(|e| Self::other("join insert_turn", e))?
        .map_err(|e| Self::other("insert_turn", e))
    }

    async fn session_turns(&self, session_id: &str) -> Result<Vec<EpisodicTurn>, MemoryError> {
        let conn = self.client.profile_conn();
        let session_id = session_id.to_string();
        let entries = tokio::task::spawn_blocking(move || {
            tinymemory_core::store::fts5::episodic_session_entries(&conn, &session_id)
        })
        .await
        .map_err(|e| Self::other("join session_turns", e))?
        .map_err(|e| Self::other("session_turns", e))?;
        Ok(entries.into_iter().map(episodic_to_contract).collect())
    }

    async fn open_segment(
        &self,
        session_id: &str,
    ) -> Result<Option<ConversationSegment>, MemoryError> {
        let conn = self.client.profile_conn();
        let session_id = session_id.to_string();
        let segment = tokio::task::spawn_blocking(move || {
            tinymemory_core::store::segments::open_segment_for_session(&conn, &session_id)
        })
        .await
        .map_err(|e| Self::other("join open_segment", e))?
        .map_err(|e| Self::other("open_segment", e))?;
        Ok(segment.map(segment_to_contract))
    }

    async fn create_segment(
        &self,
        segment_id: &str,
        session_id: &str,
        namespace: &str,
        start_episodic_id: i64,
        start_timestamp: f64,
        now: f64,
    ) -> Result<(), MemoryError> {
        let conn = self.client.profile_conn();
        let (segment_id, session_id, namespace) = (
            segment_id.to_string(),
            session_id.to_string(),
            namespace.to_string(),
        );
        tokio::task::spawn_blocking(move || {
            tinymemory_core::store::segments::segment_create(
                &conn,
                &segment_id,
                &session_id,
                &namespace,
                start_episodic_id,
                // Per-session seq numbering is the archivist store's, and it is
                // not part of this contract; legacy rows carry `None` too.
                None,
                start_timestamp,
                now,
            )
        })
        .await
        .map_err(|e| Self::other("join create_segment", e))?
        .map_err(|e| Self::other("create_segment", e))
    }

    async fn append_turn(
        &self,
        segment_id: &str,
        episodic_id: i64,
        timestamp: f64,
        now: f64,
    ) -> Result<(), MemoryError> {
        let conn = self.client.profile_conn();
        let segment_id = segment_id.to_string();
        tokio::task::spawn_blocking(move || {
            tinymemory_core::store::segments::segment_append_turn(
                &conn,
                &segment_id,
                episodic_id,
                None,
                timestamp,
                now,
            )
        })
        .await
        .map_err(|e| Self::other("join append_turn", e))?
        .map_err(|e| Self::other("append_turn", e))
    }

    async fn close_segment(&self, segment_id: &str, now: f64) -> Result<(), MemoryError> {
        let conn = self.client.profile_conn();
        let segment_id = segment_id.to_string();
        tokio::task::spawn_blocking(move || {
            tinymemory_core::store::segments::segment_close(&conn, &segment_id, now)
        })
        .await
        .map_err(|e| Self::other("join close_segment", e))?
        .map_err(|e| Self::other("close_segment", e))
    }

    async fn set_segment_summary(
        &self,
        segment_id: &str,
        summary: &str,
        now: f64,
    ) -> Result<(), MemoryError> {
        let conn = self.client.profile_conn();
        let (segment_id, summary) = (segment_id.to_string(), summary.to_string());
        tokio::task::spawn_blocking(move || {
            tinymemory_core::store::segments::segment_set_summary(&conn, &segment_id, &summary, now)
        })
        .await
        .map_err(|e| Self::other("join set_segment_summary", e))?
        .map_err(|e| Self::other("set_segment_summary", e))
    }

    async fn upsert_segment_embedding(
        &self,
        segment_id: &str,
        model_signature: &str,
        embedding: &[f32],
        created_at: f64,
    ) -> Result<(), MemoryError> {
        let conn = self.client.profile_conn();
        let (segment_id, model_signature) = (segment_id.to_string(), model_signature.to_string());
        let embedding = embedding.to_vec();
        tokio::task::spawn_blocking(move || {
            tinymemory_core::store::segments::segment_embedding_upsert(
                &conn,
                &segment_id,
                &model_signature,
                &embedding,
                created_at,
            )
        })
        .await
        .map_err(|e| Self::other("join upsert_segment_embedding", e))?
        .map_err(|e| Self::other("upsert_segment_embedding", e))
    }
}

/// Engine episodic row -> contract turn.
fn episodic_to_contract(entry: tinymemory_core::store::fts5::EpisodicEntry) -> EpisodicTurn {
    EpisodicTurn {
        id: entry.id,
        session_id: entry.session_id,
        timestamp: entry.timestamp,
        role: entry.role,
        content: entry.content,
        lesson: entry.lesson,
        tool_calls_json: entry.tool_calls_json,
        cost_microdollars: i64::try_from(entry.cost_microdollars).unwrap_or(i64::MAX),
    }
}

/// Engine segment row -> contract segment.
///
/// Written out rather than derived: the engine row carries several fields the
/// contract deliberately does not expose (`topic_keywords`, the seq numbers,
/// `created_at`), and a blanket conversion would quietly start shipping them if
/// the contract ever grew a matching name.
fn segment_to_contract(
    segment: tinymemory_core::store::segments::ConversationSegment,
) -> ConversationSegment {
    use tinymemory_core::store::segments::SegmentStatus;
    ConversationSegment {
        segment_id: segment.segment_id,
        session_id: segment.session_id,
        namespace: segment.namespace,
        start_episodic_id: segment.start_episodic_id,
        end_episodic_id: segment.end_episodic_id,
        start_timestamp: segment.start_timestamp,
        end_timestamp: segment.end_timestamp,
        turn_count: segment.turn_count,
        summary: segment.summary,
        embedding: segment.embedding,
        open: matches!(segment.status, SegmentStatus::Open),
    }
}
