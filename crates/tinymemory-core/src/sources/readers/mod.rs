//! Source reader trait and per-kind implementations.

pub mod conversation;
pub mod folder;
pub mod github;
pub mod rss;
pub mod twitter;
pub mod web_page;

use async_trait::async_trait;

use crate::sources::types::{MemorySourceEntry, SourceContent, SourceItem, SourceKind};
use crate::Config;

/// A reader that can list items and read content from a memory source.
#[async_trait]
pub trait SourceReader: Send + Sync {
    fn kind(&self) -> SourceKind;
    async fn list_items(
        &self,
        source: &MemorySourceEntry,
        config: &Config,
    ) -> Result<Vec<SourceItem>, String>;
    async fn read_item(
        &self,
        source: &MemorySourceEntry,
        item_id: &str,
        config: &Config,
    ) -> Result<SourceContent, String>;
}

/// Get the reader for a given source kind, if this crate has one.
///
/// `None` for [`SourceKind::Composio`]. The kind itself stays — records
/// synced from a connected account are still stored, still queried, and still
/// forgotten under it, and removing it would orphan every row already written.
/// What left is the *reading*: an OAuth connector is reached with a credential
/// this crate does not hold and must not, so the host fetches through
/// `tinyconnectors` and hands the records to the memory provider.
///
/// Returning `Option` rather than a stub reader that always errors is
/// deliberate: a caller has to decide what to do about a kind it cannot read,
/// and a stub would let it call and discover the same thing at runtime, once
/// per item.
pub fn reader_for(kind: &SourceKind) -> Option<Box<dyn SourceReader>> {
    match kind {
        SourceKind::Composio => None,
        SourceKind::Conversation => Some(Box::new(conversation::ConversationReader)),
        SourceKind::Folder => Some(Box::new(folder::FolderReader)),
        SourceKind::GithubRepo => Some(Box::new(github::GithubReader)),
        SourceKind::TwitterQuery => Some(Box::new(twitter::TwitterReader)),
        SourceKind::RssFeed => Some(Box::new(rss::RssReader::new())),
        SourceKind::WebPage => Some(Box::new(web_page::WebPageReader)),
    }
}
