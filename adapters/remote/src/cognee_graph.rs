//! [`CogneeGraph`] — a read-only [`MemoryGraph`] over Cognee's derived
//! knowledge graph.
//!
//! Cognee's graph is **built by its `cognify` pipeline** over ingested
//! documents, not a generic key/value store with hand-editable relations:
//! there is no endpoint to write an arbitrary KV record, and no endpoint to
//! insert a graph edge directly. So this implements exactly the one method
//! that has a genuine Cognee counterpart —
//! `relations`, backed by `GET /api/v1/datasets/{dataset_id}/graph` — and
//! returns [`MemoryError::Other`] for every method that has none (`kv_get`,
//! `kv_put`, `kv_delete`, `kv_list`, `put_relation`), rather than faking
//! empty success.

use anyhow::anyhow;
use async_trait::async_trait;
use reqwest::Method;
use serde_json::Value;
use tinymemory_api::error::MemoryError;
use tinymemory_api::provider::MemoryGraph;
use tinymemory_api::types::{GraphRelationRecord, MemoryKvRecord};

use crate::common::{stable_id, Attempts, HttpClient};

/// Read-only relation queries over one Cognee dataset's knowledge graph.
#[derive(Debug)]
pub struct CogneeGraph {
    client: HttpClient,
}

impl CogneeGraph {
    /// Connect to the same self-hosted Cognee server a [`crate::CogneeMemory`]
    /// targets (`::new`/`::self_hosted`).
    ///
    /// # Errors
    ///
    /// Returns an error when `endpoint` is not an HTTP(S) URL.
    pub fn new(endpoint: &str, access_token: Option<&str>) -> anyhow::Result<Self> {
        Ok(Self {
            client: HttpClient::bearer(endpoint, access_token)?,
        })
    }

    /// Connect to a Cognee Cloud tenant using `X-Api-Key` authentication.
    ///
    /// # Errors
    ///
    /// Returns an error when `endpoint` is invalid or `api_key` is blank.
    pub fn api(endpoint: &str, api_key: &str) -> anyhow::Result<Self> {
        anyhow::ensure!(
            !api_key.trim().is_empty(),
            "cognee API key must not be empty"
        );
        Ok(Self {
            client: HttpClient::api_key(endpoint, Some(api_key))?,
        })
    }

    /// Matches [`crate::cognee`]'s private `CogneeDialect::dataset_name`
    /// exactly, so both halves resolve one TinyMemory namespace to the same
    /// Cognee dataset.
    fn dataset_name(namespace: &str) -> String {
        format!("tinymemory__{}", stable_id("dataset", namespace))
    }

    async fn find_dataset_id(&self, namespace: &str) -> anyhow::Result<Option<String>> {
        let name = Self::dataset_name(namespace);
        let response: Value = self
            .client
            .json(
                Method::GET,
                "api/v1/datasets/",
                None,
                Attempts::RetryTransient,
            )
            .await?;
        Ok(response
            .as_array()
            .into_iter()
            .flatten()
            .find(|value| value.get("name").and_then(Value::as_str) == Some(name.as_str()))
            .and_then(|value| value.get("id").and_then(Value::as_str))
            .map(str::to_owned))
    }
}

const NO_KV_STORE: &str = "cognee has no generic key/value store to read or write";
const NO_WRITABLE_GRAPH: &str =
    "cognee's graph is derived by the cognify pipeline over ingested documents and cannot be edited directly";

#[async_trait]
impl MemoryGraph for CogneeGraph {
    async fn kv_get(
        &self,
        _namespace: Option<&str>,
        _key: &str,
    ) -> Result<Option<MemoryKvRecord>, MemoryError> {
        Err(MemoryError::Other(anyhow!(NO_KV_STORE)))
    }

    async fn kv_put(
        &self,
        _namespace: Option<&str>,
        _key: &str,
        _value: serde_json::Value,
    ) -> Result<(), MemoryError> {
        Err(MemoryError::Other(anyhow!(NO_KV_STORE)))
    }

    async fn kv_delete(&self, _namespace: Option<&str>, _key: &str) -> Result<bool, MemoryError> {
        Err(MemoryError::Other(anyhow!(NO_KV_STORE)))
    }

    async fn kv_list(
        &self,
        _namespace: Option<&str>,
        _prefix: Option<&str>,
        _limit: usize,
    ) -> Result<Vec<MemoryKvRecord>, MemoryError> {
        Err(MemoryError::Other(anyhow!(NO_KV_STORE)))
    }

    /// Reads the dataset's derived graph and reshapes it into
    /// `(subject, predicate, object)` triples.
    ///
    /// Cognee's graph endpoint takes only a dataset id, not a subject or
    /// predicate filter, so this fetches the whole dataset graph and filters
    /// client-side. `namespace: None` ("the global, namespace-less slice") has
    /// no Cognee counterpart — every dataset is namespace-scoped — so it is
    /// rejected as invalid input rather than silently returning nothing.
    async fn relations(
        &self,
        namespace: Option<&str>,
        subject: Option<&str>,
        predicate: Option<&str>,
        limit: usize,
    ) -> Result<Vec<GraphRelationRecord>, MemoryError> {
        let namespace = namespace.ok_or_else(|| {
            MemoryError::Invalid(
                "cognee requires a namespace to resolve a dataset graph".to_string(),
            )
        })?;
        let Some(dataset_id) = self.find_dataset_id(namespace).await? else {
            return Ok(Vec::new());
        };
        let graph: Value = self
            .client
            .json(
                Method::GET,
                &format!("api/v1/datasets/{dataset_id}/graph"),
                None,
                Attempts::RetryTransient,
            )
            .await?;
        let nodes = graph.get("nodes").and_then(Value::as_array);
        let labels: std::collections::HashMap<&str, &str> = nodes
            .into_iter()
            .flatten()
            .filter_map(|node| {
                Some((
                    node.get("id")?.as_str()?,
                    node.get("label")?.as_str().unwrap_or_default(),
                ))
            })
            .collect();

        let edges = graph.get("edges").and_then(Value::as_array);
        let relations = edges
            .into_iter()
            .flatten()
            .filter_map(|edge| {
                let source = edge.get("source")?.as_str()?;
                let target = edge.get("target")?.as_str()?;
                let label = edge.get("label")?.as_str().unwrap_or_default();
                Some(GraphRelationRecord {
                    namespace: Some(namespace.to_string()),
                    subject: labels.get(source).copied().unwrap_or(source).to_string(),
                    predicate: label.to_string(),
                    object: labels.get(target).copied().unwrap_or(target).to_string(),
                    attrs: Value::Null,
                    updated_at: 0.0,
                    evidence_count: 1,
                    order_index: None,
                    document_ids: Vec::new(),
                    chunk_ids: Vec::new(),
                })
            })
            .filter(|relation| subject.is_none_or(|s| relation.subject == s))
            .filter(|relation| predicate.is_none_or(|p| relation.predicate == p))
            .take(limit)
            .collect();
        Ok(relations)
    }

    async fn put_relation(&self, _relation: GraphRelationRecord) -> Result<(), MemoryError> {
        Err(MemoryError::Other(anyhow!(NO_WRITABLE_GRAPH)))
    }
}
