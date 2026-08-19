//! What the hosted adapters do when the backend does not cooperate.
//!
//! Issue #18 §E6: "backend error, timeout, and partial-page responses on every
//! remote adapter — currently zero coverage". The existing per-adapter tests all
//! drive a backend that answers correctly, which is the half that was never in
//! doubt.
//!
//! The assertion that matters is not which error comes back — the contract's
//! error type is still `anyhow` under `MemoryError::Other` here, and §A4 is what
//! makes "unsupported" distinguishable from "failed". It is that a failure comes
//! back **at all**.
//!
//! A read that answers `Ok(None)` when the backend returned 500 is saying "this
//! memory does not exist" when the truth is "I could not ask". A caller cannot
//! tell those apart, so it writes the memory again, or reports to a user that
//! their memory is gone, or — worst — a sync job treats the empty read as
//! authoritative and prunes. Nothing surfaces until much later, which is exactly
//! the failure mode that keeps `OPENCOMPANY_MEMORY=remote` gated downstream.
//!
//! Each test drives a real adapter over a real TCP socket against a double that
//! misbehaves in one specific way, matching the harness the happy-path tests
//! already use.

#![allow(clippy::expect_used, clippy::panic)]

use axum::http::StatusCode;
use axum::routing::{any, get};
use axum::Router;
use tinymemory_api::provider::MemoryProvider;
use tinymemory_api::recall::RecallOpts;
use tinymemory_api::traits::Memory;
use tinymemory_api::types::{MemoryCategory, MemoryTaint};

use crate::{CogneeMemory, Mem0Memory, SupermemoryMemory};

/// Serves `app` on an ephemeral port and returns its base URL.
async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    endpoint
}

/// A backend that fails every route with `status`.
async fn failing(status: StatusCode) -> String {
    serve(Router::new().fallback(any(move || async move { status }))).await
}

/// A backend that answers every route with `200 OK` and a body that is not the
/// JSON the adapter expects.
///
/// Distinct from an HTTP failure: the transport succeeded, so an adapter that
/// only checks the status code reaches its deserializer with rubbish.
async fn malformed() -> String {
    serve(Router::new().fallback(any(|| async { "this is not the JSON you asked for" }))).await
}

/// Every adapter, as a `Memory`, built against `endpoint`.
///
/// Boxed rather than generic so each assertion below is written once and run
/// three times — the point is that no adapter is exempt.
fn adapters(endpoint: &str) -> Vec<(&'static str, Box<dyn Memory>)> {
    vec![
        (
            "supermemory",
            Box::new(SupermemoryMemory::new(endpoint, None).expect("client")) as Box<dyn Memory>,
        ),
        (
            "mem0",
            Box::new(Mem0Memory::new(endpoint, None).expect("client")),
        ),
        (
            "cognee",
            Box::new(CogneeMemory::self_hosted(endpoint, None).expect("client")),
        ),
    ]
}

#[tokio::test]
async fn a_backend_failure_on_write_is_reported_rather_than_swallowed() {
    let endpoint = failing(StatusCode::INTERNAL_SERVER_ERROR).await;
    for (name, memory) in adapters(&endpoint) {
        let result = memory
            .store_with_taint(
                "ns",
                "k",
                "content",
                MemoryCategory::Core,
                None,
                MemoryTaint::Internal,
            )
            .await;
        assert!(
            result.is_err(),
            "{name}: a 500 on write must not report success — a caller that \
             believes the write landed has no reason to retry it"
        );
    }
}

#[tokio::test]
async fn a_backend_failure_on_read_is_not_reported_as_absence() {
    // The one that matters most. `Ok(None)` here means "no such memory", and
    // the truth is "the backend is down".
    let endpoint = failing(StatusCode::INTERNAL_SERVER_ERROR).await;
    for (name, memory) in adapters(&endpoint) {
        let result = memory.get("ns", "k").await;
        let Err(error) = result else {
            panic!(
                "{name}: a 500 on read must not be laundered into `Ok(None)` — \
                 'I could not ask' and 'it is not there' are different answers"
            );
        };
        // Assert the failure is the *backend's*, not something incidental like a
        // malformed URL. Without this the test would pass for the wrong reason
        // if the adapter never reached the network at all.
        let rendered = format!("{error:#}");
        assert!(
            rendered.contains("500"),
            "{name}: expected the backend status to survive into the error, got: {rendered}"
        );
    }
}

#[tokio::test]
async fn an_unauthorized_backend_is_not_reported_as_an_empty_store() {
    // A wrong or expired credential is the most likely failure in production,
    // and the most dangerous one to render as "you have no memories".
    let endpoint = failing(StatusCode::UNAUTHORIZED).await;
    for (name, memory) in adapters(&endpoint) {
        let listed = memory.list(None, None, None).await;
        assert!(
            listed.is_err(),
            "{name}: a 401 must not present as an empty result set"
        );

        let recalled = memory.recall("anything", 10, RecallOpts::default()).await;
        assert!(
            recalled.is_err(),
            "{name}: a 401 on recall must not present as no matches"
        );
    }
}

