//! Unit tests for the section surface.
//!
//! Two doubles, for two different jobs. [`ScoredMemory`] is a `Memory` backend
//! whose entries carry scores the test chose, wrapped through
//! [`MemoryTraitProvider`] — needed because nothing in the contract lets a
//! caller *store* a score, and the fan-out's whole job is ranking by one.
//! `NullMemoryProvider` covers the other end: a driver that retains nothing,
//! where every call must still succeed.

// A failing assertion in a test *is* a panic; the crate-wide `unwrap_used` /
// `expect_used` / `panic` lints exist to keep the library from panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tinymemory_api::error::MemoryError;
use tinymemory_api::namespace::MemorySection;
use tinymemory_api::null::NullMemoryProvider;
use tinymemory_api::recall::OwnedRecallOpts;
use tinymemory_api::traits::Memory;
use tinymemory_api::types::{
    MemoryCategory, MemoryEntry, MemoryTaint, NamespaceSummary, RecallOpts,
};

use crate::mandatory::MemoryTraitProvider;

use super::types::merge_hits;
use super::{Sections, MAX_SECTION_NAMESPACES, NAMESPACE_FILTER_CONFLICT};

/// Build an entry directly, so a test can set the `score` no API accepts.
fn entry(namespace: &str, key: &str, content: &str, score: Option<f64>) -> MemoryEntry {
    MemoryEntry {
        id: format!("{namespace}/{key}"),
        key: key.to_string(),
        content: content.to_string(),
        namespace: Some(namespace.to_string()),
        category: MemoryCategory::Core,
        timestamp: "2026-01-01T00:00:00Z".to_string(),
        session_id: None,
        score,
        taint: MemoryTaint::Internal,
    }
}

/// A `BTreeMap`-backed `Memory` whose recall honours an exact namespace filter
/// and returns the scores the test seeded.
#[derive(Default)]
struct ScoredMemory {
    entries: Mutex<BTreeMap<(String, String), MemoryEntry>>,
}

impl ScoredMemory {
    fn provider() -> (Arc<Self>, MemoryTraitProvider) {
        let memory = Arc::new(Self::default());
        let provider = MemoryTraitProvider::new(memory.clone(), "scored-double");
        (memory, provider)
    }

    /// Insert an entry with a chosen score, bypassing the score-less `store`.
    fn seed(&self, namespace: &str, key: &str, content: &str, score: Option<f64>) {
        self.entries.lock().unwrap().insert(
            (namespace.to_string(), key.to_string()),
            entry(namespace, key, content, score),
        );
    }

    fn rows(&self) -> Vec<MemoryEntry> {
        self.entries.lock().unwrap().values().cloned().collect()
    }
}

