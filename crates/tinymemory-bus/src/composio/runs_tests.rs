//! Tests for the sync-run report vocabulary — the pure-data half.
//!
//! What is pinned here is what two separately compiled processes have to agree
//! on: the `snake_case` reason tags that end up in audit rows, and the
//! arithmetic on a report that a status panel renders without re-deriving.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use super::{ComposioUsage, ComposioUsageHandle, SyncOutcome, SyncReason};

#[test]
fn every_sync_reason_tag_matches_its_serde_form() {
    for reason in [
        SyncReason::ConnectionCreated,
        SyncReason::Periodic,
        SyncReason::Manual,
    ] {
        let json = serde_json::to_string(&reason).expect("serialize");
        assert_eq!(
            json,
            format!("\"{}\"", reason.as_str()),
            "as_str and the serde form disagree for {reason:?}"
        );
        let back: SyncReason = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, reason);
    }
}

#[test]
fn sync_reason_tags_are_the_stable_strings() {
    assert_eq!(SyncReason::ConnectionCreated.as_str(), "connection_created");
    assert_eq!(SyncReason::Periodic.as_str(), "periodic");
    assert_eq!(SyncReason::Manual.as_str(), "manual");
}

#[test]
fn elapsed_is_the_difference_between_the_two_stamps() {
    let outcome = SyncOutcome {
        started_at_ms: 1_000,
        finished_at_ms: 1_750,
        ..SyncOutcome::default()
    };
    assert_eq!(outcome.elapsed_ms(), 750);
}

#[test]
fn elapsed_saturates_when_the_clock_went_backwards() {
    // Two separate clock reads; an NTP step between them must not panic a
    // status panel over a cosmetic number.
    let outcome = SyncOutcome {
        started_at_ms: 2_000,
        finished_at_ms: 1_000,
        ..SyncOutcome::default()
    };
    assert_eq!(outcome.elapsed_ms(), 0);
}

#[test]
fn an_outcome_round_trips_with_its_open_details_object() {
    let outcome = SyncOutcome {
        toolkit: "gmail".into(),
        connection_id: Some("conn-1".into()),
        reason: SyncReason::Periodic.as_str().to_string(),
        items_ingested: 12,
        started_at_ms: 5,
        finished_at_ms: 9,
        summary: "12 messages".into(),
        details: serde_json::json!({ "pages": 3 }),
    };
    let json = serde_json::to_string(&outcome).expect("serialize");
    let back: SyncOutcome = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(back.toolkit, "gmail");
    assert_eq!(back.connection_id.as_deref(), Some("conn-1"));
    assert_eq!(back.reason, "periodic");
    assert_eq!(back.items_ingested, 12);
    assert_eq!(back.elapsed_ms(), 4);
    assert_eq!(back.summary, "12 messages");
    assert_eq!(back.details["pages"], 3);
}

#[test]
fn an_outcome_decodes_when_details_is_absent() {
    // `details` is `#[serde(default)]`, so an older peer that never wrote the
    // field still decodes rather than failing the whole frame.
    let back: SyncOutcome = serde_json::from_str(
        r#"{"toolkit":"slack","connection_id":null,"reason":"manual",
            "items_ingested":0,"started_at_ms":0,"finished_at_ms":0,"summary":""}"#,
    )
    .expect("deserialize without details");
    assert!(back.details.is_null());
}

#[test]
fn cloning_a_usage_handle_shares_one_tally() {
    let handle = ComposioUsageHandle::default();
    let clone = handle.clone();
    {
        let mut usage = clone.lock().expect("usage lock");
        usage.actions_called += 2;
        usage.cost_usd += 0.5;
    }
    let usage = handle.lock().expect("usage lock");
    assert_eq!(usage.actions_called, 2);
    assert_eq!(usage.cost_usd, 0.5);
}

#[test]
fn a_usage_tally_starts_at_zero() {
    let usage = ComposioUsage::default();
    assert_eq!(usage.actions_called, 0);
    assert_eq!(usage.cost_usd, 0.0);
}
