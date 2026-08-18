//! Product `Config` adapter for the engine-neutral GitHub repo reader.
//!
//! The reader itself — commit/issue/PR fetching over `gh`, `git`, and the
//! public REST API — lives in the engine. This module keeps the host-side
//! `SourceReader` shape (`&Config`, `Result<_, String>`) that the sources RPC
//! surface and the sync runner are written against, and re-exports the two
//! coordinate helpers `sources::sync` derives its scopes from.

use async_trait::async_trait;

use crate::sources::readers::SourceReader;
use crate::sources::types::{MemorySourceEntry, SourceContent, SourceItem, SourceKind};
use crate::Config;

pub use tinymemory_sources::readers::github::{repo_archive_source_id, repo_chunk_scope};

pub struct GithubReader;

#[async_trait]
impl SourceReader for GithubReader {
    fn kind(&self) -> SourceKind {
        SourceKind::GithubRepo
    }

    async fn list_items(
        &self,
        source: &MemorySourceEntry,
        config: &Config,
    ) -> Result<Vec<SourceItem>, String> {
        tinymemory_sources::readers::SourceReader::list_items(
            &tinymemory_sources::readers::github::GithubReader,
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
            &tinymemory_sources::readers::github::GithubReader,
            source,
            item_id,
            config.workspace_dir(),
        )
        .await
        .map_err(|error| error.to_string())
    }
}
