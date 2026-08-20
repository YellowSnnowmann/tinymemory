//! Mem0 adapter contract tests over its native HTTP shapes.

#![allow(clippy::expect_used)]

use std::sync::{Arc, Mutex};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post, put},
    Json, Router,
};
use serde_json::{json, Value};
use tinymemory_api::{
    provider::{MemoryCore, MemoryProvider, MemoryRecall},
    recall::OwnedRecallOpts,
    types::{MemoryCategory, MemoryTaint},
};

#[derive(Clone, Default)]
struct AppState(Arc<Mutex<Vec<Value>>>, Arc<Mutex<Vec<String>>>);

async fn list(
    State(state): State<AppState>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
) -> Json<Value> {
    let query = query.unwrap_or_default();
    state.1.lock().expect("query lock").push(query.clone());
    // Honour the user_id filter the way the OSS server does (issue #69): a
    // scoped request must not receive the whole store back.
    let user = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("user_id="))
        .map(str::to_owned);
    let rows = state.0.lock().expect("state lock").clone();
    let rows = match user {
        Some(ref encoded) => rows
            .into_iter()
            .filter(|row| {
                row["metadata"]["tinymemory_namespace"]
                    .as_str()
                    .map(|ns| super::percent_encode_query(ns) == *encoded)
                    == Some(true)
            })
            .collect(),
        None => rows,
    };
    Json(json!({"results": rows}))
}

async fn add(State(state): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    let mut records = state.0.lock().expect("state lock");
    let id = format!("mem-{}", records.len() + 1);
    records.push(json!({
        "id": id,
        "memory": body.pointer("/messages/0/content"),
        "metadata": body.get("metadata"),
        "created_at": "2026-08-12T00:00:00Z"
    }));
    Json(json!({"results": [{"id": id}]}))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> StatusCode {
    if let Some(record) = state
        .0
        .lock()
        .expect("state lock")
        .iter_mut()
        .find(|record| record["id"] == id)
    {
        record["memory"] = body["text"].clone();
        record["metadata"] = body["metadata"].clone();
    }
    StatusCode::OK
}

async fn remove(State(state): State<AppState>, Path(id): Path<String>) -> StatusCode {
    state
        .0
        .lock()
        .expect("state lock")
        .retain(|record| record["id"] != id);
    StatusCode::OK
}

async fn search(State(state): State<AppState>) -> Json<Value> {
    let mut records = state.0.lock().expect("state lock").clone();
    for record in &mut records {
        record["score"] = json!(0.9);
    }
    Json(json!({"results": records}))
}

#[tokio::test]
async fn native_mem0_round_trips_the_tinymemory_contract() {
    let state = AppState::default();
    let app = Router::new()
        .route("/memories", get(list).post(add))
        .route("/memories/{id}", put(update).delete(remove))
        .route("/search", post(search))
        .route("/api/health", get(|| async { StatusCode::OK }))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });

    let memory = super::Mem0Memory::new(&endpoint, None).expect("client");
    let driver = crate::mem0_provider(memory);
    tinymemory_api::provider::audit_provider(&driver).expect("honest capabilities");
    driver
        .store(
            "people",
            "alice",
            "likes tea",
            MemoryCategory::Core,
            Some("s1"),
            MemoryTaint::ExternalSync,
        )
        .await
        .expect("store");
    driver
        .store(
            "people",
            "alice",
            "likes coffee",
            MemoryCategory::Daily,
            Some("s2"),
            MemoryTaint::Internal,
        )
        .await
        .expect("upsert");
    let entry = driver
        .get("people", "alice")
        .await
        .expect("get")
        .expect("entry");
    assert_eq!(entry.content, "likes coffee");
    assert_eq!(entry.category, MemoryCategory::Daily);
    let hits = driver
        .recall(
            "coffee",
            2,
            &OwnedRecallOpts {
                namespace: Some("people".into()),
                ..OwnedRecallOpts::default()
            },
            None,
        )
        .await
        .expect("recall");
    assert_eq!(hits.len(), 1);
    assert!(driver.forget("people", "alice").await.expect("forget"));
    assert!(!driver
        .forget("people", "alice")
        .await
        .expect("forget again"));
    assert!(driver.health().await.is_usable());
}

