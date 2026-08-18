//! Product `Config` adapter for the tinycortex folder reader.

use async_trait::async_trait;

use crate::sources::readers::SourceReader;
use crate::sources::types::{MemorySourceEntry, SourceContent, SourceItem, SourceKind};
use crate::Config;

pub struct FolderReader;

#[async_trait]
impl SourceReader for FolderReader {
    fn kind(&self) -> SourceKind {
        SourceKind::Folder
    }

    async fn list_items(
        &self,
        source: &MemorySourceEntry,
        config: &Config,
    ) -> Result<Vec<SourceItem>, String> {
        crate::engine::backend::sources::SourceReader::list_items(
            &crate::engine::backend::sources::readers::folder::FolderReader,
            source,
            &crate::engine::memory_config_from(config, config.workspace_dir().clone()),
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
        crate::engine::backend::sources::SourceReader::read_item(
            &crate::engine::backend::sources::readers::folder::FolderReader,
            source,
            item_id,
            &crate::engine::memory_config_from(config, config.workspace_dir().clone()),
        )
        .await
        .map_err(|error| error.to_string())
    }
}
