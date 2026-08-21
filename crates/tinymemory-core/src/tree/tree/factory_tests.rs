//! Tests for the surrounding module.

use super::*;

#[test]
fn source_factory_uses_source_kind_and_full_scope() {
    let f = TreeFactory::source("slack:#eng");
    assert_eq!(f.kind(), TreeKind::Source);
    assert_eq!(f.scope(), "slack:#eng");
    assert_eq!(f.summary_tree_kind(), SummaryTreeKind::Source);
}

#[test]
fn global_uses_global_scope_and_kind() {
    let global = TreeFactory::global();
    assert_eq!(global.kind(), TreeKind::Global);
    assert_eq!(global.scope(), GLOBAL_SCOPE);
}

#[test]
fn source_scope_slug_preserves_non_gmail_prefix() {
    let f = TreeFactory::source("slack:#eng");
    assert_eq!(f.scope_slug(), "slack-eng");
}

#[test]
fn source_scope_slug_strips_gmail_prefix_only() {
    let f = TreeFactory::source("gmail:alice@example.com|bob@example.com");
    assert_eq!(f.scope_slug(), "alice-example-com-bob-example-com");
}

#[test]
fn topic_scope_slug_keeps_canonical_prefix() {
    let f = TreeFactory::topic("email:alice@example.com");
    assert_eq!(f.scope_slug(), "email-alice-example-com");
    assert_eq!(f.summary_tree_kind(), SummaryTreeKind::Topic);
}
