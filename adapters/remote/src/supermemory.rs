//! Self-hosted Supermemory API adapter.

use async_trait::async_trait;
use reqwest::Method;
use serde_json::{json, Value};
use tinymemory_api::recall::RecallOpts;
use tinymemory_api::traits::Memory;
use tinymemory_api::types::MemoryTaint;

use crate::common::{category, stable_id, Dialect, HttpClient, RemoteMemory, StoredEntry};

/// Stable driver id used by configuration and status output.
pub use tinymemory_api::drivers::SUPERMEMORY_DRIVER_ID;

/// Default base URL for Supermemory's managed API.
pub const SUPERMEMORY_API_ENDPOINT: &str = "https://api.supermemory.ai";

/// A Supermemory managed or self-hosted service exposed through TinyMemory's contract.
#[derive(Debug)]
pub struct SupermemoryMemory {
    inner: RemoteMemory<SupermemoryDialect>,
}

impl SupermemoryMemory {
    /// Rebuilds the HTTP transport with a different per-request deadline
    /// (issue #18 follow-up U5). The default is 60s with a 10s connect
    /// deadline — right for interactive calls; a bulk migration or a tight
    /// liveness probe may want its own budget.
    ///
    /// # Errors
    ///
    /// Fails only if the underlying HTTP client cannot be rebuilt — a
    /// configuration-time failure, before any request is made.
    pub fn with_request_timeout(mut self, timeout: std::time::Duration) -> anyhow::Result<Self> {
        let client = self
            .inner
            .dialect_mut()
            .client
            .clone()
            .with_timeout(timeout)?;
        self.inner.dialect_mut().client = client;
        Ok(self)
    }

    /// Connect to a Supermemory server using its bearer API key.
    ///
    /// # Errors
    ///
    /// Returns an error when `endpoint` is not an HTTP(S) URL.
    pub fn new(endpoint: &str, api_key: Option<&str>) -> anyhow::Result<Self> {
        Ok(Self {
            inner: RemoteMemory::new(SupermemoryDialect {
                client: HttpClient::bearer(endpoint, api_key)?,
            }),
        })
    }

    /// Connect to a provided Supermemory API using bearer authentication.
    ///
    /// # Errors
    ///
    /// Returns an error when `endpoint` is invalid or `api_key` is blank.
    pub fn api(endpoint: &str, api_key: &str) -> anyhow::Result<Self> {
        anyhow::ensure!(
            !api_key.trim().is_empty(),
            "supermemory API key must not be empty"
        );
        Self::new(endpoint, Some(api_key))
    }

    /// Connect to a self-hosted Supermemory server.
    ///
    /// Self-hosted Supermemory generates a bearer API key on first boot and
    /// exposes the same API as the managed service.
    ///
    /// # Errors
    ///
    /// Returns an error when `endpoint` is invalid or `api_key` is blank.
    pub fn self_hosted(endpoint: &str, api_key: &str) -> anyhow::Result<Self> {
        Self::api(endpoint, api_key)
    }

    /// Connect to Supermemory's managed API endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error when `api_key` is blank.
    pub fn cloud(api_key: &str) -> anyhow::Result<Self> {
        Self::api(SUPERMEMORY_API_ENDPOINT, api_key)
    }
}

#[async_trait]
impl Memory for SupermemoryMemory {
    /// Returns the Supermemory driver identifier.
    fn name(&self) -> &str {
        self.inner.name()
    }
    /// Stores an internally sourced record through the shared contract.
    async fn store(
        &self,
        n: &str,
        k: &str,
        c: &str,
        cat: tinymemory_api::types::MemoryCategory,
        s: Option<&str>,
    ) -> anyhow::Result<()> {
        self.inner.store(n, k, c, cat, s).await
    }
    /// Stores a record while preserving its provenance taint.
    async fn store_with_taint(
        &self,
        n: &str,
        k: &str,
        c: &str,
        cat: tinymemory_api::types::MemoryCategory,
        s: Option<&str>,
        t: MemoryTaint,
    ) -> anyhow::Result<()> {
        self.inner.store_with_taint(n, k, c, cat, s, t).await
    }
    /// Runs native Supermemory search and applies TinyMemory filters.
    async fn recall(
        &self,
        q: &str,
        l: usize,
        o: RecallOpts<'_>,
    ) -> anyhow::Result<Vec<tinymemory_api::types::MemoryEntry>> {
        self.inner.recall(q, l, o).await
    }
    /// Fetches one exact namespace/key record.
    async fn get(
        &self,
        n: &str,
        k: &str,
    ) -> anyhow::Result<Option<tinymemory_api::types::MemoryEntry>> {
        self.inner.get(n, k).await
    }
    /// Lists records matching the supplied TinyMemory filters.
    async fn list(
        &self,
        n: Option<&str>,
        c: Option<&tinymemory_api::types::MemoryCategory>,
        s: Option<&str>,
    ) -> anyhow::Result<Vec<tinymemory_api::types::MemoryEntry>> {
        self.inner.list(n, c, s).await
    }
    /// Deletes one exact namespace/key record.
    async fn forget(&self, n: &str, k: &str) -> anyhow::Result<bool> {
        self.inner.forget(n, k).await
    }
    /// Summarizes every namespace visible through this adapter.
    async fn namespace_summaries(
        &self,
    ) -> anyhow::Result<Vec<tinymemory_api::types::NamespaceSummary>> {
        self.inner.namespace_summaries().await
    }
    /// Counts every record visible through this adapter.
    async fn count(&self) -> anyhow::Result<usize> {
        self.inner.count().await
    }
    /// Checks whether the configured Supermemory service is reachable.
    async fn health_check(&self) -> bool {
        self.inner.health_check().await
    }
}

