//! Local HTTP harness for exercising TinyMemory engines by hand.
//!
//! Not a host. It skips every policy layer a real host owns (tier
//! enforcement, taint stamping, redaction, egress checks) and exists purely so
//! a person can point a browser at a running server, pick an engine, and call
//! `store`/`recall`/`list`/`export` against it directly. See this crate's
//! `README.md` for how to run it.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tower_http::services::ServeDir;

use tinymemory_api::provider::types::SourceScope;
use tinymemory_api::provider::MemoryProvider;
use tinymemory_api::recall::OwnedRecallOpts;
use tinymemory_api::types::{MemoryCategory, MemoryTaint};

struct AppState {
    active: RwLock<Option<Arc<dyn MemoryProvider>>>,
}

type SharedState = Arc<AppState>;

/// A JSON-friendly wrapper around [`tinymemory_api::error::MemoryError`] and
/// this harness's own connection-state errors.
struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

impl From<tinymemory_api::error::MemoryError> for ApiError {
    fn from(err: tinymemory_api::error::MemoryError) -> Self {
        ApiError(StatusCode::BAD_GATEWAY, err.to_string())
    }
}

fn parse_category(value: &Option<String>) -> Result<Option<MemoryCategory>, ApiError> {
    value
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.parse::<MemoryCategory>()
                .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e))
        })
        .transpose()
}

fn parse_taint(value: &Option<String>) -> MemoryTaint {
    match value.as_deref() {
        Some("external_sync") => MemoryTaint::ExternalSync,
        _ => MemoryTaint::Internal,
    }
}

async fn current(state: &SharedState) -> Result<Arc<dyn MemoryProvider>, ApiError> {
    state
        .active
        .read()
        .await
        .clone()
        .ok_or_else(|| ApiError(StatusCode::CONFLICT, "no engine connected yet".into()))
}

#[derive(Deserialize)]
struct ConnectRequest {
    engine: String,
    #[serde(default)]
    deployment: Option<String>,
    #[serde(default)]
    endpoint: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
}

#[derive(Serialize, Clone)]
struct EngineStatus {
    connected: bool,
    driver_id: Option<String>,
    engine: Option<String>,
    has_graph: bool,
}

