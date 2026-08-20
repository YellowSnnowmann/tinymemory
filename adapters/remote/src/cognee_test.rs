//! Cognee adapter contract tests over dataset, raw-file, and recall APIs.

#![allow(clippy::expect_used)]

use std::sync::{Arc, Mutex};

use axum::{
    extract::{Multipart, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{delete, get, patch, post},
    Json, Router,
};
use serde_json::{json, Value};
use tinymemory_api::{
    provider::{MemoryCore, MemoryProvider, MemoryRecall},
    recall::OwnedRecallOpts,
    traits::Memory,
    types::{MemoryCategory, MemoryTaint},
};

#[derive(Clone, Default)]
struct AppState(
    Arc<Mutex<Option<Vec<u8>>>>,
    Arc<Mutex<CallCounts>>,
    /// The filename the adapter actually uploaded — served back in the data
    /// listing, because the issue #69 keyed path resolves BY that name. The
    /// old double hardcoded a name nothing ever read.
    Arc<Mutex<Option<String>>>,
);

/// Per-route request counters for the issue #69 fan-out assertions.
#[derive(Default, Clone, Copy)]
struct CallCounts {
    datasets: usize,
    listings: usize,
    raws: usize,
}

async fn datasets(State(state): State<AppState>) -> Json<Value> {
    state.1.lock().expect("counts").datasets += 1;
    let values = if state.0.lock().expect("state lock").is_some() {
        vec![json!({
            "id": "dataset-1",
            "name": super::CogneeDialect::dataset_name("project")
        })]
    } else {
        vec![]
    };
    Json(Value::Array(values))
}
async fn data(State(state): State<AppState>) -> Json<Value> {
    state.1.lock().expect("counts").listings += 1;
    let name = state.2.lock().expect("name lock").clone();
    let values = if state.0.lock().expect("state lock").is_some() {
        let name = name.unwrap_or_else(|| "6b6579.tinymemory".to_owned());
        vec![json!({"id": "data-1", "name": name, "created_at": "2026-08-12T00:00:00Z"})]
    } else {
        vec![]
    };
    Json(Value::Array(values))
}
async fn raw(State(state): State<AppState>) -> impl IntoResponse {
    state.1.lock().expect("counts").raws += 1;
    state.0.lock().expect("state lock").clone().map_or_else(
        || (StatusCode::NOT_FOUND, Vec::new()),
        |body| (StatusCode::OK, body),
    )
}
async fn remember(State(state): State<AppState>, mut multipart: Multipart) -> StatusCode {
    while let Some(field) = multipart.next_field().await.expect("multipart") {
        if field.name() == Some("data") {
            if let Some(name) = field.file_name() {
                // Cognee's loader strips the final `.json`; mirror it.
                *state.2.lock().expect("name lock") =
                    Some(name.trim_end_matches(".json").to_owned());
            }
            *state.0.lock().expect("state lock") =
                Some(field.bytes().await.expect("body").to_vec());
        }
    }
    StatusCode::OK
}
async fn remove(State(state): State<AppState>) -> StatusCode {
    *state.0.lock().expect("state lock") = None;
    StatusCode::NO_CONTENT
}
async fn recall(State(state): State<AppState>) -> Json<Value> {
    let records = state
        .0
        .lock()
        .expect("state lock")
        .as_ref()
        .and_then(|bytes| String::from_utf8(bytes.clone()).ok())
        .map(|text| vec![json!({"text": text, "score": 0.8})])
        .unwrap_or_default();
    Json(Value::Array(records))
}

async fn capture_auth(State(state): State<Arc<Mutex<Value>>>, headers: HeaderMap) -> StatusCode {
    *state.lock().expect("state lock") = json!({
        "authorization": headers
            .get("authorization")
            .and_then(|value| value.to_str().ok()),
        "api_key": headers
            .get("x-api-key")
            .and_then(|value| value.to_str().ok()),
    });
    StatusCode::OK
}

#[tokio::test]
async fn cognee_supports_cloud_api_keys_and_self_hosted_bearer_tokens() {
    let captured = Arc::new(Mutex::new(Value::Null));
    let app = Router::new()
        .route("/health", get(capture_auth))
        .with_state(captured.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });

    let api = super::CogneeMemory::api(&endpoint, "cloud-secret").expect("api client");
    assert!(api.health_check().await);
    let api_headers = captured.lock().expect("state lock").clone();
    assert_eq!(api_headers["api_key"], "cloud-secret");
    assert!(api_headers["authorization"].is_null());

    let hosted = super::CogneeMemory::self_hosted(&endpoint, Some("local-secret"))
        .expect("self-hosted client");
    assert!(hosted.health_check().await);
    let hosted_headers = captured.lock().expect("state lock").clone();
    assert_eq!(hosted_headers["authorization"], "Bearer local-secret");
    assert!(hosted_headers["api_key"].is_null());

    let debug = format!("{api:?}");
    assert!(!debug.contains("cloud-secret"));
    assert!(super::CogneeMemory::api(&endpoint, "  ").is_err());
}

