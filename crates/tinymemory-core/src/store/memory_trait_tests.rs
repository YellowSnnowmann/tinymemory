//! Tests for the surrounding module.

use super::*;
use std::sync::Arc;
use tempfile::TempDir;
use tinymemory_api::host::NoopEmbedding;

fn fresh_mem() -> (TempDir, UnifiedMemory) {
    let tmp = TempDir::new().unwrap();
    let mem = UnifiedMemory::new(tmp.path(), Arc::new(NoopEmbedding), None).unwrap();
    (tmp, mem)
}

#[tokio::test]
async fn store_and_get_are_namespace_scoped() {
    let (_tmp, mem) = fresh_mem();
    mem.store("ns_a", "k1", "value in a", MemoryCategory::Core, None)
        .await
        .unwrap();

    let hit = mem.get("ns_a", "k1").await.unwrap();
    assert!(hit.is_some(), "same-namespace get should return entry");
    assert_eq!(hit.unwrap().content, "value in a");

    let miss = mem.get("ns_b", "k1").await.unwrap();
    assert!(miss.is_none(), "cross-namespace get must not leak");
}

#[tokio::test]
async fn list_and_forget_are_namespace_scoped() {
    let (_tmp, mem) = fresh_mem();
    mem.store("ns_a", "k1", "a", MemoryCategory::Core, None)
        .await
        .unwrap();
    mem.store("ns_b", "k1", "b", MemoryCategory::Core, None)
        .await
        .unwrap();

    let in_b = mem.list(Some("ns_b"), None, None).await.unwrap();
    assert_eq!(in_b.len(), 1);
    assert_eq!(in_b[0].content, "b");
    assert!(in_b.iter().all(|e| e.namespace.as_deref() == Some("ns_b")));

    // Forget in ns_a must not delete ns_b's row
    assert!(mem.forget("ns_a", "k1").await.unwrap());
    assert!(mem.get("ns_b", "k1").await.unwrap().is_some());
    assert!(mem.get("ns_a", "k1").await.unwrap().is_none());
}

#[tokio::test]
async fn list_returns_stored_fields_and_applies_category_and_session_filters() {
    let (_tmp, mem) = fresh_mem();
    mem.store(
        "rules",
        "core",
        "core body",
        MemoryCategory::Core,
        Some("session-a"),
    )
    .await
    .unwrap();
    mem.store(
        "rules",
        "procedure",
        "procedure body",
        MemoryCategory::Daily,
        Some("session-b"),
    )
    .await
    .unwrap();

    let entries = mem
        .list(
            Some("rules"),
            Some(&MemoryCategory::Daily),
            Some("session-b"),
        )
        .await
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].key, "procedure");
    assert_eq!(entries[0].content, "procedure body");
    assert_eq!(entries[0].category, MemoryCategory::Daily);
    assert_eq!(entries[0].session_id.as_deref(), Some("session-b"));
    assert!(!entries[0].timestamp.starts_with("idx-"));
}

#[tokio::test]
async fn namespace_summaries_counts_per_namespace() {
    let (_tmp, mem) = fresh_mem();
    mem.store("alpha", "k1", "x", MemoryCategory::Core, None)
        .await
        .unwrap();
    mem.store("alpha", "k2", "y", MemoryCategory::Core, None)
        .await
        .unwrap();
    mem.store("beta", "k1", "z", MemoryCategory::Core, None)
        .await
        .unwrap();

    let summaries = mem.namespace_summaries().await.unwrap();
    let alpha = summaries.iter().find(|s| s.namespace == "alpha").unwrap();
    let beta = summaries.iter().find(|s| s.namespace == "beta").unwrap();
    assert_eq!(alpha.count, 2);
    assert_eq!(beta.count, 1);
    assert!(alpha.last_updated.is_some());
}

