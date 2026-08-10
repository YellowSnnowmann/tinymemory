//! Conversion tests.
//!
//! Two contracts that are allowed to drift will drift. The exhaustive
//! destructuring in `convert.rs` catches a *new* field at compile time; these
//! tests catch a *mis-mapped* one, which the compiler cannot see because every
//! field on both sides has the same type.

#![allow(clippy::expect_used, clippy::panic)]

use super::*;

/// Every category must survive a round trip, including the custom variant's
/// payload — a `Custom(String)` mapped onto the wrong arm would silently
/// re-file every custom-categorised memory.
#[test]
fn every_category_round_trips() {
    let cases = [
        tm::MemoryCategory::Core,
        tm::MemoryCategory::Daily,
        tm::MemoryCategory::Conversation,
        tm::MemoryCategory::Custom("project-notes".to_string()),
    ];
    for category in cases {
        let round_tripped = category_to_tinymemory(category_to_tinycortex(category.clone()));
        assert_eq!(round_tripped, category);
    }
}

/// The wire form is the persisted form on both sides, so a conversion that
/// round-trips the Rust value but changes the string would still corrupt a
/// store. Comparing the rendered forms catches that.
#[test]
fn category_conversion_preserves_the_persisted_spelling() {
    let cases = [
        tm::MemoryCategory::Core,
        tm::MemoryCategory::Daily,
        tm::MemoryCategory::Conversation,
        tm::MemoryCategory::Custom("x".to_string()),
    ];
    for category in cases {
        let engine = category_to_tinycortex(category.clone());
        assert_eq!(
            engine.to_string(),
            category.to_string(),
            "the two contracts must agree on the persisted spelling"
        );
    }
}

/// Provenance is the security-relevant conversion: mapping `ExternalSync` onto
/// `Internal` would upgrade the trust of externally-sourced content.
#[test]
fn every_taint_round_trips_and_keeps_its_db_spelling() {
    for taint in [tm::MemoryTaint::Internal, tm::MemoryTaint::ExternalSync] {
        let engine = taint_to_tinycortex(taint);
        assert_eq!(taint_to_tinymemory(engine), taint);
        assert_eq!(
            engine.as_db_str(),
            taint.as_db_str(),
            "the two contracts must agree on the persisted spelling"
        );
    }
}

/// `ExternalSync` must never come back as `Internal`, stated as its own
/// assertion rather than left implicit in the round trip above.
#[test]
fn external_content_is_never_laundered_into_internal_trust() {
    assert_eq!(
        taint_to_tinymemory(taint_to_tinycortex(tm::MemoryTaint::ExternalSync)),
        tm::MemoryTaint::ExternalSync
    );
    assert_ne!(
        taint_to_tinymemory(taint_to_tinycortex(tm::MemoryTaint::ExternalSync)),
        tm::MemoryTaint::Internal
    );
}

#[test]
fn an_entry_round_trips_every_field() {
    let engine = tc::MemoryEntry {
        id: "ns/key".to_string(),
        key: "key".to_string(),
        content: "body".to_string(),
        namespace: Some("ns".to_string()),
        category: tc::MemoryCategory::Custom("notes".to_string()),
        timestamp: "2026-08-10T00:00:00Z".to_string(),
        session_id: Some("s1".to_string()),
        score: Some(0.75),
        taint: tc::MemoryTaint::ExternalSync,
    };

    let converted = entry_to_tinymemory(engine.clone());

    assert_eq!(converted.id, engine.id);
    assert_eq!(converted.key, engine.key);
    assert_eq!(converted.content, engine.content);
    assert_eq!(converted.namespace, engine.namespace);
    assert_eq!(converted.category.to_string(), engine.category.to_string());
    assert_eq!(converted.timestamp, engine.timestamp);
    assert_eq!(converted.session_id, engine.session_id);
    assert_eq!(converted.score, engine.score);
    assert_eq!(converted.taint, tm::MemoryTaint::ExternalSync);
}

/// A score of `None` must stay `None`. Defaulting it to `0.0` would make an
/// unranked entry look like a worst-ranked one.
#[test]
fn an_absent_score_stays_absent() {
    let engine = tc::MemoryEntry {
        id: "i".to_string(),
        key: "k".to_string(),
        content: "c".to_string(),
        namespace: None,
        category: tc::MemoryCategory::Core,
        timestamp: "t".to_string(),
        session_id: None,
        score: None,
        taint: tc::MemoryTaint::Internal,
    };
    let converted = entry_to_tinymemory(engine);
    assert!(converted.score.is_none());
    assert!(converted.namespace.is_none());
    assert!(converted.session_id.is_none());
}

#[test]
fn a_namespace_summary_round_trips_every_field() {
    let engine = tc::NamespaceSummary {
        namespace: "projects".to_string(),
        count: 12,
        last_updated: Some("2026-08-10T00:00:00Z".to_string()),
    };
    let converted = namespace_summary_to_tinymemory(engine.clone());
    assert_eq!(converted.namespace, engine.namespace);
    assert_eq!(converted.count, engine.count);
    assert_eq!(converted.last_updated, engine.last_updated);
}

/// Every recall filter must cross. A dropped `min_score` or `cross_session`
/// silently widens a query.
#[test]
fn every_recall_filter_crosses() {
    let opts = tm::OwnedRecallOpts {
        namespace: Some("ns".to_string()),
        category: Some(tm::MemoryCategory::Daily),
        session_id: Some("s1".to_string()),
        min_score: Some(0.5),
        cross_session: true,
    };
    let engine = recall_opts_to_tinycortex(&opts);
    assert_eq!(engine.namespace, opts.namespace);
    assert_eq!(
        engine.category.as_ref().map(ToString::to_string),
        opts.category.as_ref().map(ToString::to_string)
    );
    assert_eq!(engine.session_id, opts.session_id);
    assert_eq!(engine.min_score, opts.min_score);
    assert_eq!(engine.cross_session, opts.cross_session);
}

#[test]
fn empty_recall_filters_stay_empty() {
    let engine = recall_opts_to_tinycortex(&tm::OwnedRecallOpts::default());
    assert!(engine.namespace.is_none());
    assert!(engine.category.is_none());
    assert!(engine.session_id.is_none());
    assert!(engine.min_score.is_none());
    assert!(!engine.cross_session);
}
