//! Provenance, end to end through the public surface.
//!
//! `MemoryTaint` decides whether downstream policy treats content as something
//! the user authored or as something that arrived from outside. A driver that
//! loses it does not fail loudly — it silently reclassifies external content as
//! internal-trust, and every gate keyed on taint is then wrong about everything
//! that passed through.
//!
//! # Scope note
//!
//! Issue #18 §E3 describes this file as asserting that "external content stored
//! through the **sync path** arrives with `ExternalSync` at every engine". The
//! sync layer is welded to the engine today (§1.4) and its rewrite onto the
//! memory API is §B, so there is no engine-neutral sync path to drive yet.
//!
//! What is assertable now is the seam sync will hand to: taint through store,
//! read-back, list, recall, and the export/import round trip. When §B lands,
//! the sync leg is added here rather than in a new file.

// A failing assertion in a test *is* a panic; the crate-wide `expect_used` /
// `unwrap_used` / `panic` lints exist to keep the library from panicking, not
// the tests. Same allowance, and same reasoning, as `src/registry/test.rs`.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use tinymemory::api::null::NullMemoryProvider;
use tinymemory::api::provider::{MemoryCore, MemoryPortability, MemoryProvider, MemoryRecall};
use tinymemory::types::{MemoryCategory, MemoryTaint};
use tinymemory_conformance::InMemoryProvider;

const NS: &str = "taint-e2e";

/// Every driver this workspace ships, so the assertion is "at every engine"
/// rather than "at the one we happened to test".
fn drivers() -> Vec<Arc<dyn MemoryProvider>> {
    vec![
        Arc::new(InMemoryProvider::new()),
        Arc::new(NullMemoryProvider::new()),
    ]
}

#[tokio::test]
async fn external_content_reads_back_as_external_at_every_driver() {
    for provider in drivers() {
        let who = provider.driver_id();
        provider
            .store(
                NS,
                "from-the-web",
                "scraped from a page",
                MemoryCategory::Conversation,
                None,
                MemoryTaint::ExternalSync,
            )
            .await
            .unwrap_or_else(|e| panic!("{who}: store failed: {e}"));

        // A driver that retains nothing has nothing to reclassify; one that
        // retains must hand back what it was given.
        if let Some(entry) = provider.get(NS, "from-the-web").await.unwrap_or(None) {
            assert_eq!(
                entry.taint,
                MemoryTaint::ExternalSync,
                "{who}: external content was laundered into internal-trust content"
            );
        }
        let _ = provider.forget(NS, "from-the-web").await;
    }
}

#[tokio::test]
async fn internal_content_is_not_marked_external_by_accident() {
    // The inverse error is just as bad in the other direction: over-marking
    // makes the gate refuse the company's own material.
    for provider in drivers() {
        let who = provider.driver_id();
        provider
            .store(
                NS,
                "our-own",
                "we decided this",
                MemoryCategory::Core,
                None,
                MemoryTaint::Internal,
            )
            .await
            .unwrap_or_else(|e| panic!("{who}: store failed: {e}"));
        if let Some(entry) = provider.get(NS, "our-own").await.unwrap_or(None) {
            assert_eq!(
                entry.taint,
                MemoryTaint::Internal,
                "{who}: internal content was over-marked"
            );
        }
        let _ = provider.forget(NS, "our-own").await;
    }
}

#[tokio::test]
async fn taint_survives_list_and_recall_not_just_get() {
    // `get` is the easy path. A driver that rebuilds entries on the list and
    // recall paths can drop provenance on exactly those, which is where a
    // policy gate actually reads it.
    let provider = InMemoryProvider::new();
    provider
        .store(
            NS,
            "k",
            "needle from outside",
            MemoryCategory::Core,
            None,
            MemoryTaint::ExternalSync,
        )
        .await
        .expect("store");

    let listed = provider.list(Some(NS), None, None).await.expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0].taint,
        MemoryTaint::ExternalSync,
        "list dropped provenance"
    );

    let opts = tinymemory::recall::OwnedRecallOpts {
        namespace: Some(NS.to_string()),
        ..Default::default()
    };
    let hits = provider
        .recall("needle", 10, &opts, None)
        .await
        .expect("recall");
    assert_eq!(hits.len(), 1);
    assert_eq!(
        hits[0].taint,
        MemoryTaint::ExternalSync,
        "recall dropped provenance"
    );
}

#[tokio::test]
async fn taint_survives_export_and_re_import() {
    // The migration case. An export that drops taint, or an import that
    // re-stamps it, turns every restored external record into internal-trust
    // content — and a restore is exactly when nobody is watching.
    let provider = InMemoryProvider::new();
    provider
        .store(
            NS,
            "moved",
            "carried across",
            MemoryCategory::Core,
            None,
            MemoryTaint::ExternalSync,
        )
        .await
        .expect("store");

    let page = provider.export_page(None, 64).await.expect("export");
    let record = page
        .records
        .iter()
        .find(|r| r.namespace.as_deref() == Some(NS))
        .expect("the stored record was exported");
    assert_eq!(
        record.taint,
        MemoryTaint::ExternalSync,
        "export dropped provenance"
    );

    let fresh = InMemoryProvider::new();
    let outcome = fresh
        .import_records(vec![record.clone()])
        .await
        .expect("import");
    assert_eq!(outcome.imported, 1);
    assert_eq!(outcome.failed, 0, "{:?}", outcome.errors);

    let restored = fresh
        .get(NS, "moved")
        .await
        .expect("get")
        .expect("restored");
    assert_eq!(
        restored.taint,
        MemoryTaint::ExternalSync,
        "import re-stamped provenance instead of persisting what it was given"
    );
}

#[test]
fn unknown_persisted_taint_values_fail_closed() {
    // A corrupt or future column value must read as the *more* restrictive
    // state. Failing open here would let an unrecognised row be treated as
    // user-authored, which is the one direction that cannot be undone.
    assert_eq!(MemoryTaint::from_db_str(""), MemoryTaint::ExternalSync);
    assert_eq!(
        MemoryTaint::from_db_str("future-value"),
        MemoryTaint::ExternalSync
    );
    assert_eq!(
        MemoryTaint::from_db_str("INTERNAL"),
        MemoryTaint::ExternalSync
    );
    // Only the exact known spelling reads as internal.
    assert_eq!(MemoryTaint::from_db_str("internal"), MemoryTaint::Internal);
}
