//! Tests for the surrounding module.

use super::*;

#[test]
fn historical_module_paths_resolve_to_the_same_constants_as_the_new_path() {
    assert_eq!(
        catalogs_business::SHOPIFY_CURATED.len(),
        crate::sync::composio::providers::catalogs::SHOPIFY_CURATED.len()
    );
    assert_eq!(
        catalogs_google::GOOGLEDRIVE_CURATED.len(),
        crate::sync::composio::providers::catalogs::GOOGLEDRIVE_CURATED.len()
    );
    assert_eq!(
        catalogs_messaging::SLACK_CURATED.len(),
        crate::sync::composio::providers::catalogs::SLACK_CURATED.len()
    );
    assert_eq!(
        catalogs_microsoft::EXCEL_CURATED.len(),
        crate::sync::composio::providers::catalogs::EXCEL_CURATED.len()
    );
    assert_eq!(
        catalogs_productivity::JIRA_CURATED.len(),
        crate::sync::composio::providers::catalogs::JIRA_CURATED.len()
    );
    assert_eq!(
        catalogs_social_media::TWITTER_CURATED.len(),
        crate::sync::composio::providers::catalogs::TWITTER_CURATED.len()
    );
}
