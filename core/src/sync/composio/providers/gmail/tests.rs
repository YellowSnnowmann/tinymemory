//! Host-owned Gmail provider surface tests.
//!
//! Pagination, cursor, envelope parsing, and ingest behavior are owned and
//! tested by `crate::sync::pipelines::composio::GmailSyncPipeline`.

use super::GmailProvider;
use crate::sync::composio::providers::ComposioProvider;

#[test]
fn provider_metadata_is_stable() {
    let provider = GmailProvider::new();
    assert_eq!(provider.toolkit_slug(), "gmail");
    assert_eq!(provider.sync_interval_secs(), Some(15 * 60));
}

#[test]
fn default_impl_matches_new() {
    let _new = GmailProvider::new();
    let _default = <GmailProvider as Default>::default();
}

#[test]
fn provider_source_does_not_restrict_to_inbox() {
    let source = include_str!("provider.rs");
    assert!(
        !source.contains("\"in:inbox"),
        "provider query must not exclude sent mail"
    );
}