async fn connect(
    State(state): State<SharedState>,
    Json(req): Json<ConnectRequest>,
) -> Result<Json<EngineStatus>, ApiError> {
    let bad_request = |msg: &str| ApiError(StatusCode::BAD_REQUEST, msg.to_string());

    let provider: Arc<dyn MemoryProvider> = match req.engine.as_str() {
        "local" => {
            let memory: Arc<dyn tinymemory_tinycortex::tinycortex::memory::Memory> =
                Arc::new(tinymemory_tinycortex::InMemoryMemoryStore::new());
            Arc::new(tinymemory_tinycortex::provider(memory))
        }
        "supermemory" => {
            let endpoint = req
                .endpoint
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| bad_request("supermemory requires an endpoint URL"))?;
            let memory = tinymemory_remote::SupermemoryMemory::new(
                endpoint,
                req.api_key.as_deref().filter(|s| !s.is_empty()),
            )
            .map_err(|e| bad_request(&e.to_string()))?;
            Arc::new(tinymemory_remote::supermemory_provider(memory))
        }
        "mem0" => {
            let endpoint = req
                .endpoint
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| bad_request("mem0 requires an endpoint URL"))?;
            let api_key = req.api_key.as_deref().filter(|s| !s.is_empty());
            let is_cloud = match req.deployment.as_deref() {
                Some("cloud") => true,
                Some("self_hosted") => false,
                None => endpoint == tinymemory_remote::MEM0_API_ENDPOINT,
                Some(other) => {
                    return Err(bad_request(&format!("unknown Mem0 deployment: {other}")));
                }
            };
            let memory = if is_cloud {
                tinymemory_remote::Mem0Memory::api(
                    endpoint,
                    api_key.ok_or_else(|| bad_request("Mem0 Cloud requires an API key"))?,
                )
            } else {
                tinymemory_remote::Mem0Memory::new(endpoint, api_key)
            }
            .map_err(|e| bad_request(&e.to_string()))?;
            // Also advertises Graph via `Mem0Graph` — a client-side heuristic
            // over the same stored entries, not Mem0's native Graph Memory
            // (dropped from the self-hosted OSS package's 2.x line; see the
            // module docs on `Mem0Graph`).
            Arc::new(tinymemory_remote::mem0_graph_provider(memory))
        }
        "cognee" => {
            let endpoint = req
                .endpoint
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| bad_request("cognee requires an endpoint URL"))?;
            let api_key = req.api_key.as_deref().filter(|s| !s.is_empty());
            let is_cloud = match req.deployment.as_deref() {
                Some("cloud") => true,
                Some("self_hosted") | None => false,
                Some(other) => {
                    return Err(bad_request(&format!("unknown Cognee deployment: {other}")));
                }
            };
            let memory = if is_cloud {
                tinymemory_remote::CogneeMemory::api(
                    endpoint,
                    api_key.ok_or_else(|| bad_request("Cognee Cloud requires an API key"))?,
                )
            } else {
                tinymemory_remote::CogneeMemory::new(endpoint, api_key)
            }
            .map_err(|e| bad_request(&e.to_string()))?;
            // Cognee is graph-native, so its provider also advertises Graph
            // (relations only — see `CogneeGraph`'s docs for the exact split
            // between what's a real endpoint and what isn't).
            let provider = if is_cloud {
                tinymemory_remote::cognee_api_graph_provider(
                    memory,
                    endpoint,
                    api_key.unwrap_or_default(),
                )
            } else {
                tinymemory_remote::cognee_graph_provider(memory, endpoint, api_key)
            }
            .map_err(|e| bad_request(&e.to_string()))?;
            Arc::new(provider)
        }
        other => {
            return Err(bad_request(&format!("unknown engine: {other}")));
        }
    };

    let status = EngineStatus {
        connected: true,
        driver_id: Some(provider.driver_id().to_string()),
        engine: Some(req.engine),
        has_graph: provider.as_graph().is_some(),
    };
    *state.active.write().await = Some(provider);
    Ok(Json(status))
}

async fn disconnect(State(state): State<SharedState>) -> Json<EngineStatus> {
    *state.active.write().await = None;
    Json(EngineStatus {
        connected: false,
        driver_id: None,
        engine: None,
        has_graph: false,
    })
}

async fn status(State(state): State<SharedState>) -> Json<EngineStatus> {
    let guard = state.active.read().await;
    Json(EngineStatus {
        connected: guard.is_some(),
        driver_id: guard.as_ref().map(|p| p.driver_id().to_string()),
        engine: None,
        has_graph: guard.as_ref().is_some_and(|p| p.as_graph().is_some()),
    })
}

#[derive(Deserialize)]
struct StoreRequest {
    namespace: String,
    key: String,
    content: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    taint: Option<String>,
}