#[derive(Debug)]
/// Supermemory-specific REST operations and wire-format conversion.
struct SupermemoryDialect {
    client: HttpClient,
}

impl SupermemoryDialect {
    /// Maps an arbitrary TinyMemory namespace into Supermemory's bounded tag grammar.
    fn container_tag(namespace: &str) -> String {
        format!("tinymemory:{}", stable_id("container", namespace))
    }

    /// Encodes TinyMemory identity, classification, session, and provenance.
    fn metadata(entry: &StoredEntry) -> Value {
        let mut metadata = serde_json::Map::from_iter([
            ("tinymemory_namespace".into(), json!(entry.namespace)),
            ("tinymemory_key".into(), json!(entry.key)),
            (
                "tinymemory_category".into(),
                json!(entry.category.to_string()),
            ),
            ("tinymemory_taint".into(), json!(entry.taint.as_db_str())),
        ]);
        if let Some(session) = &entry.session_id {
            metadata.insert("tinymemory_session_id".into(), json!(session));
        }
        Value::Object(metadata)
    }

    /// Decodes a Supermemory result containing TinyMemory-owned metadata.
    fn decode(value: &Value) -> Option<StoredEntry> {
        let metadata = value.get("metadata")?.as_object()?;
        Some(StoredEntry {
            remote_id: value.get("id")?.as_str()?.to_owned(),
            namespace: metadata.get("tinymemory_namespace")?.as_str()?.to_owned(),
            key: metadata.get("tinymemory_key")?.as_str()?.to_owned(),
            content: value
                .get("content")
                .or_else(|| value.get("memory"))
                .or_else(|| value.get("chunk"))?
                .as_str()?
                .to_owned(),
            category: category(metadata.get("tinymemory_category").and_then(Value::as_str)),
            timestamp: value
                .get("updatedAt")
                .or_else(|| value.get("createdAt"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            session_id: metadata
                .get("tinymemory_session_id")
                .and_then(Value::as_str)
                .map(str::to_owned),
            score: value.get("similarity").and_then(Value::as_f64),
            taint: metadata
                .get("tinymemory_taint")
                .and_then(Value::as_str)
                .map(MemoryTaint::from_db_str)
                .unwrap_or_default(),
        })
    }

    /// Enumerates memories separately for each discovered container tag.
    async fn memories(&self) -> anyhow::Result<Vec<StoredEntry>> {
        let tags: Value = self
            .client
            .json(Method::GET, "v3/container-tags/list", None)
            .await?;
        let container_tags = tags
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|value| value.get("containerTag").and_then(Value::as_str))
            .collect::<Vec<_>>();
        if container_tags.is_empty() {
            return Ok(Vec::new());
        }
        let mut entries = Vec::new();
        for container_tag in container_tags {
            entries.extend(self.memories_in_tag(container_tag).await?);
        }
        Ok(entries)
    }

    /// Enumerates the live memories of one container tag.
    ///
    /// Split out so the keyed paths (`upsert`, `delete`) can page the single
    /// tag their namespace maps to instead of every tag the account holds —
    /// before this, each store of one record enumerated the entire account
    /// over HTTP.
    async fn memories_in_tag(&self, container_tag: &str) -> anyhow::Result<Vec<StoredEntry>> {
        let mut entries = Vec::new();
        {
            let mut page = 1_u64;
            loop {
                let response: Value = self
                    .client
                    .json(
                        Method::POST,
                        "v4/memories/list",
                        Some(&json!({
                            "limit": 200,
                            "page": page,
                            "sort": "createdAt",
                            "order": "desc",
                            "containerTags": [container_tag]
                        })),
                    )
                    .await?;
                let memories = response
                    .get("memoryEntries")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for memory in memories {
                    let is_latest = memory
                        .get("isLatest")
                        .and_then(Value::as_bool)
                        .unwrap_or(true);
                    let is_forgotten = memory
                        .get("isForgotten")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    if is_latest && !is_forgotten {
                        let Some(entry) = Self::decode(&memory) else {
                            continue;
                        };
                        entries.push(entry);
                    }
                }
                let total_pages = response
                    .pointer("/pagination/totalPages")
                    .and_then(Value::as_u64)
                    .unwrap_or(1);
                if page >= total_pages {
                    break;
                }
                page += 1;
            }
        }
        Ok(entries)
    }

    /// The live entry stored under `(namespace, key)`, if any — paging only
    /// that namespace's container tag.
    async fn find_entry(&self, namespace: &str, key: &str) -> anyhow::Result<Option<StoredEntry>> {
        Ok(self
            .memories_in_tag(&Self::container_tag(namespace))
            .await?
            .into_iter()
            .find(|item| item.namespace == namespace && item.key == key))
    }
}

#[async_trait]
impl Dialect for SupermemoryDialect {
    /// Returns the stable Supermemory driver identifier.
    fn name(&self) -> &'static str {
        SUPERMEMORY_DRIVER_ID
    }

