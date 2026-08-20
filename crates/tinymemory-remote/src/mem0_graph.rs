//! [`Mem0Graph`] — a client-side, heuristic [`MemoryGraph`] over Mem0.
//!
//! Mem0's self-hosted OSS package dropped Graph Memory in its 2.x line: its
//! `graph_store`/`GraphStoreFactory` (Neo4j-backed) only exist in the 1.0.x
//! line, and 1.0.x's graph feature moved to Mem0's *hosted* platform product
//! (the `docs.mem0.ai/platform/...` docs describe that product, not this
//! self-hosted server). Downgrading the pinned server's `mem0ai` dependency
//! two major versions to get it back was tried and works mechanically, but is
//! a real version-compatibility risk for a shared test harness, so this stays
//! on the 2.x line the server actually ships.
//!
//! Instead of a native graph, this derives one: it lists every entry Mem0
//! already stores for a namespace and runs a **plain co-occurrence
//! heuristic** over each entry's content — group runs of capitalized words
//! per sentence as entity candidates, and link consecutive candidates within
//! the same sentence with predicate `co_occurs_with`. This is intentionally
//! not semantic relation extraction (no LLM call, no NER model): it is real
//! computation over real stored content, cheap and deterministic, but it will
//! both miss real relations and surface spurious ones from capitalized
//! non-entities (sentence-initial words, headers). `attrs.sentence` carries
//! the exact source sentence so a caller can judge each edge for itself.
//!
//! `kv_*` and `put_relation` have no Mem0 (or heuristic) counterpart and
//! return [`MemoryError::Other`] rather than faking one.

use std::sync::Arc;

use anyhow::anyhow;
use async_trait::async_trait;
use tinymemory_api::error::MemoryError;
use tinymemory_api::provider::MemoryGraph;
use tinymemory_api::traits::Memory;
use tinymemory_api::types::{GraphRelationRecord, MemoryKvRecord};

const NO_KV_STORE: &str = "mem0 has no generic key/value store to read or write";
const NO_WRITABLE_GRAPH: &str =
    "this graph is inferred client-side from stored content and cannot be edited directly";

/// A heuristic, co-occurrence-based [`MemoryGraph`] derived from whatever a
/// wrapped [`Memory`] backend (Mem0) already stores.
pub struct Mem0Graph {
    memory: Arc<dyn Memory>,
}

impl std::fmt::Debug for Mem0Graph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `dyn Memory` is not `Debug`; there is nothing else safe to render.
        f.debug_struct("Mem0Graph").finish_non_exhaustive()
    }
}

impl Mem0Graph {
    /// Derive relations from `memory`'s stored entries.
    #[must_use]
    pub fn new(memory: Arc<dyn Memory>) -> Self {
        Self { memory }
    }
}

/// A run of consecutive capitalized words, e.g. `"Ilya Bamon"`.
fn is_capitalized_word(word: &str) -> bool {
    let trimmed = word.trim_matches(|c: char| !c.is_alphanumeric());
    let mut chars = trimmed.chars();
    matches!(chars.next(), Some(c) if c.is_uppercase())
        && trimmed.chars().skip(1).all(char::is_alphanumeric)
}

/// Extracts entity-candidate runs (consecutive capitalized words) from one
/// sentence, in first-occurrence order, deduplicated.
fn entity_candidates(sentence: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for word in sentence.split_whitespace() {
        if is_capitalized_word(word) {
            current.push(word.trim_matches(|c: char| !c.is_alphanumeric()));
        } else if !current.is_empty() {
            candidates.push(current.join(" "));
            current.clear();
        }
    }
    if !current.is_empty() {
        candidates.push(current.join(" "));
    }
    candidates.retain(|c| c.len() > 2);
    candidates.dedup();
    candidates
}

/// Splits `content` into relation triples via the co-occurrence heuristic —
/// see the module docs for exactly what this does and does not claim.
fn infer_relations(entry_id: &str, namespace: &str, content: &str) -> Vec<GraphRelationRecord> {
    content
        .split(['.', '!', '?', '\n'])
        .flat_map(|sentence| {
            let entities = entity_candidates(sentence);
            let sentence = sentence.trim().to_string();
            entities
                .windows(2)
                .map(|pair| GraphRelationRecord {
                    namespace: Some(namespace.to_string()),
                    subject: pair[0].clone(),
                    predicate: "co_occurs_with".to_string(),
                    object: pair[1].clone(),
                    attrs: serde_json::json!({ "sentence": sentence, "source": "heuristic" }),
                    updated_at: 0.0,
                    evidence_count: 1,
                    order_index: None,
                    document_ids: vec![entry_id.to_string()],
                    chunk_ids: Vec::new(),
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

#[async_trait]
impl MemoryGraph for Mem0Graph {
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

    /// Lists the namespace's entries and infers relations from their content
    /// via the co-occurrence heuristic described in the module docs.
    async fn relations(
        &self,
        namespace: Option<&str>,
        subject: Option<&str>,
        predicate: Option<&str>,
        limit: usize,
    ) -> Result<Vec<GraphRelationRecord>, MemoryError> {
        let entries = self
            .memory
            .list(namespace, None, None)
            .await
            .map_err(MemoryError::Other)?;
        let relations = entries
            .iter()
            .flat_map(|entry| {
                infer_relations(
                    &entry.id,
                    entry.namespace.as_deref().unwrap_or_default(),
                    &entry.content,
                )
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
