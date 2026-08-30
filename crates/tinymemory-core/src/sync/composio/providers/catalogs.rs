//! The curated catalogs, re-exported at their historical path.
//!
//! The tables themselves moved to [`tinymemory_api::composio::catalogs`]
//! (OpenHuman#5560): they are `&'static str` slugs with no dependency, and the
//! *host* is their heaviest reader — it filters the agent's visible tool list
//! and renders the unlock hints. While they lived here, every one of those
//! reads was a compile-time link to this crate.
//!
//! Nothing about the data changed. This module keeps
//! `providers::catalogs::SLACK_CURATED` and its siblings resolving for the
//! provider impls beside it.

pub use tinymemory_api::composio::catalogs::business::{
    AIRTABLE_CURATED, FIGMA_CURATED, HUBSPOT_CURATED, SALESFORCE_CURATED, SHOPIFY_CURATED,
    STRIPE_CURATED,
};
pub use tinymemory_api::composio::catalogs::google::{
    GOOGLECALENDAR_CURATED, GOOGLEDOCS_CURATED, GOOGLEDRIVE_CURATED, GOOGLESHEETS_CURATED,
};
pub use tinymemory_api::composio::catalogs::messaging::{
    DISCORD_CURATED, MICROSOFT_TEAMS_CURATED, SLACK_CURATED, TELEGRAM_CURATED, WHATSAPP_CURATED,
};
pub use tinymemory_api::composio::catalogs::microsoft::{EXCEL_CURATED, ONE_DRIVE_CURATED};
pub use tinymemory_api::composio::catalogs::productivity::{
    ASANA_CURATED, DROPBOX_CURATED, JIRA_CURATED, OUTLOOK_CURATED, TODOIST_CURATED, TRELLO_CURATED,
};
pub use tinymemory_api::composio::catalogs::social_media::{
    SPOTIFY_CURATED, TWITTER_CURATED, YOUTUBE_CURATED,
};
