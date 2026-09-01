//! Tests for the chunk-family value types.
//!
//! What is worth pinning here is not the shape of a struct — the compiler has
//! that — but the two decisions a later slice could silently reverse: that a
//! score row's diagnostic fields survive a round trip through a peer that does
//! not know them, and that the ingest-status row can represent a source with
//! nothing in it. Both failures render as a plausible screen rather than as an
//! error.

// A failed assertion in a test is a panic either way; `expect` here says what
// the invariant was. Only the lint this file actually trips is allowed.
#![allow(clippy::expect_used)]

use super::*;

#[test]
fn the_drop_threshold_is_the_engines_own_number() {
    // Pinned as a literal rather than derived, because the point of carrying it
    // is that a caller and the scorer draw the same line. A change here is a
    // change to what every rendered gauge means, and it should have to be
    // typed.
    assert!((DEFAULT_DROP_THRESHOLD - 0.3).abs() < f32::EPSILON);
}

#[test]
fn a_score_row_round_trips_every_field() {
    // The row is carried whole rather than narrowed to what today's caller
    // reads, so the test is the whole row: a field quietly dropped from the
    // wire would still decode, as its default, and read as a store that
    // recorded nothing rather than as a type that forgot to ask.
    let score = ChunkScore {
        chunk_id: "chunk-1".to_string(),
        total: 0.42,
        signals: ChunkScoreSignals {
            token_count: 0.1,
            unique_words: 0.2,
            metadata_weight: 0.3,
            source_weight: 0.4,
            interaction: 0.5,
            entity_density: 0.6,
            llm_importance: 0.7,
        },
        dropped: true,
        reason: Some("below the admission threshold".to_string()),
        computed_at_ms: 1_700_000_000_000,
        llm_importance_reason: Some("boilerplate footer".to_string()),
    };
    let round_tripped: ChunkScore =
        serde_json::from_value(serde_json::to_value(&score).expect("serialize score"))
            .expect("decode score");
    assert_eq!(round_tripped, score);
}

#[test]
fn a_score_row_from_a_store_that_keeps_no_llm_signal_still_decodes() {
    // The engine's score table has no column for either LLM field, so a real
    // row omits both. They are on the type for a driver that does keep them,
    // which is only safe if their absence is not a decode failure.
    let score: ChunkScore = serde_json::from_value(serde_json::json!({
        "chunk_id": "chunk-1",
        "total": 0.9,
        "signals": { "token_count": 0.5 },
        "dropped": false,
        "computed_at_ms": 1_700_000_000_000_i64,
    }))
    .expect("decode a row with no LLM signal");
    assert_eq!(score.llm_importance_reason, None);
    assert!(score.signals.llm_importance.abs() < f32::EPSILON);
    assert!((score.signals.token_count - 0.5).abs() < f32::EPSILON);
    assert_eq!(score.reason, None);
}

#[test]
fn an_ingest_status_can_say_a_source_has_never_synced() {
    // The gap this type exists for. `SourceTotal` cannot express it — a group
    // with no rows is not a zero row, it is an absent one — and a dashboard
    // built on that loses the source instead of showing it idle.
    let never_synced = SourceIngestStatus {
        source_id: "src_new".to_string(),
        chunks_synced: 0,
        chunks_pending: 0,
        last_chunk_at_ms: None,
    };
    let encoded = serde_json::to_value(&never_synced).expect("serialize status");
    assert!(
        encoded.get("last_chunk_at_ms").is_none(),
        "a never-synced source omits the timestamp rather than sending a zero one"
    );
    let round_tripped: SourceIngestStatus = serde_json::from_value(encoded).expect("decode status");
    assert_eq!(round_tripped, never_synced);
}

#[test]
fn the_two_source_identifiers_are_kept_apart() {
    // The registry id and the chunk key are different strings for a connector
    // source — the chunk key does not contain the registry id at all — so a
    // type that carried one field would force the caller to send whichever the
    // other end did not want.
    let query = SourceIngestQuery {
        source_id: "src_gmail_work".to_string(),
        chunk_id_prefix: "gmail:conn-1:".to_string(),
    };
    let round_tripped: SourceIngestQuery =
        serde_json::from_value(serde_json::to_value(&query).expect("serialize query"))
            .expect("decode query");
    assert_eq!(round_tripped, query);
    assert_ne!(round_tripped.source_id, round_tripped.chunk_id_prefix);
}