/// Issue #69: a self-hosted keyed read scopes the listing to the namespace's
/// `user_id` — percent-encoded, since namespaces carry slashes — instead of
/// walking the whole store.
#[tokio::test]
async fn self_hosted_keyed_reads_scope_by_user_id() {
    let state = AppState::default();
    let app = Router::new()
        .route("/memories", get(list).post(add))
        .route("/memories/{id}", put(update).delete(remove))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    let driver = crate::mem0_provider(
        super::Mem0Memory::self_hosted(&endpoint, Some("token")).expect("client"),
    );
    driver
        .store(
            "oc/team a",
            "decision",
            "content",
            MemoryCategory::Core,
            None,
            MemoryTaint::Internal,
        )
        .await
        .expect("store");
    state.1.lock().expect("query lock").clear();

    let got = driver.get("oc/team a", "decision").await.expect("get");
    assert!(
        got.is_some(),
        "the scoped listing must still find the record"
    );
    let queries = state.1.lock().expect("query lock").clone();
    assert_eq!(queries.len(), 1, "one scoped request: {queries:?}");
    assert!(
        queries[0].contains("user_id=oc%2Fteam%20a"),
        "the namespace rides percent-encoded: {queries:?}"
    );
}

/// Issue #69: the hosted platform's keyed lookup is ONE filtered request
/// carrying the metadata key — and verify-after-resolve refuses a server
/// that answers with someone else's record instead of honoring the filter.
#[tokio::test]
async fn cloud_keyed_lookup_filters_by_metadata_and_verifies() {
    use axum::routing::post;
    let bodies: Arc<Mutex<Vec<Value>>> = Arc::default();
    let answer: Arc<Mutex<Value>> = Arc::default();
    let captured = bodies.clone();
    let served = answer.clone();
    let app = Router::new().route(
        "/v3/memories/",
        post(move |Json(body): Json<Value>| {
            let captured = captured.clone();
            let served = served.clone();
            async move {
                captured.lock().expect("bodies").push(body);
                Json(json!({"results": served.lock().expect("answer").clone(), "next": null}))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    let record = |ns: &str, key: &str| {
        json!({"id": "mem-1", "memory": "content", "metadata": {
            "tinymemory_namespace": ns,
            "tinymemory_key": key,
            "tinymemory_category": "core",
            "tinymemory_taint": "internal",
        }})
    };

    let driver = crate::mem0_provider(super::Mem0Memory::api(&endpoint, "key").expect("client"));
    *answer.lock().expect("answer") = json!([record("project", "decision")]);
    let got = driver.get("project", "decision").await.expect("get");
    assert!(got.is_some());
    let sent = bodies.lock().expect("bodies").clone();
    assert_eq!(sent.len(), 1, "one filtered request, no account walk");
    let clauses = sent[0]["filters"]["AND"].as_array().expect("AND clauses");
    assert!(
        clauses
            .iter()
            .any(|c| c["metadata"]["tinymemory_key"] == json!("decision")),
        "the filter carries the key: {sent:?}"
    );

    // A server that ignores the metadata clause answers with the whole
    // namespace: the asked-for record must still resolve even when a sibling
    // rides ahead of it in the page.
    *answer.lock().expect("answer") = json!([
        record("project", "someone-elses"),
        record("project", "decision"),
    ]);
    let got = driver
        .get("project", "decision")
        .await
        .expect("degraded-filter get")
        .expect("record present in the degraded page");
    assert_eq!(got.key, "decision", "the exact match wins, not the sibling");

    // A server answering ONLY foreign records: refuse loudly.
    *answer.lock().expect("answer") = json!([record("project", "someone-elses")]);
    let err = driver.get("project", "decision").await;
    assert!(
        err.is_err(),
        "a mismatched filtered answer must refuse, not serve another record"
    );

    // #71 review M2: a FULL page of records none of which even decode is
    // inconclusive, not absent — the record may sit past page 1 of a
    // filter-dropping server's account. Refuse rather than answer a
    // trustless `absent`.
    let junk: Vec<Value> = (0..200)
        .map(|n| json!({"id": format!("foreign-{n}"), "memory": "not ours", "metadata": {}}))
        .collect();
    *answer.lock().expect("answer") = json!(junk);
    let err = driver.get("project", "decision").await;
    assert!(
        err.is_err(),
        "a full undecodable page must refuse, not report absent"
    );

    // A SHORT page of undecodable records IS a trustworthy absent: the
    // server returned everything it had and ours was not among it.
    *answer.lock().expect("answer") =
        json!([{"id": "foreign-1", "memory": "not ours", "metadata": {}}]);
    let got = driver
        .get("project", "decision")
        .await
        .expect("short undecodable page");
    assert!(got.is_none(), "a short page proves absence");
}