    /// Replaces an existing exact record or creates a direct v4 memory.
    async fn upsert(&self, entry: StoredEntry) -> anyhow::Result<()> {
        let existing = self.find_entry(&entry.namespace, &entry.key).await?;
        let metadata = Self::metadata(&entry);
        if let Some(existing) = existing {
            self.client
                .empty(
                    Method::PATCH,
                    "v4/memories",
                    Some(&json!({
                        "id": existing.remote_id,
                        "newContent": entry.content,
                        "metadata": metadata
                    })),
                )
                .await?;
        } else {
            self.client
                .empty(
                    Method::POST,
                    "v4/memories",
                    Some(&json!({
                        "memories": [{
                            "content": entry.content,
                            "isStatic": false,
                            "metadata": metadata
                        }],
                        "containerTag": Self::container_tag(&entry.namespace),
                    })),
                )
                .await?;
        }
        Ok(())
    }

    /// Enumerates TinyMemory-owned Supermemory records.
    async fn entries(&self) -> anyhow::Result<Vec<StoredEntry>> {
        self.memories().await
    }

    /// Executes Supermemory's native v4 search.
    async fn search(
        &self,
        query: &str,
        limit: usize,
        opts: RecallOpts<'_>,
    ) -> anyhow::Result<Vec<StoredEntry>> {
        let mut body = json!({
            "q": query,
            "searchMode": "memories",
            "limit": limit
        });
        if let Some(object) = body.as_object_mut() {
            if let Some(namespace) = opts.namespace {
                object.insert("containerTag".into(), json!(Self::container_tag(namespace)));
            }
            if let Some(minimum) = opts.min_score {
                object.insert("threshold".into(), json!(minimum));
            }
        }
        let response: Value = self
            .client
            .json(Method::POST, "v4/search", Some(&body))
            .await?;
        Ok(response
            .get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Self::decode)
            .collect())
    }

    /// Finds and deletes an exact TinyMemory logical record.
    async fn delete(&self, namespace: &str, key: &str) -> anyhow::Result<bool> {
        let Some(entry) = self.find_entry(namespace, key).await? else {
            return Ok(false);
        };
        self.client
            .empty(
                Method::DELETE,
                "v4/memories",
                Some(&json!({
                    "id": entry.remote_id,
                    "containerTag": Self::container_tag(namespace),
                    "reason": "deleted through TinyMemory"
                })),
            )
            .await?;
        Ok(true)
    }

    /// Probes `v3/container-tags/list` — the cheapest endpoint that proves
    /// BOTH the credential and the data plane (issue #18 §U4). The old
    /// root-page probe answered 200 to an unauthenticated client against a
    /// live server, so a wrong key looked healthy until the first real call.
    async fn health(&self) -> anyhow::Result<()> {
        // Status-only: a 200 from this authenticated route is the proof; the
        // body's shape is the data path's concern, not the probe's.
        self.client.probe("v3/container-tags/list").await
    }
}

#[cfg(test)]
#[path = "supermemory_test.rs"]
mod test;