#[async_trait]
impl Memory for ScoredMemory {
    fn name(&self) -> &'static str {
        "scored-double"
    }

    async fn store(
        &self,
        namespace: &str,
        key: &str,
        content: &str,
        category: MemoryCategory,
        session_id: Option<&str>,
    ) -> anyhow::Result<()> {
        self.store_with_taint(
            namespace,
            key,
            content,
            category,
            session_id,
            MemoryTaint::Internal,
        )
        .await
    }

    async fn store_with_taint(
        &self,
        namespace: &str,
        key: &str,
        content: &str,
        category: MemoryCategory,
        session_id: Option<&str>,
        taint: MemoryTaint,
    ) -> anyhow::Result<()> {
        let mut row = entry(namespace, key, content, None);
        row.category = category;
        row.session_id = session_id.map(str::to_string);
        row.taint = taint;
        self.entries
            .lock()
            .unwrap()
            .insert((namespace.to_string(), key.to_string()), row);
        Ok(())
    }

    async fn recall(
        &self,
        query: &str,
        limit: usize,
        opts: RecallOpts<'_>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        // An exact namespace match, as the contract specifies. A `None`
        // namespace matches nothing here on purpose: the fan-out must never
        // depend on what `None` means, because the bundled drivers disagree.
        Ok(self
            .rows()
            .into_iter()
            .filter(|row| row.namespace.as_deref() == opts.namespace)
            .filter(|row| query.is_empty() || row.content.contains(query))
            .take(limit)
            .collect())
    }

    async fn get(&self, namespace: &str, key: &str) -> anyhow::Result<Option<MemoryEntry>> {
        Ok(self
            .entries
            .lock()
            .unwrap()
            .get(&(namespace.to_string(), key.to_string()))
            .cloned())
    }

    async fn list(
        &self,
        namespace: Option<&str>,
        category: Option<&MemoryCategory>,
        session_id: Option<&str>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        Ok(self
            .rows()
            .into_iter()
            .filter(|row| namespace.is_none_or(|ns| row.namespace.as_deref() == Some(ns)))
            .filter(|row| category.is_none_or(|cat| &row.category == cat))
            .filter(|row| session_id.is_none_or(|sid| row.session_id.as_deref() == Some(sid)))
            .collect())
    }

    async fn forget(&self, namespace: &str, key: &str) -> anyhow::Result<bool> {
        Ok(self
            .entries
            .lock()
            .unwrap()
            .remove(&(namespace.to_string(), key.to_string()))
            .is_some())
    }

    async fn namespace_summaries(&self) -> anyhow::Result<Vec<NamespaceSummary>> {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for row in self.rows() {
            if let Some(namespace) = row.namespace {
                *counts.entry(namespace).or_default() += 1;
            }
        }
        Ok(counts
            .into_iter()
            .map(|(namespace, count)| NamespaceSummary {
                namespace,
                count,
                last_updated: None,
            })
            .collect())
    }

    async fn count(&self) -> anyhow::Result<usize> {
        Ok(self.entries.lock().unwrap().len())
    }

    async fn health_check(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------- merge_hits

#[test]
fn merge_orders_by_score_descending() {
    let merged = merge_hits(
        vec![
            entry("learning:a", "low", "x", Some(0.1)),
            entry("learning:b", "high", "x", Some(0.9)),
            entry("learning:c", "mid", "x", Some(0.5)),
        ],
        10,
    );
    let keys: Vec<&str> = merged.iter().map(|e| e.key.as_str()).collect();
    assert_eq!(keys, ["high", "mid", "low"]);
}

#[test]
fn merge_sorts_absent_scores_last() {
    let merged = merge_hits(
        vec![
            entry("learning:a", "unscored", "x", None),
            entry("learning:b", "scored", "x", Some(0.01)),
        ],
        10,
    );
    let keys: Vec<&str> = merged.iter().map(|e| e.key.as_str()).collect();
    assert_eq!(keys, ["scored", "unscored"]);
}

#[test]
fn merge_breaks_ties_by_namespace_then_key() {
    let merged = merge_hits(
        vec![
            entry("learning:b", "second", "x", Some(0.5)),
            entry("learning:a", "zebra", "x", Some(0.5)),
            entry("learning:a", "alpha", "x", Some(0.5)),
        ],
        10,
    );
    let pairs: Vec<(&str, &str)> = merged
        .iter()
        .map(|e| (e.namespace.as_deref().unwrap(), e.key.as_str()))
        .collect();
    assert_eq!(
        pairs,
        [
            ("learning:a", "alpha"),
            ("learning:a", "zebra"),
            ("learning:b", "second"),
        ]
    );
}

#[test]
fn merge_truncates_after_ranking_not_before() {
    let merged = merge_hits(
        vec![
            entry("learning:a", "low", "x", Some(0.1)),
            entry("learning:b", "high", "x", Some(0.9)),
        ],
        1,
    );
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].key, "high");
}

// --------------------------------------------------------------- SectionView

#[tokio::test]
async fn put_writes_under_the_section_prefix() {
    let (memory, provider) = ScoredMemory::provider();
    let namespace = Sections::new(&provider)
        .conversations()
        .put(
            "thread-8f21",
            "turn-1",
            "hello",
            MemoryCategory::Core,
            None,
            MemoryTaint::Internal,
        )
        .await
        .unwrap();

    assert_eq!(namespace.as_str(), "conversation:thread-8f21");
    assert_eq!(
        memory.rows()[0].namespace.as_deref(),
        Some("conversation:thread-8f21")
    );
}

#[tokio::test]
async fn get_reads_back_what_put_wrote() {
    let (_memory, provider) = ScoredMemory::provider();
    let sections = Sections::new(&provider);
    sections
        .learnings()
        .put(
            "rust-async",
            "pin",
            "pin is not unpin",
            MemoryCategory::Core,
            None,
            MemoryTaint::Internal,
        )
        .await
        .unwrap();

    let found = sections.learnings().get("rust-async", "pin").await.unwrap();
    assert_eq!(found.unwrap().content, "pin is not unpin");
}

