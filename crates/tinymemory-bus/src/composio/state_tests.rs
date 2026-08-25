//! Tests for the persisted sync-state shape.
//!
//! The load/save round-trip through a key/value store is tested in the engine
//! crate, next to the `SyncStateStore` trait that performs it. What is pinned
//! here is what a migration would have to care about: the namespace, the
//! serialised field set, and the day-rollover arithmetic that decides whether a
//! connection is allowed to make another request.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use super::{
    extract_item_id, DailyBudget, SyncState, DEFAULT_DAILY_REQUEST_LIMIT, KV_NAMESPACE,
    STATE_NAMESPACE,
};

/// The namespace is durable: every persisted Composio sync cursor lives under
/// this string, so a change strands all of them. The engine's own copy of this
/// type must agree — failing here means a coordinated migration, never a local
/// edit.
#[test]
fn the_state_namespace_is_pinned() {
    assert_eq!(
        STATE_NAMESPACE, "composio-sync-state",
        "the Composio sync-state namespace changed; every persisted cursor is \
         stored under the old value and needs migrating"
    );
    assert_eq!(KV_NAMESPACE, STATE_NAMESPACE);
}

/// The serialised shape is persisted and is also what the engine's copy writes.
/// Pinned so the two cannot drift silently.
#[test]
fn the_state_wire_shape_is_pinned() {
    let mut state = SyncState::new("gmail", "conn-1");
    state.advance_cursor("c2");
    state.mark_synced("m1");
    state.item_versions.insert("m1".into(), "v1".into());
    state.set_last_seen_id("m1");
    state.set_last_sync_at_ms(1_000);
    // Written directly rather than through `record_requests`, which would roll
    // the stale date forward to today and make the expectation clock-dependent.
    state.daily_budget.date = "2026-01-02".into();
    state.daily_budget.requests_used = 3;

    let value = serde_json::to_value(&state).expect("serialize");
    assert_eq!(
        value,
        serde_json::json!({
            "toolkit": "gmail",
            "connection_id": "conn-1",
            "cursor": "c2",
            "synced_ids": ["m1"],
            "item_versions": {"m1": "v1"},
            "daily_budget": {"date": "2026-01-02", "requests_used": 3, "limit": 500},
            "last_seen_id": "m1",
            "last_sync_at_ms": 1000
        })
    );
}

#[test]
fn per_run_counters_never_reach_the_wire() {
    // A persisted tally would make every subsequent run report the sum of all
    // the runs before it, which is not what the audit log reads it as.
    let mut state = SyncState::new("gmail", "conn-1");
    state.record_action(2, 0.25);
    assert_eq!(state.run_requests, 2);
    assert_eq!(state.run_provider_cost_usd, 0.25);

    let value = serde_json::to_value(&state).expect("serialize");
    let object = value.as_object().expect("state serialises as an object");
    assert!(!object.contains_key("run_requests"));
    assert!(!object.contains_key("run_provider_cost_usd"));
}

#[test]
fn the_key_is_toolkit_then_connection() {
    assert_eq!(SyncState::key("gmail", "conn-1"), "gmail:conn-1");
}

#[test]
fn a_fresh_state_has_synced_nothing_and_spent_nothing() {
    let state = SyncState::new("slack", "conn-2");
    assert!(state.cursor.is_none());
    assert!(!state.is_synced("anything"));
    assert!(!state.budget_exhausted());
    assert_eq!(state.budget_remaining(), DEFAULT_DAILY_REQUEST_LIMIT);
    assert_eq!(state.run_requests, 0);
}

#[test]
fn a_state_missing_every_optional_field_still_decodes() {
    // Only `toolkit` and `connection_id` are required; a row written by an
    // older peer that never knew about `item_versions` must load rather than
    // fail the whole connection.
    let stored = serde_json::json!({ "toolkit": "gmail", "connection_id": "c" });
    let state: SyncState = serde_json::from_value(stored).expect("deserialize");
    assert!(state.cursor.is_none());
    assert!(state.synced_ids.is_empty());
    assert!(state.item_versions.is_empty());
    assert_eq!(state.daily_budget.limit, DEFAULT_DAILY_REQUEST_LIMIT);
}

#[test]
fn a_stale_budget_reports_full_and_resets_on_the_next_charge() {
    let mut budget = DailyBudget {
        date: "2000-01-01".into(),
        requests_used: 499,
        limit: 500,
    };
    assert_eq!(budget.remaining(), 500);
    budget.record_requests(1);
    assert_eq!(budget.requests_used, 1);
    assert_eq!(budget.remaining(), 499);
}

#[test]
fn an_exhausted_budget_reports_zero_remaining() {
    let mut budget = DailyBudget {
        limit: 2,
        ..DailyBudget::default()
    };
    budget.record_request();
    assert!(!budget.is_exhausted());
    budget.record_request();
    assert!(budget.is_exhausted());
    assert_eq!(budget.remaining(), 0);
}

#[test]
fn charging_past_the_limit_saturates_rather_than_wrapping() {
    let mut budget = DailyBudget {
        limit: 1,
        ..DailyBudget::default()
    };
    budget.record_requests(u32::MAX);
    budget.record_requests(10);
    assert_eq!(budget.requests_used, u32::MAX);
    assert_eq!(budget.remaining(), 0);
}

#[test]
fn an_action_always_costs_at_least_one_request() {
    let mut state = SyncState::new("gmail", "conn-1");
    state.record_action(0, 0.0);
    assert_eq!(state.run_requests, 1);
    assert_eq!(state.daily_budget.requests_used, 1);
}

#[test]
fn a_nonsense_action_cost_is_discarded_rather_than_totalled() {
    let mut state = SyncState::new("gmail", "conn-1");
    state.record_action(1, f64::NAN);
    state.record_action(1, f64::INFINITY);
    state.record_action(1, -5.0);
    assert_eq!(state.run_provider_cost_usd, 0.0);
    state.record_action(1, 0.5);
    assert_eq!(state.run_provider_cost_usd, 0.5);
}

#[test]
fn an_item_id_is_taken_from_the_first_populated_path() {
    let item = serde_json::json!({ "data": { "id": "inner" }, "messageId": "outer" });
    assert_eq!(
        extract_item_id(&item, &["missing", "data.id", "messageId"]).as_deref(),
        Some("inner")
    );
}

#[test]
fn a_blank_item_id_counts_as_absent() {
    // An id of `"  "` dedupes nothing and would poison the synced set, so it
    // must not win over a later path that is actually populated.
    let item = serde_json::json!({ "id": "   ", "messageId": "m-1" });
    assert_eq!(
        extract_item_id(&item, &["id", "messageId"]).as_deref(),
        Some("m-1")
    );
}

#[test]
fn a_non_string_item_id_is_skipped() {
    let item = serde_json::json!({ "id": 7, "messageId": "m-1" });
    assert_eq!(
        extract_item_id(&item, &["id", "messageId"]).as_deref(),
        Some("m-1")
    );
}

#[test]
fn no_matching_path_yields_none() {
    let item = serde_json::json!({ "id": "x" });
    assert_eq!(extract_item_id(&item, &["nope", "also.nope"]), None);
}
