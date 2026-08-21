//! Tests for the surrounding module.

use super::*;

#[test]
fn toolkit_slug_is_stable() {
    assert_eq!(SlackProvider::new().toolkit_slug(), "slack");
}

#[test]
fn sync_interval_matches_constant() {
    assert_eq!(
        SlackProvider::new().sync_interval_secs(),
        Some(SYNC_INTERVAL_SECS)
    );
}

#[test]
fn curated_tools_returns_slack_catalog() {
    let tools = SlackProvider::new().curated_tools().unwrap();
    assert!(tools
        .iter()
        .any(|t| t.slug == "SLACK_FETCH_CONVERSATION_HISTORY"));
    assert!(tools.iter().any(|t| t.slug == "SLACK_LIST_CONVERSATIONS"));
}

#[test]
fn post_process_action_result_delegates_to_post_process_module() {
    let provider = SlackProvider::new();
    let mut data = serde_json::json!({
        "channels": [{"id": "C1", "name": "eng", "is_private": false}]
    });
    // Calling with an unknown slug should be a no-op.
    provider.post_process_action_result("SLACK_UNKNOWN_ACTION", None, &mut data);
    assert!(
        data.get("channels").is_some(),
        "no-op slug must not mutate data"
    );
}