#[tokio::test]
async fn each_named_section_is_isolated_from_the_others() {
    let (_memory, provider) = ScoredMemory::provider();
    let sections = Sections::new(&provider);
    sections
        .documents()
        .put(
            "handbook",
            "k",
            "doc",
            MemoryCategory::Core,
            None,
            MemoryTaint::Internal,
        )
        .await
        .unwrap();

    // Same scope and key, a different section: not visible.
    assert!(sections
        .conversations()
        .get("handbook", "k")
        .await
        .unwrap()
        .is_none());
    assert!(sections
        .learnings()
        .get("handbook", "k")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn forget_is_idempotent() {
    let (_memory, provider) = ScoredMemory::provider();
    let sections = Sections::new(&provider);
    sections
        .documents()
        .put(
            "handbook",
            "k",
            "doc",
            MemoryCategory::Core,
            None,
            MemoryTaint::Internal,
        )
        .await
        .unwrap();

    assert!(sections.documents().forget("handbook", "k").await.unwrap());
    assert!(!sections.documents().forget("handbook", "k").await.unwrap());
}

#[tokio::test]
async fn an_empty_scope_is_rejected_without_storing() {
    let (memory, provider) = ScoredMemory::provider();
    let err = Sections::new(&provider)
        .conversations()
        .put(
            "",
            "turn-1",
            "hello",
            MemoryCategory::Core,
            None,
            MemoryTaint::Internal,
        )
        .await
        .expect_err("an empty scope cannot form a namespace");

    assert!(matches!(err, MemoryError::Invalid(_)), "got {err:?}");
    assert!(
        memory.rows().is_empty(),
        "a rejected put must store nothing"
    );
}

#[tokio::test]
async fn an_overlong_scope_is_rejected() {
    let (_memory, provider) = ScoredMemory::provider();
    let err = Sections::new(&provider)
        .learnings()
        .namespace(&"x".repeat(500))
        .expect_err("an overlong scope cannot form a namespace");
    assert!(matches!(err, MemoryError::Invalid(_)), "got {err:?}");
}

#[tokio::test]
async fn scopes_lists_only_this_sections_namespaces() {
    let (memory, provider) = ScoredMemory::provider();
    memory.seed("conversation:one", "a", "x", None);
    memory.seed("conversation:two", "b", "x", None);
    memory.seed("learning:rust", "c", "x", None);
    memory.seed("research-notes", "d", "x", None); // unsectioned, legacy
    memory.seed("ops:deploys", "e", "x", None); // a Custom section

    let sections = Sections::new(&provider);
    let mut conversations: Vec<String> = sections
        .conversations()
        .scopes()
        .await
        .unwrap()
        .iter()
        .map(|s| s.scope().to_string())
        .collect();
    conversations.sort();
    assert_eq!(conversations, ["one", "two"]);

    let learnings = sections.learnings().scopes().await.unwrap();
    assert_eq!(learnings.len(), 1);
    assert_eq!(learnings[0].scope(), "rust");

    // A custom section is reachable, and is never confused for a known one.
    let ops = sections
        .section(MemorySection::Custom("ops".to_string()))
        .scopes()
        .await
        .unwrap();
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].scope(), "deploys");
}

#[tokio::test]
async fn scopes_orders_by_entry_count_descending() {
    let (memory, provider) = ScoredMemory::provider();
    memory.seed("learning:small", "a", "x", None);
    memory.seed("learning:big", "b", "x", None);
    memory.seed("learning:big", "c", "x", None);

    let scopes = Sections::new(&provider).learnings().scopes().await.unwrap();
    let ordered: Vec<&str> = scopes.iter().map(super::SectionScope::scope).collect();
    assert_eq!(ordered, ["big", "small"]);
}

#[tokio::test]
async fn list_section_spans_every_scope_in_the_section() {
    let (memory, provider) = ScoredMemory::provider();
    memory.seed("learning:a", "one", "x", None);
    memory.seed("learning:b", "two", "x", None);
    memory.seed("conversation:c", "three", "x", None);

    let entries = Sections::new(&provider)
        .learnings()
        .list_section(None, None)
        .await
        .unwrap();
    let keys: Vec<&str> = entries.iter().map(|e| e.key.as_str()).collect();
    assert_eq!(keys, ["one", "two"], "ordered by namespace then key");
}

// ------------------------------------------------------------- SectionRecall

fn opts() -> OwnedRecallOpts {
    OwnedRecallOpts::default()
}

#[tokio::test]
async fn in_scope_recall_is_confined_to_one_namespace() {
    let (memory, provider) = ScoredMemory::provider();
    memory.seed("learning:rust", "a", "shipping async", Some(0.9));
    memory.seed("learning:go", "b", "shipping async", Some(0.9));

    let found = Sections::new(&provider)
        .recall()
        .in_scope(
            &MemorySection::Learning,
            "rust",
            "shipping",
            10,
            &opts(),
            None,
        )
        .await
        .unwrap();

    assert_eq!(found.namespaces_searched, 1);
    assert_eq!(found.hits.len(), 1);
    assert_eq!(found.hits[0].namespace.as_deref(), Some("learning:rust"));
}

