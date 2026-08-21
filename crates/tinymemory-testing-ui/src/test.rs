//! HTTP and static UI contract tests for the local testing harness.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

use tinymemory_api::capabilities::{Capabilities, Capability};
use tinymemory_api::error::MemoryError;
use tinymemory_api::health::MemoryHealth;
use tinymemory_api::provider::types::{ExportPage, ExportRecord, ImportOutcome, SourceScope};
use tinymemory_api::provider::{
    MemoryCore, MemoryDocuments, MemoryPortability, MemoryProvider, MemoryRecall,
};
use tinymemory_api::recall::OwnedRecallOpts;
use tinymemory_api::types::{
    MemoryCategory, MemoryEntry, MemoryTaint, NamespaceDocumentInput, NamespaceRetrievalContext,
    NamespaceSummary, StoredMemoryDocument,
};

use super::*;

fn empty_state() -> SharedState {
    Arc::new(AppState {
        active: RwLock::new(None),
    })
}

fn test_app(state: SharedState) -> Router {
    app(state, concat!(env!("CARGO_MANIFEST_DIR"), "/web"))
}

fn json_request(method: Method, uri: &str, value: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(value.to_string()))
        .unwrap()
}

async fn json_body(response: Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    }
}

async fn connect_local(router: &Router) {
    let response = router
        .clone()
        .oneshot(json_request(
            Method::POST,
            "/api/connect",
            json!({ "engine": "local" }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn operations_require_a_connected_engine() {
    let response = test_app(empty_state())
        .oneshot(json_request(
            Method::POST,
            "/api/store",
            json!({ "namespace": "notes", "key": "one", "content": "body" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(response).await,
        json!({ "error": "no engine connected yet" })
    );
}

#[tokio::test]
async fn local_connect_status_and_disconnect_are_consistent() {
    let router = test_app(empty_state());
    connect_local(&router).await;

    let status = router
        .clone()
        .oneshot(Request::get("/api/status").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let body = json_body(status).await;
    assert_eq!(body["connected"], true);
    assert_eq!(body["driver_id"], "tinycortex");

    let disconnected = router
        .clone()
        .oneshot(
            Request::post("/api/disconnect")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        json_body(disconnected).await,
        json!({
            "connected": false,
            "driver_id": null,
            "engine": null,
            "has_graph": false
        })
    );
}

#[tokio::test]
async fn local_engine_supports_the_complete_core_http_workflow() {
    let router = test_app(empty_state());
    connect_local(&router).await;

    let stored = router
        .clone()
        .oneshot(json_request(
            Method::POST,
            "/api/store",
            json!({
                "namespace": "notes",
                "key": "theme",
                "content": "prefers dark mode",
                "category": "daily",
                "session_id": "session-1",
                "taint": "external_sync"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(stored.status(), StatusCode::NO_CONTENT);

    let entry = router
        .clone()
        .oneshot(
            Request::get("/api/get?namespace=notes&key=theme")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let entry = json_body(entry).await;
    assert_eq!(entry["content"], "prefers dark mode");
    assert_eq!(entry["category"], "daily");
    assert_eq!(entry["session_id"], "session-1");
    assert_eq!(entry["taint"], "external_sync");

    let listed = router
        .clone()
        .oneshot(
            Request::get("/api/list?namespace=notes&category=daily&session_id=session-1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(json_body(listed).await.as_array().unwrap().len(), 1);

    let recalled = router
        .clone()
        .oneshot(json_request(
            Method::POST,
            "/api/recall",
            json!({ "query": "dark mode", "namespace": "notes", "limit": 10 }),
        ))
        .await
        .unwrap();
    assert_eq!(json_body(recalled).await[0]["key"], "theme");

    let exported = router
        .clone()
        .oneshot(
            Request::get("/api/export?limit=10")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        json_body(exported).await["records"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let forgotten = router
        .clone()
        .oneshot(json_request(
            Method::POST,
            "/api/forget",
            json!({ "namespace": "notes", "key": "theme" }),
        ))
        .await
        .unwrap();
    assert_eq!(json_body(forgotten).await, json!(true));
}

#[tokio::test]
async fn invalid_engine_deployment_and_cloud_credentials_are_rejected() {
    let cases = [
        (json!({ "engine": "unknown" }), "unknown engine: unknown"),
        (
            json!({ "engine": "supermemory" }),
            "supermemory requires an endpoint URL",
        ),
        (
            json!({ "engine": "mem0", "endpoint": "http://localhost", "deployment": "other" }),
            "unknown Mem0 deployment: other",
        ),
        (
            json!({ "engine": "mem0", "endpoint": "https://api.mem0.ai", "deployment": "cloud" }),
            "Mem0 Cloud requires an API key",
        ),
        (
            json!({ "engine": "cognee", "endpoint": "https://example.invalid", "deployment": "cloud" }),
            "Cognee Cloud requires an API key",
        ),
    ];

    for (request, message) in cases {
        let response = test_app(empty_state())
            .oneshot(json_request(Method::POST, "/api/connect", request))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(response).await["error"], message);
    }
}

#[tokio::test]
async fn memory_errors_have_stable_http_statuses_and_json_bodies() {
    let cases = [
        (MemoryError::Invalid("bad".into()), StatusCode::BAD_REQUEST),
        (
            MemoryError::PathEscape("bad".into()),
            StatusCode::BAD_REQUEST,
        ),
        (MemoryError::NotFound("gone".into()), StatusCode::NOT_FOUND),
        (
            MemoryError::BudgetExceeded("large".into()),
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (
            MemoryError::Unauthorized("key".into()),
            StatusCode::UNAUTHORIZED,
        ),
        (
            MemoryError::Timeout("slow".into()),
            StatusCode::GATEWAY_TIMEOUT,
        ),
        (
            MemoryError::Unavailable("busy".into()),
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        (MemoryError::Backend("bad".into()), StatusCode::BAD_GATEWAY),
    ];

    for (error, expected) in cases {
        let expected_message = error.to_string();
        let response = ApiError::from(error).into_response();
        assert_eq!(response.status(), expected);
        assert_eq!(json_body(response).await["error"], expected_message);
    }
}

#[tokio::test]
async fn document_formats_report_conversion_and_connection_route() {
    let router = test_app(empty_state());
    let disconnected = router
        .clone()
        .oneshot(
            Request::get("/api/documents/formats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let disconnected = json_body(disconnected).await;
    assert_eq!(disconnected["route"], Value::Null);
    assert_eq!(
        disconnected["formats"],
        json!(["markdown", "plain_text", "html"])
    );

    connect_local(&router).await;
    let connected = router
        .oneshot(
            Request::get("/api/documents/formats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(json_body(connected).await["route"].is_string());
}

type MultipartPart<'a> = (&'a str, Option<&'a str>, Option<&'a str>, &'a [u8]);

fn multipart_request(parts: &[MultipartPart<'_>]) -> Request<Body> {
    let boundary = "tinymemory-test-boundary";
    let mut body = Vec::new();
    for (name, filename, content_type, value) in parts {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"").as_bytes(),
        );
        if let Some(filename) = filename {
            body.extend_from_slice(format!("; filename=\"{filename}\"").as_bytes());
        }
        body.extend_from_slice(b"\r\n");
        if let Some(content_type) = content_type {
            body.extend_from_slice(format!("Content-Type: {content_type}\r\n").as_bytes());
        }
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(value);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Request::builder()
        .method(Method::POST)
        .uri("/api/documents/upload")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap()
}

#[tokio::test]
async fn document_upload_validates_required_parts_and_supported_formats() {
    let router = test_app(empty_state());
    connect_local(&router).await;

    let no_file = router
        .clone()
        .oneshot(multipart_request(&[(
            "namespace",
            None,
            None,
            b"documents",
        )]))
        .await
        .unwrap();
    assert_eq!(no_file.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(no_file).await["error"],
        "no `file` part in the upload"
    );

    let no_namespace = router
        .clone()
        .oneshot(multipart_request(&[(
            "file",
            Some("note.txt"),
            Some("text/plain"),
            b"hello",
        )]))
        .await
        .unwrap();
    assert_eq!(no_namespace.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(no_namespace).await["error"],
        "no `namespace` part in the upload"
    );

    let unsupported = router
        .oneshot(multipart_request(&[
            ("namespace", None, None, b"documents"),
            ("file", Some("note.pdf"), Some("application/pdf"), b"%PDF"),
        ]))
        .await
        .unwrap();
    assert_eq!(unsupported.status(), StatusCode::BAD_REQUEST);
}

#[derive(Default)]
struct RecordingProvider {
    document: Mutex<Option<NamespaceDocumentInput>>,
}

#[async_trait]
impl MemoryCore for RecordingProvider {
    async fn store(
        &self,
        _namespace: &str,
        _key: &str,
        _content: &str,
        _category: MemoryCategory,
        _session_id: Option<&str>,
        _taint: MemoryTaint,
    ) -> Result<(), MemoryError> {
        Ok(())
    }

    async fn get(&self, _namespace: &str, _key: &str) -> Result<Option<MemoryEntry>, MemoryError> {
        Ok(None)
    }

    async fn forget(&self, _namespace: &str, _key: &str) -> Result<bool, MemoryError> {
        Ok(false)
    }

    async fn list(
        &self,
        _namespace: Option<&str>,
        _category: Option<&MemoryCategory>,
        _session_id: Option<&str>,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        Ok(Vec::new())
    }

    async fn namespaces(&self) -> Result<Vec<NamespaceSummary>, MemoryError> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl MemoryRecall for RecordingProvider {
    async fn recall(
        &self,
        _query: &str,
        _limit: usize,
        _opts: &OwnedRecallOpts,
        _scope: Option<&SourceScope>,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl MemoryPortability for RecordingProvider {
    async fn export_page(
        &self,
        _cursor: Option<&str>,
        _limit: usize,
    ) -> Result<ExportPage, MemoryError> {
        Ok(ExportPage::default())
    }

    async fn import_records(
        &self,
        _records: Vec<ExportRecord>,
    ) -> Result<ImportOutcome, MemoryError> {
        Ok(ImportOutcome::default())
    }
}

#[async_trait]
impl MemoryDocuments for RecordingProvider {
    async fn put_document(&self, input: NamespaceDocumentInput) -> Result<String, MemoryError> {
        *self.document.lock().unwrap() = Some(input);
        Ok("document-1".to_string())
    }

    async fn get_document(
        &self,
        _namespace: &str,
        _key: &str,
    ) -> Result<Option<StoredMemoryDocument>, MemoryError> {
        Ok(None)
    }

    async fn list_documents(&self, _namespace: Option<&str>) -> Result<Value, MemoryError> {
        Ok(Value::Null)
    }

    async fn list_namespaces(&self) -> Result<Vec<String>, MemoryError> {
        Ok(Vec::new())
    }

    async fn delete_document(
        &self,
        _namespace: &str,
        _document_id: &str,
    ) -> Result<Value, MemoryError> {
        Ok(Value::Null)
    }

    async fn clear_namespace(&self, _namespace: &str) -> Result<(), MemoryError> {
        Ok(())
    }

    async fn query_documents(
        &self,
        namespace: &str,
        _query: &str,
        _limit: usize,
    ) -> Result<NamespaceRetrievalContext, MemoryError> {
        Ok(NamespaceRetrievalContext {
            namespace: namespace.to_string(),
            query: None,
            context_text: String::new(),
            hits: Vec::new(),
        })
    }
}

#[async_trait]
impl MemoryProvider for RecordingProvider {
    fn driver_id(&self) -> &str {
        "recording"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::mandatory().with(Capability::Documents)
    }

    async fn health(&self) -> MemoryHealth {
        MemoryHealth::Ready
    }

    fn as_documents(&self) -> Option<&dyn MemoryDocuments> {
        Some(self)
    }
}

#[tokio::test]
async fn text_upload_preserves_filename_tags_category_and_taint() {
    let provider = Arc::new(RecordingProvider::default());
    let state = empty_state();
    *state.active.write().await = Some(provider.clone());

    let response = test_app(state)
        .oneshot(multipart_request(&[
            ("namespace", None, None, b"document:manual"),
            ("key", None, None, b"readme"),
            ("tags", None, None, b"guide, important"),
            ("category", None, None, b"custom:manual"),
            ("taint", None, None, b"external_sync"),
            (
                "file",
                Some("README.txt"),
                Some("text/plain"),
                b"TinyMemory manual",
            ),
        ]))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["route"], "documents");

    let recorded = provider.document.lock().unwrap();
    let document = recorded.as_ref().unwrap();
    assert_eq!(document.namespace, "document:manual");
    assert_eq!(document.key, "readme");
    assert_eq!(document.content, "TinyMemory manual");
    assert_eq!(document.tags, ["guide", "important"]);
    assert_eq!(document.category, "custom:manual");
    assert_eq!(document.taint, MemoryTaint::ExternalSync);
    assert_eq!(document.metadata["filename"], "README.txt");
    assert_eq!(document.metadata["source_format"], "plain_text");
}

#[test]
fn html_exposes_every_visible_operation_and_its_route_contract() {
    let html = include_str!("../web/index.html");
    for id in [
        "connect-btn",
        "disconnect-btn",
        "store-btn",
        "upload-btn",
        "get-btn",
        "recall-btn",
        "list-btn",
        "namespaces-btn",
        "forget-btn",
        "export-btn",
        "graph-btn",
    ] {
        assert!(html.contains(&format!("id=\"{id}\"")), "missing #{id}");
    }
    for route in [
        "/connect",
        "/disconnect",
        "/store",
        "/get",
        "/recall",
        "/list",
        "/namespaces",
        "/forget",
        "/export",
        "/graph/relations",
    ] {
        assert!(
            html.contains(&format!("\"{route}")),
            "missing route {route}"
        );
    }
}
