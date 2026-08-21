//! Tests for the surrounding module.

use super::*;

/// The two GitHub coordinate helpers are re-exported from `tinymemory-sources` and
/// they deliberately differ: `tree_scope` slugifies to
/// `github-tinyhumansai-openhuman` while `archive_source_id` slugifies to
/// `github-com-tinyhumansai-openhuman`. Swapping the two still compiles and
/// still type-checks — it just makes reconcile scan an empty directory at
/// runtime. Pin both spellings.
#[test]
fn derive_scopes_keeps_github_tree_and_archive_ids_distinct() {
    let source: MemorySourceEntry = serde_json::from_value(serde_json::json!({
        "id": "gh-scope",
        "kind": "github_repo",
        "label": "Repo",
        "url": "https://github.com/tinyhumansai/openhuman",
    }))
    .expect("github source entry");

    let scopes = derive_scopes(&source, &TestHostConfig::default());

    assert_eq!(scopes.len(), 1);
    assert_eq!(scopes[0].tree_scope, "github:tinyhumansai/openhuman");
    assert_eq!(
        scopes[0].archive_source_id,
        "github.com/tinyhumansai/openhuman"
    );
}