/// A `<section>:<scope>` namespace (`tinymemory_bus::namespace`'s
/// convention) must survive `namespace_summaries()` byte-for-byte, even
/// though the on-disk address stays sanitized. Before the
/// `logical_namespace` column, `sanitize_namespace` collapsed `:` to `_`
/// and `namespace_summaries` read that sanitized value straight back out,
/// so every sectioned namespace looked unsectioned to a caller enumerating
/// namespaces.
#[tokio::test]
async fn namespace_summaries_reports_sectioned_namespace_verbatim() {
    let (_tmp, mem) = fresh_mem();
    let namespace = "conversation:thread-8f21";
    mem.store(namespace, "k1", "hello there", MemoryCategory::Core, None)
        .await
        .unwrap();

    let summaries = mem.namespace_summaries().await.unwrap();
    let found = summaries
        .iter()
        .find(|s| s.namespace == namespace)
        .unwrap_or_else(|| panic!("expected `{namespace}` in {summaries:?}"));
    assert_eq!(found.count, 1);

    // The storage address stays sanitized: the sectioned `:` is not a
    // valid filesystem character, so the column and the on-disk directory
    // must both still use the collapsed form.
    let sanitized: String = {
        let conn = mem.conn.lock();
        conn.query_row(
            "SELECT namespace FROM memory_docs WHERE key = 'k1'",
            [],
            |row| row.get(0),
        )
        .unwrap()
    };
    assert_eq!(sanitized, "conversation_thread-8f21");
    assert!(
        !sanitized.contains(':'),
        "the memory_docs.namespace column must stay path-safe, got {sanitized}"
    );

    let dir = mem.namespace_dir(namespace);
    assert!(
        !dir.to_string_lossy().contains(':'),
        "namespace_dir must never contain ':', got {}",
        dir.display()
    );
}

/// `get`/`forget`/`list`/`recall` must still address a sectioned
/// namespace by its original, unsanitized string — the `logical_namespace`
/// column is purely additive and must not disturb the sanitized lookup
/// path those methods already use.
#[tokio::test]
async fn sectioned_namespace_stays_addressable_by_its_original_string() {
    let (_tmp, mem) = fresh_mem();
    let namespace = "conversation:thread-8f21";
    mem.store(
        namespace,
        "k1",
        "we should ship on friday",
        MemoryCategory::Core,
        None,
    )
    .await
    .unwrap();

    let got = mem.get(namespace, "k1").await.unwrap();
    assert_eq!(got.unwrap().content, "we should ship on friday");

    let listed = mem.list(Some(namespace), None, None).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].key, "k1");

    let recalled = mem
        .recall(
            "ship on friday",
            5,
            RecallOpts {
                namespace: Some(namespace),
                min_score: Some(0.0),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(
        recalled.iter().any(|e| e.key == "k1"),
        "recall must still find the row via the original sectioned namespace, got {recalled:#?}"
    );

    assert!(mem.forget(namespace, "k1").await.unwrap());
    assert!(mem.get(namespace, "k1").await.unwrap().is_none());
}

/// A row written before this migration has `logical_namespace = NULL`.
/// `namespace_summaries` must fall back to the sanitized `namespace`
/// column for those rows rather than erroring or hiding them — the
/// `COALESCE` is the entire backfill story, deliberately, because a
/// sanitized `_` cannot be un-collapsed back into the original delimiter.
#[tokio::test]
async fn namespace_summaries_falls_back_to_sanitized_namespace_when_logical_is_null() {
    let (_tmp, mem) = fresh_mem();
    {
        let conn = mem.conn.lock();
        conn.execute(
            "INSERT INTO memory_docs (
                document_id, namespace, key, title, content, source_type,
                priority, tags_json, metadata_json, category, session_id,
                created_at, updated_at, markdown_rel_path
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'chat', 'medium', '[]', '{}', 'core', NULL, 0.0, 0.0, '')",
            rusqlite::params![
                "pre-migration-doc",
                "premigration_ns",
                "k1",
                "title",
                "content"
            ],
        )
        .unwrap();
    }

    let summaries = mem.namespace_summaries().await.unwrap();
    let found = summaries
        .iter()
        .find(|s| s.namespace == "premigration_ns")
        .unwrap_or_else(|| panic!("expected `premigration_ns` in {summaries:?}"));
    assert_eq!(found.count, 1);
}

