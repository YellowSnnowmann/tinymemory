//! Tests for source defaults and fail-closed host-registry decoding.

use super::*;
use serde_json::json;
use tinymemory_api::host::test_support::TestHostConfig;

fn entry(kind: SourceKind) -> MemorySourceEntry {
    serde_json::from_value(json!({
        "id": "source-1",
        "kind": kind,
        "label": "Source",
        "enabled": true
    }))
    .unwrap()
}

#[test]
fn kind_defaults_fill_only_missing_limits() {
    let mut github = entry(SourceKind::GithubRepo);
    github.max_issues = Some(3);
    apply_kind_defaults(&mut github);
    assert_eq!(github.max_prs, Some(10));
    assert_eq!(github.max_issues, Some(3));
    assert_eq!(github.max_commits, Some(50));

    let mut rss = entry(SourceKind::RssFeed);
    apply_kind_defaults(&mut rss);
    assert_eq!(rss.max_items, Some(20));
    let mut twitter = entry(SourceKind::TwitterQuery);
    apply_kind_defaults(&mut twitter);
    assert_eq!(twitter.since_days, Some(7));
    twitter.since_days = Some(2);
    apply_kind_defaults(&mut twitter);
    assert_eq!(twitter.since_days, Some(2));

    let mut folder = entry(SourceKind::Folder);
    apply_kind_defaults(&mut folder);
    assert!(folder.max_items.is_none());
}

#[test]
fn decode_memory_sources_accepts_valid_rows_and_rejects_bad_shapes() {
    let valid = entry(SourceKind::WebPage);
    let mut config = TestHostConfig::default();
    config.memory_sources = Some(serde_json::to_value([valid]).unwrap());
    let decoded = decode_memory_sources(&config);
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].id, "source-1");

    let mut malformed = TestHostConfig::default();
    malformed.memory_sources = Some(json!({"not": "an array"}));
    assert!(decode_memory_sources(&malformed).is_empty());
    assert!(decode_memory_sources(&TestHostConfig::default()).is_empty());
}
