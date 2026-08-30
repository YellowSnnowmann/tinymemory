//! Tests for the TinyInference embedding adapters.

use std::sync::{Arc, Mutex};

use axum::{extract::State, routing::post, Json, Router};
use serde_json::{json, Value};
use tinyinference::embeddings::EmbeddingModel;

use super::{LongContextOllamaEmbeddingModel, RECOMMENDED_OLLAMA_CONTEXT_TOKENS};

#[tokio::test]
async fn ollama_requests_preserve_the_long_context_window() {
    let captured = Arc::new(Mutex::new(None));
    let state = Arc::clone(&captured);
    let app = Router::new()
        .route(
            "/api/embed",
            post(
                |State(captured): State<Arc<Mutex<Option<Value>>>>,
                 Json(body): Json<Value>| async move {
                    *captured.lock().expect("capture request body") = Some(body);
                    Json(json!({ "embeddings": [vec![0.0_f32; 1024]] }))
                },
            ),
        )
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock Ollama server");
    let address = listener.local_addr().expect("read mock server address");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve mock Ollama response");
    });

    let model = LongContextOllamaEmbeddingModel::try_new(
        &format!("http://{address}"),
        "bge-m3",
        1024,
        reqwest::Client::new(),
    )
    .expect("build Ollama model");
    let vectors = model
        .embed(&["long memory document".to_owned()])
        .await
        .expect("embed through mock Ollama");

    assert_eq!(vectors.len(), 1);
    let body = captured
        .lock()
        .expect("read captured request")
        .take()
        .expect("request was captured");
    assert_eq!(
        body["options"],
        json!({
            "num_ctx": RECOMMENDED_OLLAMA_CONTEXT_TOKENS,
            "num_batch": RECOMMENDED_OLLAMA_CONTEXT_TOKENS,
        })
    );
}