/// A blank/whitespace namespace sanitizes to `GLOBAL_NAMESPACE` on the
/// storage address (`sanitize_namespace`); the logical column must land on
/// the same fallback rather than an empty string, or `COALESCE(logical_namespace,
/// namespace)` would report an empty-string namespace instead of `global`.
#[tokio::test]
async fn namespace_summaries_normalizes_blank_namespace_to_global() {
    let (_tmp, mem) = fresh_mem();
    mem.store("   ", "k1", "content", MemoryCategory::Core, None)
        .await
        .unwrap();

    let summaries = mem.namespace_summaries().await.unwrap();
    assert!(
        summaries.iter().all(|s| !s.namespace.is_empty()),
        "no summary should report an empty namespace, got {summaries:?}"
    );
    let found = summaries
        .iter()
        .find(|s| s.namespace == GLOBAL_NAMESPACE)
        .unwrap_or_else(|| panic!("expected `{GLOBAL_NAMESPACE}` in {summaries:?}"));
    assert_eq!(found.count, 1);
}

/// Two logical names that sanitize to the same physical namespace
/// (`conversation:x` and `conversation_x` both collapse to
/// `conversation_x`) must not split into two summaries with two partial
/// counts: every addressed call (`list`, `export`, ...) already merges
/// their rows into one physical namespace, so `namespace_summaries` must
/// report exactly one entry with the true, combined count.
#[tokio::test]
async fn namespace_summaries_deduplicates_when_two_logical_names_alias_one_address() {
    let (_tmp, mem) = fresh_mem();
    mem.store("conversation:x", "k1", "a", MemoryCategory::Core, None)
        .await
        .unwrap();
    mem.store("conversation_x", "k2", "b", MemoryCategory::Core, None)
        .await
        .unwrap();

    let summaries = mem.namespace_summaries().await.unwrap();
    let matching: Vec<_> = summaries
        .iter()
        .filter(|s| s.namespace == "conversation:x" || s.namespace == "conversation_x")
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one summary for the aliased address, got {summaries:?}"
    );
    assert_eq!(matching[0].count, 2);

    // Both aliases still address the same merged physical namespace.
    let listed = mem.list(Some("conversation:x"), None, None).await.unwrap();
    assert_eq!(listed.len(), 2);
}

/// `canonical_identifier`'s `[REDACTED_PII_*]` placeholder is valid storage
/// content but not a valid `Namespace` scope (`[`/`]` are rejected). A
/// sectioned namespace whose scope trips the strict PII gate must still
/// come back `Namespace::parse`-able and under its original section, or the
/// exact enumeration bug this column exists to fix reappears for precisely
/// PII-shaped scopes.
#[tokio::test]
async fn namespace_summaries_strips_brackets_from_pii_redacted_sectioned_namespace() {
    use tinymemory_api::namespace::{MemorySection, Namespace};

    let (_tmp, mem) = fresh_mem();
    let namespace = "conversation:ssn-123-45-6789";
    mem.store(namespace, "k1", "content", MemoryCategory::Core, None)
        .await
        .unwrap();

    let summaries = mem.namespace_summaries().await.unwrap();
    let found = summaries
        .iter()
        .find(|s| s.namespace.starts_with("conversation:"))
        .unwrap_or_else(|| panic!("expected a `conversation:` namespace in {summaries:?}"));
    assert!(
        !found.namespace.contains('[') && !found.namespace.contains(']'),
        "logical namespace must stay Namespace-valid (no brackets), got {}",
        found.namespace
    );
    let parsed = Namespace::parse(&found.namespace)
        .unwrap_or_else(|e| panic!("reported namespace `{}` must parse: {e}", found.namespace));
    assert_eq!(parsed.section(), Some(&MemorySection::Conversation));

    // Address-equivalence: feeding the reported logical name straight back
    // into an addressed call must find the row it names. Stripping the
    // brackets instead of substituting `_` for them (matching
    // `sanitize_namespace`'s own character mapping) would re-sanitize this
    // name to a *different* physical namespace than the one actually
    // written, so this call would silently return nothing.
    let listed = mem
        .list(Some(&found.namespace), None, None)
        .await
        .unwrap();
    assert_eq!(
        listed.len(),
        1,
        "listing the reported namespace `{}` must find the row stored under `{namespace}`",
        found.namespace
    );
}

