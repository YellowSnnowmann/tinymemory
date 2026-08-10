//! Product `Config` adapter for the tinycortex single-page web reader.

use async_trait::async_trait;

use crate::Config;
use crate::sources::readers::SourceReader;
use crate::sources::types::{
    MemorySourceEntry, SourceContent, SourceItem, SourceKind,
};

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
        tinycortex::memory::sources::SourceReader::list_items(
            &tinycortex::memory::sources::readers::web_page::WebPageReader,
            source,
            &crate::tinycortex::memory_config_from(
                config,
                config.workspace_dir().clone(),
            ),
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
        tinycortex::memory::sources::SourceReader::read_item(
            &tinycortex::memory::sources::readers::web_page::WebPageReader,
            source,
            item_id,
            &crate::tinycortex::memory_config_from(
                config,
                config.workspace_dir().clone(),
            ),
        )
        .await
        .map_err(|error| error.to_string())
    }
}
