//! Product `Config` adapter for the engine-neutral single-page web reader.
//!
//! The reader itself lives in `tinymemory-sources` (#18 §B4); this adapts the
//! host's `Config` to the workspace path it takes.

use async_trait::async_trait;

use crate::sources::readers::SourceReader;
use crate::sources::types::{MemorySourceEntry, SourceContent, SourceItem, SourceKind};
use crate::Config;

pub struct WebPageReader;

#[async_trait]
impl SourceReader for WebPageReader {
    fn kind(&self) -> SourceKind {
        SourceKind::WebPage
    }

    async fn list_items(
        &self,
        source: &MemorySourceEntry,
        config: &Config,
    ) -> Result<Vec<SourceItem>, String> {
        tinymemory_sources::readers::SourceReader::list_items(
            &tinymemory_sources::readers::web_page::WebPageReader,
            source,
            config.workspace_dir(),
        )
        .await
        .map_err(|error| error.to_string())
    }

    async fn read_item(
        &self,
        source: &MemorySourceEntry,
        item_id: &str,
        config: &Config,
    ) -> Result<SourceContent, String> {
        tinymemory_sources::readers::SourceReader::read_item(
            &tinymemory_sources::readers::web_page::WebPageReader,
            source,
            item_id,
            config.workspace_dir(),
        )
        .await
        .map_err(|error| error.to_string())
    }
}