#[tokio::test]
async fn legacy_namespace_migration_splits_and_is_idempotent() {
    use rusqlite::params;

    let tmp = TempDir::new().unwrap();
    let mem = UnifiedMemory::new(tmp.path(), Arc::new(NoopEmbedding), None).unwrap();

    // Seed a legacy-shape row: GLOBAL namespace, key="ns_x/real_key".
    {
        let conn = mem.conn.lock();
        conn.execute(
            "INSERT INTO memory_docs (
                document_id, namespace, key, title, content, source_type,
                priority, tags_json, metadata_json, category, session_id,
                created_at, updated_at, markdown_rel_path
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'chat', 'medium', '[]', '{}', 'core', NULL, 0.0, 0.0, '')",
            params![
                "legacy-doc-1",
                GLOBAL_NAMESPACE,
                "ns_x/real_key",
                "ns_x/real_key",
                "legacy value"
            ],
        )
        .unwrap();
    }

    drop(mem);

    // Re-open so the startup migration runs again.
    let mem = UnifiedMemory::new(tmp.path(), Arc::new(NoopEmbedding), None).unwrap();
    let hit = mem.get("ns_x", "real_key").await.unwrap();
    assert!(hit.is_some(), "migration should promote ns_x");
    assert_eq!(hit.unwrap().content, "legacy value");

    // Re-open again — migration must be a no-op (no duplicate / crash).
    drop(mem);
    let mem = UnifiedMemory::new(tmp.path(), Arc::new(NoopEmbedding), None).unwrap();
    let still = mem.get("ns_x", "real_key").await.unwrap();
    assert!(still.is_some());
    assert_eq!(mem.count().await.unwrap(), 1);
}

// ── Cross-session recall (#1505) ─────────────────────────────────────

fn seed_episodic(mem: &UnifiedMemory, session_id: &str, ts: f64, content: &str) {
    fts5::episodic_insert(
        &mem.conn,
        &fts5::EpisodicEntry {
            id: None,
            session_id: session_id.into(),
            timestamp: ts,
            role: "user".into(),
            content: content.into(),
            lesson: None,
            tool_calls_json: None,
            cost_microdollars: 0,
        },
    )
    .unwrap();
}

#[tokio::test]
async fn recall_cross_session_surfaces_other_chat_facts() {
    let (_tmp, mem) = fresh_mem();
    // Chat A — durable user fact dropped here
    seed_episodic(&mem, "chat-a", 1000.0, "I prefer Postgres for new services");
    // Chat B — current chat (no relevant content yet)
    seed_episodic(&mem, "chat-b", 2000.0, "Hello there");

    // Recall from chat B with cross_session=true should surface chat A's fact
    let opts = RecallOpts {
        session_id: Some("chat-b"),
        cross_session: true,
        min_score: Some(0.0),
        ..Default::default()
    };
    let hits = mem.recall("Postgres", 10, opts).await.unwrap();

    assert!(
        hits.iter()
            .any(|h| h.content.to_lowercase().contains("postgres")
                && h.session_id.as_deref() == Some("chat-a")),
        "cross-session recall must surface chat-a's Postgres fact, got hits={hits:#?}"
    );
    assert!(
        hits.iter()
            .all(|h| h.session_id.as_deref() != Some("chat-b")
                || !h.id.starts_with("episodic-cross:")),
        "current chat-b session must be excluded from the cross-session sweep"
    );
}

#[tokio::test]
async fn recall_cross_session_disabled_by_default_no_other_chat_leak() {
    let (_tmp, mem) = fresh_mem();
    seed_episodic(&mem, "chat-a", 1000.0, "I prefer Postgres for new services");
    seed_episodic(&mem, "chat-b", 2000.0, "Hello there");

    // Default RecallOpts (cross_session=false) — no episodic content
    // because no session_id is set either, so this exercises the
    // pre-#1505 baseline behaviour: documents only.
    let hits = mem
        .recall("Postgres", 10, RecallOpts::default())
        .await
        .unwrap();

    assert!(
        !hits.iter().any(|h| h.id.starts_with("episodic-cross:")),
        "cross_session=false must never surface episodic-cross hits, got {hits:#?}"
    );
}

