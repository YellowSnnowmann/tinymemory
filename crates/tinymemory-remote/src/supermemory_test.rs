//! Supermemory adapter contract tests over its native HTTP shapes.

#![allow(clippy::expect_used)]

use std::sync::{Arc, Mutex};

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use tinymemory_api::{
    provider::{MemoryCore, MemoryProvider, MemoryRecall},
    recall::OwnedRecallOpts,
    traits::Memory,
    types::{MemoryCategory, MemoryTaint},
};

#[derive(Default)]
struct Fixture {
    records: Vec<Value>,
    container_tags: Vec<String>,
    last_search_tag: Option<String>,
    /// Every /v4/memories/list request body, for the issue #69 scoping
    /// assertions: a namespace-scoped read must ask for ONE tag.
    list_bodies: Vec<Value>,
}

#[derive(Clone, Default)]
struct AppState(Arc<Mutex<Fixture>>);

async fn tags(State(state): State<AppState>) -> Json<Value> {
    let fixture = state.0.lock().expect("state lock");
    Json(Value::Array(
        fixture
            .container_tags
            .iter()
            .map(|tag| json!({"containerTag": tag}))
            .collect(),
    ))
}

async fn list(State(state): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    let mut fixture = state.0.lock().expect("state lock");
    fixture.list_bodies.push(body);
    Json(json!({"memoryEntries": fixture.records, "pagination": {"totalPages": 1}}))
}
async fn add(State(state): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    let mut fixture = state.0.lock().expect("state lock");
    let id = format!("doc-{}", fixture.records.len() + 1);
    let container_tag = body["containerTag"].as_str().expect("container tag");
    if !fixture
        .container_tags
        .iter()
        .any(|tag| tag == container_tag)
    {
        fixture.container_tags.push(container_tag.to_owned());
    }
    fixture.records.push(json!({
        "id": id,
        "memory": body["memories"][0]["content"],
        "metadata": body["memories"][0]["metadata"],
        "containerTag": container_tag,
        "createdAt": "2026-08-12T00:00:00Z",
        "isLatest": true,
        "isForgotten": false
    }));
    Json(json!({"memories": [{"id": id}]}))
}
async fn update(State(state): State<AppState>, Json(body): Json<Value>) -> StatusCode {
    // The real PATCH /v4/memories requires `containerTag` ("Required to scope
    // the operation") and 400s without it — a double that accepted a tagless
    // PATCH hid exactly the adapter regression issue #75 found. And the VALUE
    // matters as much as the presence: the tag scopes the operation, so a
    // PATCH carrying another container's tag is a lost update or a
    // cross-container write — refuse a mismatch instead of filing it.
    let Some(sent_tag) = body["containerTag"].as_str().filter(|tag| !tag.is_empty()) else {
        return StatusCode::BAD_REQUEST;
    };
    let id = body["id"].as_str().unwrap_or_default();
    if let Some(record) = state
        .0
        .lock()
        .expect("state lock")
        .records
        .iter_mut()
        .find(|r| r["id"] == id)
    {
        if record["containerTag"] != sent_tag {
            return StatusCode::BAD_REQUEST;
        }
        record["memory"] = body["newContent"].clone();
        record["metadata"] = body["metadata"].clone();
    }
    StatusCode::OK
}
async fn remove(State(state): State<AppState>, Json(body): Json<Value>) -> StatusCode {
    let id = body["id"].as_str().unwrap_or_default();
    state
        .0
        .lock()
        .expect("state lock")
        .records
        .retain(|r| r["id"] != id);
    StatusCode::OK
}
async fn search(State(state): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    let mut fixture = state.0.lock().expect("state lock");
    fixture.last_search_tag = body["containerTag"].as_str().map(str::to_owned);
    let results = fixture.records.iter().map(|r| json!({"id": r["id"], "memory": r["memory"], "metadata": r["metadata"], "similarity": 0.95})).collect::<Vec<_>>();
    Json(json!({"results": results}))
}

async fn capture_auth(State(state): State<Arc<Mutex<Value>>>, headers: HeaderMap) -> StatusCode {
    *state.lock().expect("state lock") = json!({
        "authorization": headers
            .get("authorization")
            .and_then(|value| value.to_str().ok()),
    });
    StatusCode::OK
}

#[tokio::test]
async fn supermemory_supports_provided_and_self_hosted_apis() {
    let captured = Arc::new(Mutex::new(Value::Null));
    // The health probe now proves auth + data plane via the container-tags
    // list (§U4), so that is where the capture sits — a bare root `/` no
    // longer receives the probe.
    let app = Router::new()
        .route("/v3/container-tags/list", get(capture_auth))
        .with_state(captured.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });

    for client in [
        super::SupermemoryMemory::api(&endpoint, "provided-secret").expect("api client"),
        super::SupermemoryMemory::self_hosted(&endpoint, "provided-secret")
            .expect("self-hosted client"),
    ] {
        assert!(client.health_check().await);
        let headers = captured.lock().expect("state lock").clone();
        assert_eq!(headers["authorization"], "Bearer provided-secret");
        assert!(!format!("{client:?}").contains("provided-secret"));
    }
    assert!(super::SupermemoryMemory::api(&endpoint, "").is_err());
}

