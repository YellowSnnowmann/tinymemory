//! Tests for the source-sync value types.
//!
//! Two things a later slice can silently break: the cost rule
//! ([`SyncAuditEntry::effective_cost_usd`] must prefer the real charge and must
//! always add Composio's), and the serde defaults an older peer's payload
//! relies on — every one of those fields is a `#[serde(default)]` precisely so
//! a module and a host built a release apart still decode each other.

// A failed assertion in a test is a panic either way; `unwrap`/`expect` here say
// what the invariant was. Same allowance the crate's other test modules take.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use super::*;

fn entry() -> SyncAuditEntry {
    SyncAuditEntry {
        timestamp: DateTime::<Utc>::from_timestamp(1_700_000_000, 0).expect("valid timestamp"),
        source_id: "composio:gmail:conn-1".to_string(),
        source_kind: "composio".to_string(),
        scope: "gmail:conn-1".to_string(),
        items_fetched: 12,
        batches: 2,
        input_tokens: 1_000_000,
        output_tokens: 1_000_000,
        estimated_cost_usd: 0.35,
        composio_actions_called: 4,
        composio_cost_usd: 0.02,
        actual_charged_usd: None,
        duration_ms: 4_200,
        success: true,
        error: None,
        tree_ingest_failures: 0,
        tree_error: None,
    }
}

#[test]
fn effective_cost_falls_back_to_the_estimate_and_always_adds_composio() {
    let entry = entry();
    // No reported charge: the estimate stands, plus the provider's own cost.
    assert!((entry.effective_cost_usd() - 0.37).abs() < 1e-9);
}

#[test]
fn effective_cost_prefers_the_real_charge_over_the_estimate() {
    let mut entry = entry();
    entry.actual_charged_usd = Some(0.10);
    // The estimate is superseded, not averaged with, and Composio's cost is
    // still additive — it is billed by a different party.
    assert!((entry.effective_cost_usd() - 0.12).abs() < 1e-9);
}

#[test]
fn a_reported_charge_of_zero_is_not_an_absent_one() {
    // `Some(0.0)` is "the provider billed nothing"; `None` is "the provider said
    // nothing". Collapsing them would price a free run at the estimate.
    let mut entry = entry();
    entry.actual_charged_usd = Some(0.0);
    assert!((entry.effective_cost_usd() - 0.02).abs() < 1e-9);
}

#[test]
fn an_audit_row_from_an_older_writer_still_decodes() {
    // The four `#[serde(default)]` fields were added after the log format
    // existed, and the file is append-only across releases: a row written
    // before they existed must still read.
    let raw = serde_json::json!({
        "timestamp": "2023-11-14T22:13:20Z",
        "source_id": "folder:notes",
        "source_kind": "folder",
        "scope": "folder:notes",
        "items_fetched": 3,
        "batches": 1,
        "input_tokens": 10,
        "output_tokens": 5,
        "estimated_cost_usd": 0.5,
        "duration_ms": 10,
        "success": true,
    });
    let entry: SyncAuditEntry = serde_json::from_value(raw).expect("decode a pre-default row");
    assert_eq!(entry.composio_actions_called, 0);
    assert!((entry.composio_cost_usd - 0.0).abs() < 1e-9);
    assert_eq!(entry.actual_charged_usd, None);
    assert!((entry.effective_cost_usd() - 0.5).abs() < 1e-9);
}

#[test]
fn a_sync_run_outcome_from_an_older_module_decodes_to_no_usage() {
    // `actions_called`, `provider_cost_usd` and `note` all default: a module
    // that predates them reports a run without them, and the caller must read
    // that as "no usage recorded", not fail the call.
    let outcome: SyncRunOutcome =
        serde_json::from_value(serde_json::json!({ "records_ingested": 7, "more_pending": true }))
            .expect("decode a minimal outcome");
    assert_eq!(outcome.records_ingested, 7);
    assert!(outcome.more_pending);
    assert_eq!(outcome.actions_called, 0);
    assert_eq!(outcome.note, None);
}

#[test]
fn freshness_wire_strings_are_snake_case() {
    // Rendered as a badge by name, so these strings are the contract.
    for (freshness, expected) in [
        (SyncFreshness::Active, "active"),
        (SyncFreshness::Recent, "recent"),
        (SyncFreshness::Idle, "idle"),
    ] {
        assert_eq!(
            serde_json::to_value(freshness).expect("serialize freshness"),
            serde_json::Value::String(expected.to_string())
        );
    }
}

#[test]
fn coverage_counts_pending_and_names_nothing() {
    // The reduction is deliberate and is asserted rather than left to the docs:
    // a path inside the driver's content vault must not appear on the wire.
    let coverage = RawArchiveCoverage {
        total: 10,
        covered: 7,
        pending: 3,
    };
    let encoded = serde_json::to_value(coverage).expect("serialize coverage");
    assert_eq!(encoded["pending"], serde_json::json!(3));
    assert!(
        encoded.as_object().is_some_and(|map| map.len() == 3),
        "coverage carries exactly total/covered/pending: {encoded}"
    );
}

#[test]
fn sync_state_omits_the_absent_optionals_rather_than_nulling_them() {
    // The status row is rendered straight from this shape, and a `null` cursor
    // and an omitted one are the same fact; emitting both spellings over the
    // life of one connection makes a caller handle two.
    let state = SourceSyncState {
        toolkit: "slack".to_string(),
        connection_id: "conn-1".to_string(),
        daily_request_limit: 500,
        ..SourceSyncState::default()
    };
    let encoded = serde_json::to_value(&state).expect("serialize sync state");
    assert!(encoded.get("cursor").is_none());
    assert!(encoded.get("last_seen_id").is_none());
    assert!(encoded.get("last_sync_at_ms").is_none());
    assert_eq!(encoded["daily_requests_used"], serde_json::json!(0));
}