async fn store(
    State(state): State<SharedState>,
    Json(req): Json<StoreRequest>,
) -> Result<StatusCode, ApiError> {
    let provider = current(&state).await?;
    let category = parse_category(&req.category)?.unwrap_or(MemoryCategory::Core);
    let taint = parse_taint(&req.taint);
    provider
        .store(
            &req.namespace,
            &req.key,
            &req.content,
            category,
            req.session_id.as_deref(),
            taint,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct GetQuery {
    namespace: String,
    key: String,
}

async fn get_entry(
    State(state): State<SharedState>,
    Query(q): Query<GetQuery>,
) -> Result<Response, ApiError> {
    let provider = current(&state).await?;
    let entry = provider.get(&q.namespace, &q.key).await?;
    Ok(Json(entry).into_response())
}

#[derive(Deserialize)]
struct ForgetRequest {
    namespace: String,
    key: String,
}

async fn forget(
    State(state): State<SharedState>,
    Json(req): Json<ForgetRequest>,
) -> Result<Json<bool>, ApiError> {
    let provider = current(&state).await?;
    let existed = provider.forget(&req.namespace, &req.key).await?;
    Ok(Json(existed))
}

#[derive(Deserialize, Default)]
struct ListQuery {
    #[serde(default)]
    namespace: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
}

async fn list(
    State(state): State<SharedState>,
    Query(q): Query<ListQuery>,
) -> Result<Response, ApiError> {
    let provider = current(&state).await?;
    let category = parse_category(&q.category)?;
    let entries = provider
        .list(
            q.namespace.as_deref(),
            category.as_ref(),
            q.session_id.as_deref(),
        )
        .await?;
    Ok(Json(entries).into_response())
}

async fn namespaces(State(state): State<SharedState>) -> Result<Response, ApiError> {
    let provider = current(&state).await?;
    let namespaces = provider.namespaces().await?;
    Ok(Json(namespaces).into_response())
}

#[derive(Deserialize)]
struct RecallRequest {
    query: String,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    namespace: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    min_score: Option<f64>,
    #[serde(default)]
    cross_session: bool,
}

fn default_limit() -> usize {
    10
}

async fn recall(
    State(state): State<SharedState>,
    Json(req): Json<RecallRequest>,
) -> Result<Response, ApiError> {
    let provider = current(&state).await?;
    let category = parse_category(&req.category)?;
    let opts = OwnedRecallOpts {
        namespace: req.namespace,
        category,
        session_id: req.session_id,
        min_score: req.min_score,
        cross_session: req.cross_session,
    };
    let hits = provider
        .recall(&req.query, req.limit, &opts, None::<&SourceScope>)
        .await?;
    Ok(Json(hits).into_response())
}

#[derive(Deserialize, Default)]
struct ExportQuery {
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default = "default_export_limit")]
    limit: usize,
}

fn default_export_limit() -> usize {
    50
}

async fn export(
    State(state): State<SharedState>,
    Query(q): Query<ExportQuery>,
) -> Result<Response, ApiError> {
    let provider = current(&state).await?;
    let page = provider.export_page(q.cursor.as_deref(), q.limit).await?;
    Ok(Json(page).into_response())
}

#[derive(Deserialize, Default)]
struct GraphRelationsQuery {
    #[serde(default)]
    namespace: Option<String>,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    predicate: Option<String>,
    #[serde(default = "default_relations_limit")]
    limit: usize,
}

fn default_relations_limit() -> usize {
    100
}

async fn graph_relations(
    State(state): State<SharedState>,
    Query(q): Query<GraphRelationsQuery>,
) -> Result<Response, ApiError> {
    let provider = current(&state).await?;
    let graph = provider.as_graph().ok_or_else(|| {
        ApiError(
            StatusCode::NOT_IMPLEMENTED,
            "the connected engine does not advertise a graph".to_string(),
        )
    })?;
    let relations = graph
        .relations(
            q.namespace.as_deref(),
            q.subject.as_deref(),
            q.predicate.as_deref(),
            q.limit,
        )
        .await?;
    Ok(Json(relations).into_response())
}

#[tokio::main]
async fn main() {
    let state: SharedState = Arc::new(AppState {
        active: RwLock::new(None),
    });

    let web_dir = std::env::var("TINYMEMORY_TESTING_UI_WEB")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/web").to_string());

    let api = Router::new()
        .route("/connect", post(connect))
        .route("/disconnect", post(disconnect))
        .route("/status", get(status))
        .route("/store", post(store))
        .route("/get", get(get_entry))
        .route("/forget", post(forget))
        .route("/list", get(list))
        .route("/namespaces", get(namespaces))
        .route("/recall", post(recall))
        .route("/export", get(export))
        .route("/graph/relations", get(graph_relations))
        .with_state(state);

    let app = Router::new()
        .nest("/api", api)
        .fallback_service(ServeDir::new(web_dir));

    let addr: SocketAddr = std::env::var("TINYMEMORY_TESTING_UI_ADDR")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 4180)));

    println!("tinymemory testing UI listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind testing UI address");
    axum::serve(listener, app).await.expect("serve testing UI");
}
