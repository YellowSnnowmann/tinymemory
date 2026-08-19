use async_trait::async_trait;
use serde_json::Value;

use super::common::{checked_execute, document, first_array, pick_str};
use crate::sync::composio::providers::sync_state::SyncState;
use crate::sync::pipelines::composio::{
    run_incremental_sync, ActionExecutor, ComposioClient, IncrementalSource, PageFetch, SyncItem,
    SyncScope,
};
use crate::sync::pipelines::traits::PipelineConfig;
use crate::sync::pipelines::traits::{
    SkillDocument, SyncContext, SyncOutcome, SyncPipeline, SyncPipelineKind,
};

const ACTION_SEARCH: &str = "GOOGLEDOCS_SEARCH_DOCUMENTS";
const ACTION_PLAINTEXT: &str = "GOOGLEDOCS_GET_DOCUMENT_PLAINTEXT";

/// Incremental Google Docs synchronization through Composio.
///
/// Two-step, document-shaped (like `NotionSyncPipeline`): `GOOGLEDOCS_SEARCH_DOCUMENTS`
/// enumerates accessible documents, then `GOOGLEDOCS_GET_DOCUMENT_PLAINTEXT` fetches the
/// body for each item inside [`IncrementalSource::document`].
pub struct GoogleDocsSyncPipeline {
    client: ComposioClient,
    connection_id: String,
    max_pages: usize,
    page_size: usize,
}

impl GoogleDocsSyncPipeline {
    pub fn new(client: ComposioClient, connection_id: impl Into<String>) -> Self {
        Self {
            client,
            connection_id: connection_id.into(),
            // NOTE: SEARCH_DOCUMENTS' page-token arg name is not pinned by the
            // curated catalog, so we do a single-page-per-tick fetch (no page
            // token emitted) rather than guessing a pagination scheme. Capped at
            // 1 page: since `arguments()` never advances the token, a >1 cap
            // would re-fire the identical page-1 request and burn budget slots
            // for silently-deduplicated items.
            max_pages: 1,
            page_size: 25,
        }
    }
}

#[async_trait]
impl SyncPipeline for GoogleDocsSyncPipeline {
    fn id(&self) -> &str {
        "composio:googledocs"
    }
    fn kind(&self) -> SyncPipelineKind {
        SyncPipelineKind::Composio
    }
    async fn init(&self, _: &PipelineConfig, _: &SyncContext) -> anyhow::Result<()> {
        Ok(())
    }
    async fn tick(
        &self,
        config: &PipelineConfig,
        context: &SyncContext,
    ) -> anyhow::Result<SyncOutcome> {
        run_incremental_sync(self, &self.client, &self.connection_id, config, context).await
    }
}

#[async_trait]
impl IncrementalSource for GoogleDocsSyncPipeline {
    fn toolkit(&self) -> &'static str {
        "googledocs"
    }
    fn action(&self) -> &'static str {
        ACTION_SEARCH
    }
    fn max_pages(&self) -> usize {
        self.max_pages
    }
    fn arguments(
        &self,
        _: &SyncScope,
        config: &PipelineConfig,
        state: &SyncState,
        _page: Option<&str>,
    ) -> Value {
        // `GOOGLEDOCS_SEARCH_DOCUMENTS` fronts Drive's `files.list`, so it takes
        // the same server-side controls Drive does. Order deterministically by
        // modification time and bound the window with a `q` clause, so each
        // tick fetches what changed since the cursor rather than the same
        // first batch forever. Without this the action returned the identical
        // page every tick and documents past `max_results` were unreachable.
        let mut args = serde_json::json!({
            "query": "",
            "max_results": self.page_size,
            "order_by": "modifiedTime desc",
        });
        // Prefer the last-synced cursor, else the configured horizon. The
        // cursor is validated as RFC 3339 before it is interpolated into `q`,
        // so a malformed persisted value can never inject into the query — on
        // a bad value the depth filter is simply omitted (full scan).
        let floor = state
            .cursor
            .as_deref()
            .filter(|cursor| chrono::DateTime::parse_from_rfc3339(cursor).is_ok())
            .map(str::to_owned)
            .or_else(|| {
                config.sync_depth_days.map(|days| {
                    (chrono::Utc::now() - chrono::Duration::days(days as i64)).to_rfc3339()
                })
            });
        if let Some(floor) = floor {
            args["q"] = serde_json::json!(format!("modifiedTime > '{floor}'"));
        }
        args
    }
    fn server_side_depth(&self) -> bool {
        // The `q` floor above bounds depth on the server, so the orchestrator
        // must not additionally treat the cursor as a client-side stop.
        true
    }
    fn extract_page(&self, data: &Value, _: Option<&str>) -> PageFetch {
        PageFetch {
            items: first_array(
                data,
                &[
                    "/data/documents",
                    "/documents",
                    "/data/files",
                    "/files",
                    "/data/results",
                    "/results",
                    "/data/items",
                    "/items",
                ],
            ),
            // Bounded fetch: no page token consumed (see `max_pages`). The
            // pointers are read defensively should Composio surface one.
            next: [
                "/data/nextPageToken",
                "/nextPageToken",
                "/data/next_page_token",
                "/next_page_token",
            ]
            .iter()
            .find_map(|path| data.pointer(path).and_then(Value::as_str))
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .map(str::to_owned),
        }
    }
    fn dedup_key(&self, item: &Value) -> Option<String> {
        let id = pick_str(item, &["id", "data.id", "documentId", "data.documentId"])?;
        Some(match self.sort_cursor(item) {
            Some(modified) => format!("{id}@{modified}"),
            None => id,
        })
    }
    fn sort_cursor(&self, item: &Value) -> Option<String> {
        pick_str(
            item,
            &[
                "modifiedTime",
                "data.modifiedTime",
                "modified_time",
                "updatedTime",
            ],
        )
    }
    async fn document(
        &self,
        _: &SyncScope,
        connection_id: &str,
        item: SyncItem,
        executor: &dyn ActionExecutor,
        state: &mut SyncState,
    ) -> anyhow::Result<SkillDocument> {
        let id = pick_str(
            &item.raw,
            &["id", "data.id", "documentId", "data.documentId"],
        )
        .unwrap_or_else(|| item.dedup_key.clone());
        let title = pick_str(&item.raw, &["title", "data.title", "name", "data.name"])
            .unwrap_or_else(|| format!("Google Doc {id}"));
        // NOTE: GET_DOCUMENT_PLAINTEXT identifies the doc by an id argument;
        // Composio commonly keys this as "id" (or "document_id"). We send "id".
        let response = checked_execute(
            executor,
            ACTION_PLAINTEXT,
            serde_json::json!({"id": id}),
            connection_id,
            state,
        )
        .await?;
        let content = [
            "/data/text",
            "/text",
            "/data/plaintext",
            "/plaintext",
            "/data/content",
            "/content",
            "/data/response_data/text",
        ]
        .iter()
        .find_map(|path| response.data.pointer(path).and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or(serde_json::to_string_pretty(&item.raw)?);
        Ok(document(
            "googledocs",
            connection_id,
            &id,
            title,
            content,
            item.raw,
        ))
    }
}