#[tokio::test]
async fn a_malformed_backend_response_is_an_error_and_not_a_panic() {
    // `200 OK` with a body the adapter cannot parse. An adapter that unwraps
    // its way through deserialization takes the caller's process down.
    let endpoint = malformed().await;
    for (name, memory) in adapters(&endpoint) {
        let result = memory.get("ns", "k").await;
        assert!(
            result.is_err(),
            "{name}: an unparseable 200 body must surface as an error"
        );
    }
}

#[tokio::test]
async fn an_unreachable_backend_is_reported_rather_than_hanging() {
    // Nothing is listening. This is the timeout/connection-refused leg of §E6.
    // Bind a port, learn its number, drop the listener: the address is now
    // reliably closed rather than merely unlikely to be in use.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    drop(listener);

    for (name, memory) in adapters(&endpoint) {
        let result = memory.get("ns", "k").await;
        assert!(
            result.is_err(),
            "{name}: an unreachable backend must surface as an error"
        );
    }
}

#[tokio::test]
async fn a_cursor_that_never_clears_is_refused_rather_than_walked_for_ever() {
    // Mem0's hosted arm pages until the server says stop: an empty page or a
    // null `next`. Both are things the *server* controls, so a server that
    // keeps answering a page and a cursor -- a bug, a proxy replaying one
    // response, a filter that never narrows -- would spin the request loop and
    // grow the buffer until the process died. The self-hosted arm already
    // refuses past its ceiling; this pins the hosted one doing the same.
    let app = Router::new().fallback(any(|| async {
        axum::Json(serde_json::json!({
            "count": 1,
            "next": "https://api.mem0.ai/v3/memories/?page=2",
            "previous": null,
            "results": [{"id": "m-1", "memory": "x", "metadata": {}}]
        }))
    }));
    let endpoint = serve(app).await;
    let memory = Mem0Memory::api(&endpoint, "m0-test-key").expect("client");

    // Bounded so a genuinely unbounded loop fails the test rather than hanging
    // the suite: the ceiling is 500 requests against a local socket, which
    // finishes far inside this.
    let outcome =
        tokio::time::timeout(std::time::Duration::from_secs(60), memory.get("ns", "k")).await;

    let Ok(result) = outcome else {
        panic!("the hosted listing never terminated against a cursor that never clears");
    };
    let error = result.expect_err("a cursor that never clears cannot be answered correctly");
    assert!(
        format!("{error:#}").contains("pages"),
        "the refusal must name the page ceiling it hit, got: {error:#}"
    );
}

#[tokio::test]
async fn a_paginated_export_terminates_instead_of_looping() {
    // The partial-page leg of §E6. A backend that keeps answering with a page
    // and a cursor would spin an exporter forever; the contract terminates on
    // `next_cursor: None`, and a driver that never emits one never finishes.
    //
    // Driven through the bound provider rather than the raw `Memory`:
    // `export_page` is a `MemoryPortability` method, and portability is a
    // mandatory supertrait of `MemoryProvider`, so it is always callable — which
    // is exactly why a non-terminating one is worth pinning.
    let app = Router::new().fallback(get(|| async {
        axum::Json(serde_json::json!({
            "memoryEntries": [],
            "results": [],
            "data": [],
            "pagination": {"totalPages": 1}
        }))
    }));
    let endpoint = serve(app).await;

    let providers: Vec<(&str, Box<dyn MemoryProvider>)> = vec![
        (
            "supermemory",
            Box::new(crate::supermemory_provider(
                SupermemoryMemory::new(&endpoint, None).expect("client"),
            )) as Box<dyn MemoryProvider>,
        ),
        (
            "mem0",
            Box::new(crate::mem0_provider(
                Mem0Memory::new(&endpoint, None).expect("client"),
            )),
        ),
        (
            "cognee",
            Box::new(crate::cognee_provider(
                CogneeMemory::self_hosted(&endpoint, None).expect("client"),
            )),
        ),
    ];

    for (name, provider) in providers {
        // Bounded so a non-terminating implementation fails rather than hanging
        // the whole suite.
        let finished = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            let mut cursor: Option<String> = None;
            for page_number in 0..100usize {
                let Ok(page) = provider.export_page(cursor.as_deref(), 100).await else {
                    // An error is an acceptable answer here; a hang is not.
                    return true;
                };
                match page.next_cursor {
                    None => return true,
                    Some(next) => cursor = Some(next),
                }
                let _ = page_number;
            }
            false
        })
        .await;
        assert_eq!(
            finished,
            Ok(true),
            "{name}: export_page never terminated — an exporter would spin here"
        );
    }
}