#[tokio::test]
async fn in_scope_rejects_recall_options_that_pin_a_namespace() {
    let (_memory, provider) = ScoredMemory::provider();
    let pinned = OwnedRecallOpts {
        namespace: Some("learning:elsewhere".to_string()),
        ..OwnedRecallOpts::default()
    };

    let err = Sections::new(&provider)
        .recall()
        .in_scope(&MemorySection::Learning, "rust", "q", 10, &pinned, None)
        .await
        .expect_err("a pinned namespace conflicts with the section");

    match err {
        MemoryError::Invalid(message) => assert_eq!(message, NAMESPACE_FILTER_CONFLICT),
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[tokio::test]
async fn across_section_merges_every_scope_and_ranks_by_score() {
    let (memory, provider) = ScoredMemory::provider();
    memory.seed("learning:rust", "mid", "async", Some(0.5));
    memory.seed("learning:go", "high", "async", Some(0.9));
    memory.seed("learning:zig", "low", "async", Some(0.1));
    memory.seed("conversation:chat", "other", "async", Some(1.0));

    let found = Sections::new(&provider)
        .recall()
        .across_section(&MemorySection::Learning, "async", 10, &opts(), None)
        .await
        .unwrap();

    assert_eq!(found.namespaces_searched, 3);
    assert!(!found.truncated);
    let keys: Vec<&str> = found.hits.iter().map(|e| e.key.as_str()).collect();
    assert_eq!(
        keys,
        ["high", "mid", "low"],
        "ranked across namespaces, and never crossing into another section"
    );
}

#[tokio::test]
async fn across_section_truncates_the_hits_to_the_limit() {
    let (memory, provider) = ScoredMemory::provider();
    memory.seed("learning:a", "high", "async", Some(0.9));
    memory.seed("learning:b", "low", "async", Some(0.1));

    let found = Sections::new(&provider)
        .recall()
        .across_section(&MemorySection::Learning, "async", 1, &opts(), None)
        .await
        .unwrap();

    assert_eq!(found.hits.len(), 1);
    assert_eq!(found.hits[0].key, "high", "the limit keeps the best hit");
    assert!(
        !found.truncated,
        "reaching the hit limit is not namespace truncation"
    );
}

#[tokio::test]
async fn across_section_reports_truncation_past_the_namespace_cap() {
    let (memory, provider) = ScoredMemory::provider();
    for index in 0..=MAX_SECTION_NAMESPACES {
        memory.seed(
            &format!("learning:topic-{index:03}"),
            "k",
            "async",
            Some(0.5),
        );
    }

    let found = Sections::new(&provider)
        .recall()
        .across_section(&MemorySection::Learning, "async", 1000, &opts(), None)
        .await
        .unwrap();

    assert_eq!(found.namespaces_searched, MAX_SECTION_NAMESPACES);
    assert!(found.truncated, "one namespace was skipped");
}

#[tokio::test]
async fn across_section_rejects_recall_options_that_pin_a_namespace() {
    let (_memory, provider) = ScoredMemory::provider();
    let pinned = OwnedRecallOpts {
        namespace: Some("learning:elsewhere".to_string()),
        ..OwnedRecallOpts::default()
    };

    let err = Sections::new(&provider)
        .recall()
        .across_section(&MemorySection::Learning, "q", 10, &pinned, None)
        .await
        .expect_err("a pinned namespace conflicts with the section");

    assert!(matches!(err, MemoryError::Invalid(_)), "got {err:?}");
}

#[tokio::test]
async fn across_section_on_an_empty_section_is_ok_and_empty() {
    let (_memory, provider) = ScoredMemory::provider();
    let found = Sections::new(&provider)
        .recall()
        .across_section(&MemorySection::Learning, "async", 10, &opts(), None)
        .await
        .unwrap();

    assert!(found.hits.is_empty());
    assert_eq!(found.namespaces_searched, 0);
    assert!(!found.truncated);
}

// ------------------------------------------------- works on any provider

#[tokio::test]
async fn the_whole_surface_succeeds_on_a_provider_that_retains_nothing() {
    let provider = NullMemoryProvider::new();
    let sections = Sections::new(&provider);

    for view in [
        sections.conversations(),
        sections.learnings(),
        sections.documents(),
    ] {
        view.put(
            "scope",
            "key",
            "content",
            MemoryCategory::Core,
            None,
            MemoryTaint::Internal,
        )
        .await
        .expect("put must succeed");
        assert!(view.get("scope", "key").await.expect("get").is_none());
        assert!(!view.forget("scope", "key").await.expect("forget"));
        assert!(view
            .list("scope", None, None)
            .await
            .expect("list")
            .is_empty());
        assert!(view.scopes().await.expect("scopes").is_empty());
        assert!(view
            .list_section(None, None)
            .await
            .expect("list_section")
            .is_empty());
    }

    let found = sections
        .recall()
        .across_section(&MemorySection::Conversation, "q", 10, &opts(), None)
        .await
        .expect("across_section must succeed");
    assert!(found.hits.is_empty());
    assert_eq!(found.namespaces_searched, 0);
}
