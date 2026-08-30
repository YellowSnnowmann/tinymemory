//! Tests for deterministic fast retrieval and explicit source gating.

use std::collections::HashSet;

use tempfile::TempDir;
use tinymemory_api::host::test_support::TestHostConfig;

use super::{fast_retrieve, fast_retrieve_scoped, FastRetrieveOptions};

fn config() -> (TempDir, TestHostConfig) {
    crate::test_seams::init();
    let tmp = TempDir::new().unwrap();
    let mut config = TestHostConfig::default();
    config.workspace_dir = tmp.path().join("workspace");
    config.embeddings_provider = Some("none".into());
    (tmp, config)
}

#[tokio::test]
async fn empty_store_returns_well_formed_empty_responses_for_every_scope() {
    let (_tmp, config) = config();
    let options = FastRetrieveOptions {
        limit: 5,
        max_hops: 2,
        ..Default::default()
    };
    let unrestricted = fast_retrieve(&config, "missing subject", options.clone())
        .await
        .unwrap();
    assert!(unrestricted.hits.is_empty());
    assert_eq!(unrestricted.total, 0);
    assert!(!unrestricted.truncated);

    let denied = fast_retrieve_scoped(&config, "missing subject", options, Some(HashSet::new()))
        .await
        .unwrap();
    assert!(denied.hits.is_empty());
    assert_eq!(denied.total, 0);
}

#[tokio::test]
async fn ambient_empty_source_scope_remains_fail_closed() {
    let (_tmp, config) = config();
    let response = crate::source_scope::with_source_scope(
        Some(Vec::new()),
        fast_retrieve(&config, "anything", FastRetrieveOptions::default()),
    )
    .await
    .unwrap();
    assert!(response.hits.is_empty());
}

/// The contract's `FastRetrieveQuery::default()` must stay the engine's
/// `FastRetrieveOptions::default()`.
///
/// The contract grew a `Default` so a host migrating off the engine type does
/// not have to re-spell `limit: 10, max_hops: 2` at every call site
/// (OpenHuman#5560) — which is exactly how two defaults drift apart. Neither
/// crate can see the other's constant, so this is the only place the two can
/// be compared. A change to either side without the other lands here.
#[test]
fn the_contract_default_matches_the_engine_default() {
    let engine = FastRetrieveOptions::default();
    let contract = tinymemory_api::provider::retrieval::FastRetrieveQuery::default();
    assert_eq!(engine.limit, contract.limit, "default limit drifted");
    assert_eq!(engine.max_hops, contract.max_hops, "default max_hops drifted");
    assert_eq!(
        engine.time_window_days, contract.time_window_days,
        "default time_window_days drifted"
    );
}