#[test]
fn cognee_remote_names_are_bounded_and_safe_for_arbitrary_contract_keys() {
    let unusual = format!("tenant / 🧠 / {}", "x".repeat(500));
    let dataset = super::CogneeDialect::dataset_name(&unusual);
    let filename = super::CogneeDialect::filename(&unusual);

    assert!(dataset.starts_with("tinymemory__tm_"));
    assert!(dataset.len() < 100);
    assert!(dataset
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'));
    assert!(filename.starts_with("tm_"));
    assert!(filename.ends_with(".tinymemory.json"));
    assert!(filename.len() < 100);
    assert_eq!(dataset, super::CogneeDialect::dataset_name(&unusual));
}

#[tokio::test]
async fn native_cognee_round_trips_the_tinymemory_contract() {
    let state = AppState::default();
    let app = Router::new()
        // The real API serves the collection at the slashed form and 307s the
        // bare one; the adapter now asks for `/api/v1/datasets/` directly, so
        // the double must answer there or it stops mirroring the service.
        .route("/api/v1/datasets/", get(datasets))
        .route("/api/v1/datasets/{dataset}/data", get(data))
        .route("/api/v1/datasets/{dataset}/data/{data}/raw", get(raw))
        .route("/api/v1/datasets/{dataset}/data/{data}", delete(remove))
        .route("/api/v1/remember", post(remember))
        .route("/api/v1/update", patch(remember))
        .route("/api/v1/recall", post(recall))
        .route("/health", get(|| async { StatusCode::OK }))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });

    let driver =
        crate::cognee_provider(super::CogneeMemory::self_hosted(&endpoint, None).expect("client"));
    tinymemory_api::provider::audit_provider(&driver).expect("honest capabilities");
    driver
        .store(
            "project",
            "key",
            "knowledge graph",
            MemoryCategory::Conversation,
            Some("session"),
            MemoryTaint::ExternalSync,
        )
        .await
        .expect("store");
    driver
        .store(
            "project",
            "key",
            "updated knowledge graph",
            MemoryCategory::Conversation,
            Some("session"),
            MemoryTaint::ExternalSync,
        )
        .await
        .expect("upsert");
    let entry = driver
        .get("project", "key")
        .await
        .expect("get")
        .expect("entry");
    assert_eq!(entry.content, "updated knowledge graph");
    assert_eq!(entry.taint, MemoryTaint::ExternalSync);
    assert_eq!(
        driver
            .recall(
                "graph",
                3,
                &OwnedRecallOpts {
                    namespace: Some("project".into()),
                    ..OwnedRecallOpts::default()
                },
                None
            )
            .await
            .expect("recall")
            .len(),
        1
    );
    // #68 review Major 2: Cognee's context-only recall is scoreless, so the
    // strict filter would have dropped 100% of every thresholded result.
    // The dialect declares scores_recall() = false and min_score is
    // documented-inert: the hit survives.
    assert_eq!(
        driver
            .recall(
                "graph",
                3,
                &OwnedRecallOpts {
                    namespace: Some("project".into()),
                    min_score: Some(0.5),
                    ..OwnedRecallOpts::default()
                },
                None
            )
            .await
            .expect("recall with a threshold the backend cannot score")
            .len(),
        1
    );
    assert!(driver.forget("project", "key").await.expect("forget"));
    assert!(!driver.forget("project", "key").await.expect("forget again"));
    assert!(driver.health().await.is_usable());
}

/// Issue #69: the keyed get is three requests — dataset resolve, one
/// listing, ONE raw — however many records the store holds. The pre-seam
/// path raw-fetched every record in every dataset (1 + D + N), which is what
/// made a 10k-record hosted store cost ~10,002 serial requests per get. And
/// a keyed delete needs no envelope at all: zero raws.
#[tokio::test]
async fn keyed_ops_never_fan_out_over_raw_fetches() {
    let state = AppState::default();
    let app = Router::new()
        .route("/api/v1/datasets/", get(datasets))
        .route("/api/v1/datasets/{dataset}/data", get(data))
        .route("/api/v1/datasets/{dataset}/data/{data}/raw", get(raw))
        .route("/api/v1/datasets/{dataset}/data/{data}", delete(remove))
        .route("/api/v1/remember", post(remember))
        .route("/api/v1/update", patch(remember))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    let driver =
        crate::cognee_provider(super::CogneeMemory::self_hosted(&endpoint, None).expect("client"));

    driver
        .store(
            "project",
            "key",
            "knowledge graph",
            MemoryCategory::Core,
            None,
            MemoryTaint::Internal,
        )
        .await
        .expect("store");
    *state.1.lock().expect("counts") = CallCounts::default();

    driver
        .get("project", "key")
        .await
        .expect("get")
        .expect("entry");
    let counts = *state.1.lock().expect("counts");
    assert_eq!(counts.raws, 1, "exactly one raw fetch per keyed get");
    assert_eq!(counts.listings, 1, "exactly one data listing per keyed get");
    assert!(
        counts.datasets <= 1,
        "one dataset resolve per keyed get, got {}",
        counts.datasets
    );

    *state.1.lock().expect("counts") = CallCounts::default();
    assert!(driver.forget("project", "key").await.expect("forget"));
    let counts = *state.1.lock().expect("counts");
    assert_eq!(counts.raws, 0, "a keyed delete reads no envelopes");
}