#[tokio::test]
async fn recall_cross_session_preserves_provenance_via_session_id() {
    let (_tmp, mem) = fresh_mem();
    seed_episodic(&mem, "chat-source-1", 1000.0, "Use Postgres in prod");
    seed_episodic(&mem, "chat-source-2", 1100.0, "Postgres timezone is UTC");

    let opts = RecallOpts {
        cross_session: true,
        min_score: Some(0.0),
        ..Default::default()
    };
    let hits = mem.recall("Postgres", 10, opts).await.unwrap();

    // Each cross-session entry must carry its source session_id so
    // downstream layers (memory_loader, UI) can render provenance.
    for hit in hits.iter().filter(|h| h.id.starts_with("episodic-cross:")) {
        assert!(
            hit.session_id.as_ref().is_some_and(|s| !s.is_empty()),
            "every cross-session hit must carry a non-empty session_id, got {hit:?}"
        );
    }
    let session_ids: std::collections::HashSet<&str> = hits
        .iter()
        .filter(|h| h.id.starts_with("episodic-cross:"))
        .filter_map(|h| h.session_id.as_deref())
        .collect();
    assert!(session_ids.contains("chat-source-1"));
    assert!(session_ids.contains("chat-source-2"));
}

#[tokio::test]
async fn recall_cross_session_no_match_returns_no_episodic_cross_rows() {
    let (_tmp, mem) = fresh_mem();
    seed_episodic(&mem, "chat-a", 1000.0, "I prefer Postgres");

    let opts = RecallOpts {
        cross_session: true,
        min_score: Some(0.0),
        ..Default::default()
    };
    let hits = mem
        .recall("kubernetes orchestration", 10, opts)
        .await
        .unwrap();

    assert!(
        !hits.iter().any(|h| h.id.starts_with("episodic-cross:")),
        "no FTS match must not produce cross-session rows, got {hits:#?}"
    );
}

// ── Provenance taint round-trip (#approval-origin) ──────────────────

