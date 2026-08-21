//! Deterministic contract tests for the document-oriented Composio providers.

use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::{
    GoogleCalendarSyncPipeline, GoogleDocsSyncPipeline, GoogleDriveSyncPipeline,
    GoogleSheetsSyncPipeline, OutlookSyncPipeline, TodoistSyncPipeline,
};
use crate::sync::composio::providers::sync_state::SyncState;
use crate::sync::pipelines::composio::{
    ActionExecutor, ComposioClient, ExecuteResponse, IncrementalSource, SyncItem, SyncScope,
};
use crate::sync::pipelines::traits::{
    ComposioSyncConfig, PipelineConfig, SyncPipeline, SyncPipelineKind,
};

#[derive(Debug)]
struct StubExecutor {
    response: anyhow::Result<ExecuteResponse, &'static str>,
    calls: Mutex<Vec<(String, Value, Option<String>)>>,
}

impl StubExecutor {
    fn succeeds(data: Value) -> Self {
        Self {
            response: Ok(ExecuteResponse {
                data,
                successful: true,
                error: None,
                cost_usd: 0.25,
                markdown_formatted: None,
                attempts: 2,
            }),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn provider_failure(message: &'static str) -> Self {
        Self {
            response: Ok(ExecuteResponse {
                data: Value::Null,
                successful: false,
                error: Some(message.into()),
                cost_usd: 0.0,
                markdown_formatted: None,
                attempts: 1,
            }),
            calls: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl ActionExecutor for StubExecutor {
    async fn execute(
        &self,
        action: &str,
        arguments: Value,
        connection_id: Option<&str>,
    ) -> anyhow::Result<ExecuteResponse> {
        self.calls.lock().expect("calls lock").push((
            action.into(),
            arguments,
            connection_id.map(str::to_owned),
        ));
        match &self.response {
            Ok(response) => Ok(response.clone()),
            Err(message) => anyhow::bail!(*message),
        }
    }
}

fn client() -> ComposioClient {
    ComposioClient::new(ComposioSyncConfig::default())
}

fn item(raw: Value, dedup_key: &str) -> SyncItem {
    SyncItem {
        dedup_key: dedup_key.into(),
        sort_cursor: None,
        raw,
    }
}

#[test]
fn providers_publish_stable_action_and_paging_contracts() {
    let calendar = GoogleCalendarSyncPipeline::new(client(), "calendar").with_limits(0, 9_999);
    let docs = GoogleDocsSyncPipeline::new(client(), "docs");
    let drive = GoogleDriveSyncPipeline::new(client(), "drive").with_limits(0, 9_999);
    let sheets = GoogleSheetsSyncPipeline::new(client(), "sheets");
    let outlook = OutlookSyncPipeline::new(client(), "outlook").with_limits(0, 0);
    let todoist = TodoistSyncPipeline::new(client(), "todoist").with_limits(0, 500);

    let cases: [(&dyn IncrementalSource, &str, &str, usize, bool, bool); 6] = [
        (
            &calendar,
            "googlecalendar",
            "GOOGLECALENDAR_EVENTS_LIST",
            1,
            false,
            true,
        ),
        (
            &docs,
            "googledocs",
            "GOOGLEDOCS_SEARCH_DOCUMENTS",
            1,
            false,
            true,
        ),
        (
            &drive,
            "googledrive",
            "GOOGLEDRIVE_FIND_FILE",
            1,
            false,
            true,
        ),
        (
            &sheets,
            "googlesheets",
            "GOOGLESHEETS_SEARCH_SPREADSHEETS",
            1,
            false,
            false,
        ),
        (&outlook, "outlook", "OUTLOOK_LIST_MESSAGES", 1, true, true),
        (&todoist, "todoist", "TODOIST_GET_ALL_TASKS", 1, true, false),
    ];
    for (provider, toolkit, action, pages, stop_on_empty, server_depth) in cases {
        assert_eq!(provider.toolkit(), toolkit);
        assert_eq!(provider.action(), action);
        assert_eq!(provider.max_pages(), pages);
        assert_eq!(provider.stop_on_empty_pending(), stop_on_empty);
        assert_eq!(provider.server_side_depth(), server_depth);
    }

    let pipelines: [(&dyn SyncPipeline, &str); 6] = [
        (&calendar, "composio:googlecalendar"),
        (&docs, "composio:googledocs"),
        (&drive, "composio:googledrive"),
        (&sheets, "composio:googlesheets"),
        (&outlook, "composio:outlook"),
        (&todoist, "composio:todoist"),
    ];
    for (pipeline, id) in pipelines {
        assert_eq!(pipeline.id(), id);
        assert_eq!(pipeline.kind(), SyncPipelineKind::Composio);
    }
}

#[test]
fn calendar_uses_cursor_page_and_normalizes_event_shapes() {
    let provider = GoogleCalendarSyncPipeline::new(client(), "connection").with_limits(3, 0);
    let mut state = SyncState::new("googlecalendar", "connection");
    state.cursor = Some("2026-01-02T03:04:05Z".into());
    let args = provider.arguments(
        &SyncScope::flat(),
        &PipelineConfig::default(),
        &state,
        Some(" next "),
    );
    assert_eq!(args["calendar_id"], "primary");
    assert_eq!(args["max_results"], 1);
    assert_eq!(args["page_token"], " next ");
    assert_eq!(args["updated_min"], "2026-01-02T03:04:05Z");
    assert!(args.get("time_min").is_none());

    let page = provider.extract_page(
        &json!({"data":{"events":[{"id":"event"}],"nextPageToken":" token "}}),
        None,
    );
    assert_eq!(page.items, vec![json!({"id":"event"})]);
    assert_eq!(page.next.as_deref(), Some("token"));
    let event = json!({"iCalUID":"ical", "updated":"2026-01-03T00:00:00Z"});
    assert_eq!(
        provider.dedup_key(&event).as_deref(),
        Some("ical@2026-01-03T00:00:00Z")
    );
    assert_eq!(
        provider.sort_cursor(&event).as_deref(),
        Some("2026-01-03T00:00:00Z")
    );
    assert_eq!(provider.dedup_key(&json!({})), None);
}

#[tokio::test]
async fn calendar_document_preserves_external_metadata_and_fallbacks() {
    let provider = GoogleCalendarSyncPipeline::new(client(), "unused");
    let mut state = SyncState::new("googlecalendar", "connection");
    let executor = StubExecutor::succeeds(Value::Null);
    let document = provider
        .document(
            &SyncScope::flat(),
            "connection",
            item(json!({"iCalUID": 42, "summary":"Planning"}), "fallback"),
            &executor,
            &mut state,
        )
        .await
        .expect("calendar document");
    assert_eq!(document.document_id, "googlecalendar:42");
    assert_eq!(document.title, "Planning");
    assert_eq!(document.metadata["provider_id"], "42");
    assert_eq!(document.metadata["taint"], "external_sync");
    assert!(document.content.contains("Planning"));
}

#[test]
fn docs_validate_cursor_and_accept_wrapped_search_results() {
    let provider = GoogleDocsSyncPipeline::new(client(), "connection");
    let mut state = SyncState::new("googledocs", "connection");
    state.cursor = Some("2026-04-05T06:07:08Z".into());
    let args = provider.arguments(
        &SyncScope::flat(),
        &PipelineConfig::default(),
        &state,
        Some("ignored"),
    );
    assert_eq!(args["q"], "modifiedTime > '2026-04-05T06:07:08Z'");
    assert_eq!(args["max_results"], 25);
    assert!(args.get("page_token").is_none());
    state.cursor = Some("' or trashed = false".into());
    assert!(provider
        .arguments(&SyncScope::flat(), &PipelineConfig::default(), &state, None)
        .get("q")
        .is_none());

    let page = provider.extract_page(
        &json!({"data":{"documents":[{"documentId":"doc"}]},"next_page_token":" p2 "}),
        None,
    );
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.next.as_deref(), Some("p2"));
    let doc = json!({"data":{"documentId":"doc","modifiedTime":"2026-05-01T00:00:00Z"}});
    assert_eq!(
        provider.dedup_key(&doc).as_deref(),
        Some("doc@2026-05-01T00:00:00Z")
    );
}

#[tokio::test]
async fn docs_fetch_plaintext_and_propagate_provider_failures() {
    let provider = GoogleDocsSyncPipeline::new(client(), "unused");
    let executor = StubExecutor::succeeds(json!({"data":{"plaintext":"body text"}}));
    let mut state = SyncState::new("googledocs", "connection");
    let document = provider
        .document(
            &SyncScope::flat(),
            "connection",
            item(json!({"documentId":"doc-1","name":"Roadmap"}), "key"),
            &executor,
            &mut state,
        )
        .await
        .expect("docs document");
    assert_eq!(document.title, "Roadmap");
    assert_eq!(document.content, "body text");
    assert_eq!(state.run_requests, 2);
    assert_eq!(state.run_provider_cost_usd, 0.25);
    assert_eq!(
        executor.calls.lock().expect("calls lock")[0],
        (
            "GOOGLEDOCS_GET_DOCUMENT_PLAINTEXT".into(),
            json!({"id":"doc-1"}),
            Some("connection".into()),
        )
    );

    let failing = StubExecutor::provider_failure("permission denied");
    let error = provider
        .document(
            &SyncScope::flat(),
            "connection",
            item(json!({"id":"doc-2"}), "key"),
            &failing,
            &mut state,
        )
        .await
        .expect_err("provider failure must propagate");
    assert!(error.to_string().contains("permission denied"));

    let empty = StubExecutor::succeeds(json!({"text":"   "}));
    let fallback = provider
        .document(
            &SyncScope::flat(),
            "connection",
            item(json!({"id":"doc-3","name":"Empty body"}), "key"),
            &empty,
            &mut state,
        )
        .await
        .expect("empty plaintext falls back to search metadata");
    assert!(fallback.content.contains("Empty body"));
}

#[test]
fn drive_clamps_limits_validates_cursor_and_paginates() {
    let provider = GoogleDriveSyncPipeline::new(client(), "connection").with_limits(2, 2_000);
    let mut state = SyncState::new("googledrive", "connection");
    state.cursor = Some("2026-06-01T12:00:00+00:00".into());
    let args = provider.arguments(
        &SyncScope::flat(),
        &PipelineConfig::default(),
        &state,
        Some("page-2"),
    );
    assert_eq!(provider.max_pages(), 2);
    assert_eq!(args["page_size"], 1_000);
    assert_eq!(args["page_token"], "page-2");
    assert_eq!(args["q"], "modifiedTime > '2026-06-01T12:00:00+00:00'");
    assert!(args["fields"]
        .as_str()
        .expect("fields")
        .contains("modifiedTime"));
    let page = provider.extract_page(
        &json!({"data":{"data":{"files":[{"fileId":7}],"nextPageToken":"p3"}}}),
        None,
    );
    assert_eq!(page.items, vec![json!({"fileId":7})]);
    assert_eq!(page.next.as_deref(), Some("p3"));
    let file = json!({"fileId":7,"modified_time":"cursor"});
    assert_eq!(provider.dedup_key(&file).as_deref(), Some("7@cursor"));
}

#[tokio::test]
async fn drive_document_serializes_metadata_without_fetching_body() {
    let provider = GoogleDriveSyncPipeline::new(client(), "unused");
    let executor = StubExecutor::provider_failure("must not execute");
    let mut state = SyncState::new("googledrive", "connection");
    let document = provider
        .document(
            &SyncScope::flat(),
            "connection",
            item(
                json!({"fileId":"file-1","name":"Budget.pdf","mimeType":"application/pdf"}),
                "key",
            ),
            &executor,
            &mut state,
        )
        .await
        .expect("drive document");
    assert_eq!(document.document_id, "googledrive:file-1");
    assert_eq!(document.title, "Budget.pdf");
    assert!(document.content.contains("application/pdf"));
    assert!(executor.calls.lock().expect("calls lock").is_empty());
}

#[test]
fn sheets_use_bounded_search_and_normalize_spreadsheet_shapes() {
    let provider = GoogleSheetsSyncPipeline::new(client(), "connection");
    let args = provider.arguments(
        &SyncScope::flat(),
        &PipelineConfig::default(),
        &SyncState::new("googlesheets", "connection"),
        Some("ignored"),
    );
    assert_eq!(args, json!({"query":"","max_results":25}));
    let page = provider.extract_page(
        &json!({"data":{"files":[{"spreadsheetId":"sheet"}],"next_page_token":" next "}}),
        None,
    );
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.next.as_deref(), Some("next"));
    let sheet = json!({"spreadsheetId":"sheet","modified_time":"cursor"});
    assert_eq!(provider.dedup_key(&sheet).as_deref(), Some("sheet@cursor"));
}

#[tokio::test]
async fn sheets_fetch_info_with_canonical_argument_and_accounting() {
    let provider = GoogleSheetsSyncPipeline::new(client(), "unused");
    let executor = StubExecutor::succeeds(json!({"data":{"properties":{"locale":"en_US"}}}));
    let mut state = SyncState::new("googlesheets", "connection");
    let document = provider
        .document(
            &SyncScope::flat(),
            "connection",
            item(
                json!({"spreadsheetId":"sheet-1","properties":{"title":"Forecast"}}),
                "key",
            ),
            &executor,
            &mut state,
        )
        .await
        .expect("sheets document");
    assert_eq!(document.title, "Forecast");
    assert!(document.content.contains("en_US"));
    assert_eq!(state.run_requests, 2);
    assert_eq!(
        executor.calls.lock().expect("calls lock")[0].1,
        json!({"spreadsheet_id":"sheet-1"})
    );
}

#[test]
fn outlook_filters_by_cursor_and_extracts_graph_skiptoken() {
    let provider = OutlookSyncPipeline::new(client(), "connection").with_limits(4, 0);
    let mut state = SyncState::new("outlook", "connection");
    state.cursor = Some("2026-07-01T01:02:03Z".into());
    let args = provider.arguments(
        &SyncScope::flat(),
        &PipelineConfig::default(),
        &state,
        Some("already-bare"),
    );
    assert_eq!(args["top"], 1);
    assert_eq!(args["skip_token"], "already-bare");
    assert_eq!(args["filter"], "receivedDateTime ge 2026-07-01T01:02:03Z");
    let page = provider.extract_page(
        &json!({
            "value":[{"messageId":"mail"}],
            "@odata.nextLink":"https://graph.example/messages?foo=1&$skiptoken=A%2BB&top=25"
        }),
        None,
    );
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.next.as_deref(), Some("A%2BB"));
    let mail =
        json!({"messageId":"mail","received_date_time":"cursor","lastModifiedDateTime":"wrong"});
    assert_eq!(provider.dedup_key(&mail).as_deref(), Some("mail@cursor"));
    assert_eq!(
        provider.sort_cursor(&json!({"lastModifiedDateTime":"wrong"})),
        None
    );
}

#[tokio::test]
async fn outlook_document_uses_subject_and_raw_message_body() {
    let provider = OutlookSyncPipeline::new(client(), "unused");
    let executor = StubExecutor::provider_failure("must not execute");
    let mut state = SyncState::new("outlook", "connection");
    let document = provider
        .document(
            &SyncScope::flat(),
            "connection",
            item(
                json!({"id":"mail-1","subject":"Hello","body":{"content":"World"}}),
                "key",
            ),
            &executor,
            &mut state,
        )
        .await
        .expect("outlook document");
    assert_eq!(document.title, "Hello");
    assert!(document.content.contains("World"));
    assert!(executor.calls.lock().expect("calls lock").is_empty());
}

#[test]
fn todoist_handles_bare_and_wrapped_arrays_and_fingerprints_edits() {
    let provider = TodoistSyncPipeline::new(client(), "connection");
    assert_eq!(
        provider.arguments(
            &SyncScope::flat(),
            &PipelineConfig::default(),
            &SyncState::new("todoist", "connection"),
            Some("ignored"),
        ),
        json!({})
    );
    assert_eq!(
        provider
            .extract_page(&json!([{"id":"1"}]), None)
            .items
            .len(),
        1
    );
    assert_eq!(
        provider
            .extract_page(&json!({"data":{"tasks":[{"id":"2"}]}}), None)
            .items
            .len(),
        1
    );
    let first = json!({"id":"task","content":"write","nested":{"b":2,"a":1}});
    let reordered = json!({"nested":{"a":1,"b":2},"content":"write","id":"task"});
    let edited = json!({"id":"task","content":"ship","nested":{"a":1,"b":2}});
    assert_eq!(provider.dedup_key(&first), provider.dedup_key(&reordered));
    assert_ne!(provider.dedup_key(&first), provider.dedup_key(&edited));
    assert_eq!(provider.sort_cursor(&first), None);
}

#[tokio::test]
async fn todoist_document_combines_task_text_and_description() {
    let provider = TodoistSyncPipeline::new(client(), "unused");
    let executor = StubExecutor::provider_failure("must not execute");
    let mut state = SyncState::new("todoist", "connection");
    let document = provider
        .document(
            &SyncScope::flat(),
            "connection",
            item(
                json!({"task_id":9,"content":"Write tests","description":"Cover failures"}),
                "key",
            ),
            &executor,
            &mut state,
        )
        .await
        .expect("todoist document");
    assert_eq!(document.document_id, "todoist:9");
    assert_eq!(document.content, "Write tests\n\nCover failures");
    assert!(executor.calls.lock().expect("calls lock").is_empty());

    let fallback = provider
        .document(
            &SyncScope::flat(),
            "connection",
            item(json!({"id":"task-raw","priority":4}), "key"),
            &executor,
            &mut state,
        )
        .await
        .expect("task without content uses raw payload");
    assert_eq!(fallback.title, "Todoist task task-raw");
    assert!(fallback.content.contains("priority"));
}