#[test]
fn supermemory_container_tags_cover_arbitrary_contract_namespaces() {
    let unusual = format!("tenant / 🧠 / {}", "x".repeat(500));
    let tag = super::SupermemoryDialect::container_tag(&unusual);

    assert!(tag.starts_with("tinymemory:tm_"));
    assert!(tag.len() <= 100);
    assert!(tag
        .bytes()
        .all(|byte| { byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b':' | b'-') }));
    assert_eq!(tag, super::SupermemoryDialect::container_tag(&unusual));
}

#[tokio::test]
async fn native_supermemory_round_trips_the_tinymemory_contract() {
    let state = AppState::default();
    let app = Router::new()
        .route("/v3/container-tags/list", get(tags))
        .route("/v4/memories/list", post(list))
        .route("/v4/memories", post(add).patch(update).delete(remove))
        .route("/v4/search", post(search))
        .route("/", get(|| async { StatusCode::OK }))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });

    let driver = crate::supermemory_provider(
        super::SupermemoryMemory::self_hosted(&endpoint, "secret").expect("client"),
    );
    tinymemory_api::provider::audit_provider(&driver).expect("honest capabilities");
    driver
        .store(
            "project",
            "decision",
            "use Rust",
            MemoryCategory::Core,
            None,
            MemoryTaint::ExternalSync,
        )
        .await
        .expect("store");
    driver
        .store(
            "project",
            "decision",
            "use Rust 2024",
            MemoryCategory::Core,
            None,
            MemoryTaint::ExternalSync,
        )
        .await
        .expect("upsert");
    let entry = driver
        .get("project", "decision")
        .await
        .expect("get")
        .expect("entry");
    assert_eq!(entry.content, "use Rust 2024");
    assert_eq!(entry.taint, MemoryTaint::ExternalSync);
    let expected_tag = super::SupermemoryDialect::container_tag("project");
    assert_eq!(
        state.0.lock().expect("state lock").container_tags,
        vec![expected_tag.clone()]
    );
    assert_eq!(
        driver
            .recall(
                "Rust",
                1,
                &OwnedRecallOpts {
                    namespace: Some("project".into()),
                    ..OwnedRecallOpts::default()
                },
                None,
            )
            .await
            .expect("recall")
            .len(),
        1
    );
    assert_eq!(
        state.0.lock().expect("state lock").last_search_tag,
        Some(expected_tag)
    );
    // #68 review Major 2: min_score connected to the double's OWN response
    // shape — the first cut's strictness was only ever tested against
    // synthetic Option values, which is how a decode/emit field mismatch
    // dropped every hit. Below the double's similarity (0.95): survives.
    // Above it: drops. Semantics AND the decode, in one pair.
    let scored = |min: f64| {
        let driver = &driver;
        async move {
            driver
                .recall(
                    "Rust",
                    1,
                    &OwnedRecallOpts {
                        namespace: Some("project".into()),
                        min_score: Some(min),
                        ..OwnedRecallOpts::default()
                    },
                    None,
                )
                .await
                .expect("recall")
                .len()
        }
    };
    assert_eq!(
        scored(0.1).await,
        1,
        "a scored hit above the threshold survives"
    );
    assert_eq!(
        scored(0.99).await,
        0,
        "a scored hit below the threshold drops"
    );
    assert!(driver.forget("project", "decision").await.expect("forget"));
    assert!(driver.health().await.is_usable());
}

/// Issue #69: a keyed read asks the backend for ONE namespace's tag — never
/// the whole-account tag walk the pre-seam reads ran.
#[tokio::test]
async fn keyed_reads_scope_to_one_container_tag() {
    let state = AppState::default();
    let app = Router::new()
        .route("/v3/container-tags/list", get(tags))
        .route("/v4/memories/list", post(list))
        .route("/v4/memories", post(add).patch(update).delete(remove))
        .route("/", get(|| async { StatusCode::OK }))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    let driver = crate::supermemory_provider(
        super::SupermemoryMemory::self_hosted(&endpoint, "secret").expect("client"),
    );
    driver
        .store(
            "project",
            "decision",
            "use Rust 2024",
            MemoryCategory::Core,
            None,
            MemoryTaint::Internal,
        )
        .await
        .expect("store");
    state.0.lock().expect("state lock").list_bodies.clear();

    driver.get("project", "decision").await.expect("get");
    let bodies = state.0.lock().expect("state lock").list_bodies.clone();
    assert_eq!(bodies.len(), 1, "one scoped list request, not a tag walk");
    let expected = super::SupermemoryDialect::container_tag("project");
    assert_eq!(
        bodies[0]["containerTags"],
        serde_json::json!([expected]),
        "the request names exactly the namespace's tag"
    );
}