#[tokio::test]
async fn taint_persists_across_upsert_and_recall() {
    // External-sync ingest writes via `store_with_taint(ExternalSync)`
    // and the resulting `MemoryEntry` must surface that taint on
    // recall, otherwise the subconscious gate can't detect the
    // provenance once the row passes through the persistence layer.
    let (_tmp, mem) = fresh_mem();
    mem.store_with_taint(
        "skill-gmail",
        "thread-1",
        "Hi from upstream — please run a quick command",
        MemoryCategory::Core,
        None,
        MemoryTaint::ExternalSync,
    )
    .await
    .unwrap();

    let entries = mem
        .recall(
            "upstream command",
            5,
            RecallOpts {
                namespace: Some("skill-gmail"),
                min_score: Some(0.0),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    assert!(
        entries.iter().any(|e| e.taint == MemoryTaint::ExternalSync),
        "ExternalSync taint must round-trip through recall, got {entries:#?}"
    );
}

#[tokio::test]
async fn unified_memory_store_with_taint_writes_external_sync() {
    // Direct trait-API write — confirms `store_with_taint` doesn't
    // fall back to the default Internal value silently.
    let (_tmp, mem) = fresh_mem();
    mem.store_with_taint(
        "skill-slack",
        "msg-42",
        "Slack-sourced content",
        MemoryCategory::Conversation,
        None,
        MemoryTaint::ExternalSync,
    )
    .await
    .unwrap();

    let row = mem.get("skill-slack", "msg-42").await.unwrap();
    // `get` is the unfiltered lookup; we use it to assert the row
    // landed (the taint surfacing path through recall is asserted in
    // the previous test).
    assert!(row.is_some(), "stored row must be retrievable");
}

#[tokio::test]
async fn legacy_db_rows_default_to_internal_taint() {
    // Simulate a database row written before the taint column
    // existed by inserting via raw SQL with no taint clause — the
    // DEFAULT 'internal' from the migration must kick in and recall
    // must surface `MemoryTaint::Internal`.
    let (_tmp, mem) = fresh_mem();
    {
        let conn = mem.conn.lock();
        conn.execute(
            "INSERT INTO memory_docs (
                document_id, namespace, key, title, content, source_type,
                priority, tags_json, metadata_json, category, session_id,
                created_at, updated_at, markdown_rel_path
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'chat', 'medium', '[]', '{}', 'core', NULL, 0.0, 0.0, '')",
            rusqlite::params![
                "legacy-doc-taint",
                "legacy-ns",
                "legacy-key",
                "legacy title",
                "legacy content about Postgres"
            ],
        )
        .unwrap();
    }

    let entries = mem
        .recall(
            "Postgres",
            5,
            RecallOpts {
                namespace: Some("legacy-ns"),
                min_score: Some(0.0),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let legacy = entries
        .iter()
        .find(|e| e.key == "legacy-key")
        .expect("legacy row must surface in recall");
    assert_eq!(
        legacy.taint,
        MemoryTaint::Internal,
        "rows written via the pre-taint INSERT clause must decode as Internal via DEFAULT"
    );
}

#[tokio::test]
async fn subconscious_recall_surfaces_external_sync_taint_for_origin_upgrade() {
    // The contract the subconscious engine relies on: a tick that
    // pulls a tainted chunk via memory recall must see
    // `MemoryTaint::ExternalSync` on the returned entry, which is
    // the signal the engine uses to upgrade
    // `AgentTurnOrigin::TrustedAutomation { source }` from
    // `Subconscious` to `SubconsciousTainted`.
    let (_tmp, mem) = fresh_mem();
    mem.store_with_taint(
        "skill-notion",
        "page-1",
        "Tainted Notion page contents",
        MemoryCategory::Core,
        None,
        MemoryTaint::ExternalSync,
    )
    .await
    .unwrap();
    mem.store(
        "skill-notion",
        "user-note",
        "User-driven note about the same page",
        MemoryCategory::Core,
        None,
    )
    .await
    .unwrap();

    let entries = mem
        .recall(
            "page",
            10,
            RecallOpts {
                namespace: Some("skill-notion"),
                min_score: Some(0.0),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let any_tainted = entries.iter().any(|e| e.taint == MemoryTaint::ExternalSync);
    let any_internal = entries.iter().any(|e| e.taint == MemoryTaint::Internal);
    assert!(
        any_tainted,
        "ExternalSync row must surface for the engine's upgrade check"
    );
    assert!(
        any_internal,
        "user-driven row must keep its Internal label so mixed contexts don't over-escalate"
    );
}

// ── Same-session self-echo exclusion, via the ambient thread scope ────
//
// `Memory::recall` (backing the agent's `memory_recall` tool) reads the
// ambient chat-thread id set by `tinyagents::thread_context`
// around a live turn, and excludes documents tagged with that same id —
// guarding against the harness's own `user_msg:<uuid>` autosave being
// recalled as the top "relevant" result for the very request that
// triggered the search. See `agent::harness::session::turn::core`
// (autosave tagging) and `query::query_namespace_hits_excluding_session`
// (the exclusion mechanism).

#[tokio::test]
async fn recall_excludes_document_from_ambient_current_thread() {
    use crate::thread_context::with_thread_id;

    let (_tmp, mem) = fresh_mem();
    mem.store(
        "global",
        "user_msg:current-turn",
        "Please look up Jordan Rivera's chat platform user ID for me.",
        MemoryCategory::Conversation,
        Some("thread-current"),
    )
    .await
    .unwrap();
    mem.store(
        "global",
        "fact:jordan-rivera-platform-id",
        "Jordan Rivera's chat platform user ID is U0000042.",
        MemoryCategory::Conversation,
        Some("thread-other"),
    )
    .await
    .unwrap();

    let entries = with_thread_id("thread-current", async {
        mem.recall(
            "Jordan Rivera chat platform user ID",
            10,
            RecallOpts {
                namespace: Some("global"),
                min_score: Some(0.0),
                ..Default::default()
            },
        )
        .await
        .unwrap()
    })
    .await;

    assert!(
        !entries.iter().any(|e| e.key == "user_msg:current-turn"),
        "recall inside the ambient current-thread scope must exclude that thread's own \
         autosaved request, got {entries:#?}"
    );
    assert!(
        entries
            .iter()
            .any(|e| e.key == "fact:jordan-rivera-platform-id"),
        "an unrelated document from a different session must still be recalled, got {entries:#?}"
    );
}

#[tokio::test]
async fn recall_outside_any_thread_scope_is_unaffected() {
    let (_tmp, mem) = fresh_mem();
    mem.store(
        "global",
        "user_msg:current-turn",
        "Please look up Jordan Rivera's chat platform user ID for me.",
        MemoryCategory::Conversation,
        Some("thread-current"),
    )
    .await
    .unwrap();

    // No `with_thread_id(...)` scope active — mirrors cron, CLI,
    // standalone, and any pre-existing caller. `current_thread_id()`
    // returns `None`, so no exclusion applies and behavior is
    // byte-for-byte the same as before this fix.
    let entries = mem
        .recall(
            "Jordan Rivera chat platform user ID",
            10,
            RecallOpts {
                namespace: Some("global"),
                min_score: Some(0.0),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    assert!(
        entries.iter().any(|e| e.key == "user_msg:current-turn"),
        "with no ambient thread scope, recall must return the document exactly as before \
         this fix, got {entries:#?}"
    );
}

// ── The engine takes the exclusion as a parameter (H0, piece 1) ──────
//
// `recall_excluding_session` is the policy-free engine body: it must honour
// an exclusion handed to it with **no ambient turn scope active**, and
// apply none when handed `None`. Together these pin that the exclusion
// travels as an argument rather than being re-derived from a task-local
// inside the storage layer — the property that lets the engine move into a
// persistence crate without dragging the chat-turn concept along.

async fn seed_self_echo_fixture(mem: &UnifiedMemory) {
    mem.store(
        "global",
        "user_msg:current-turn",
        "Please look up Jordan Rivera's chat platform user ID for me.",
        MemoryCategory::Conversation,
        Some("thread-current"),
    )
    .await
    .unwrap();
    mem.store(
        "global",
        "fact:jordan-rivera-platform-id",
        "Jordan Rivera's chat platform user ID is U0000042.",
        MemoryCategory::Conversation,
        Some("thread-other"),
    )
    .await
    .unwrap();
}

fn self_echo_opts() -> RecallOpts<'static> {
    RecallOpts {
        namespace: Some("global"),
        min_score: Some(0.0),
        ..Default::default()
    }
}

#[tokio::test]
async fn recall_excluding_session_applies_an_explicit_exclusion_with_no_ambient_scope() {
    let (_tmp, mem) = fresh_mem();
    seed_self_echo_fixture(&mem).await;

    // Deliberately NOT wrapped in `with_thread_id`: if the engine were
    // still reading the ambient task-local rather than the argument, the
    // exclusion below would have no effect and the first assert fails.
    let entries = mem
        .recall_excluding_session(
            "Jordan Rivera chat platform user ID",
            10,
            self_echo_opts(),
            Some("thread-current"),
        )
        .await
        .unwrap();

    assert!(
        !entries.iter().any(|e| e.key == "user_msg:current-turn"),
        "an explicitly passed exclusion must drop that session's own document even with no \
         ambient turn scope, got {entries:#?}"
    );
    assert!(
        entries
            .iter()
            .any(|e| e.key == "fact:jordan-rivera-platform-id"),
        "a document from a different session must survive the exclusion, got {entries:#?}"
    );
}

#[tokio::test]
async fn recall_excluding_session_with_none_excludes_nothing() {
    let (_tmp, mem) = fresh_mem();
    seed_self_echo_fixture(&mem).await;

    // Inside an ambient turn scope, yet passed `None`: the engine must
    // honour the argument, not the task-local.
    let entries = crate::thread_context::with_thread_id("thread-current", async {
        mem.recall_excluding_session(
            "Jordan Rivera chat platform user ID",
            10,
            self_echo_opts(),
            None,
        )
        .await
        .unwrap()
    })
    .await;

    assert!(
        entries.iter().any(|e| e.key == "user_msg:current-turn"),
        "`None` must exclude nothing — the engine must not re-derive an exclusion from the \
         ambient turn scope, got {entries:#?}"
    );
}
