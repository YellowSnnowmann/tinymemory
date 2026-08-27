//! Tests for the surrounding module.

use super::*;

/// 4xx is permanent: an invalid key must fail once, not retry with
/// backoff. Only rate-limit/upstream statuses and transport failures
/// (connect/read errors, timeouts) are worth another attempt.
#[test]
fn retry_classification_is_by_status_not_by_substring() {
    let retry = |m: &str| retryable_transport_error(&anyhow::anyhow!("{m}"));
    assert!(retry(
        "Composio direct request failed with HTTP 429 Too Many Requests"
    ));
    assert!(retry(
        "Composio proxy request failed with HTTP 503 Service Unavailable"
    ));
    assert!(retry("Composio direct transport error: connection reset"));
    assert!(!retry(
        "Composio direct request failed with HTTP 401 Unauthorized"
    ));
    assert!(!retry(
        "Composio proxy request failed with HTTP 404 Not Found"
    ));
    assert!(!retry(
        "Composio direct request failed with HTTP 400 Bad Request"
    ));
}

/// An error payload is a failure even when the flag is absent or true.
#[test]
fn an_error_payload_is_never_a_success() {
    let r = decode_direct_response(serde_json::json!({"error": "quota exceeded"}));
    assert!(!r.successful, "missing flag + error must be a failure");
    assert_eq!(r.error.as_deref(), Some("quota exceeded"));

    let r = decode_direct_response(serde_json::json!({"successful": true, "error": " boom "}));
    assert!(!r.successful, "flag=true + error must still be a failure");
    assert_eq!(r.error.as_deref(), Some("boom"));

    let r = decode_direct_response(
        serde_json::json!({"successful": true, "error": "  ", "data": {"x": 1}}),
    );
    assert!(r.successful, "an empty error string is no error");
    assert!(r.error.is_none());
    assert_eq!(r.data["x"], 1);
}

/// The client is built with finite timeouts; a build failure must not
/// silently degrade to an untimed client.
#[test]
fn client_builds_with_timeouts() {
    let _ = ComposioClient::new(ComposioSyncConfig::default());
    assert!(CONNECT_TIMEOUT < REQUEST_TIMEOUT);
}

#[test]
fn proxied_backend_envelope_decodes_provider_response() {
    let response = decode_proxy_response(serde_json::json!({
        "success": true,
        "data": {
            "successful": true,
            "data": {"messages": [{"messageId": "message-1"}]},
            "error": null
        }
    }))
    .unwrap();

    assert!(response.successful);
    assert_eq!(response.data["messages"][0]["messageId"], "message-1");
}

#[test]
fn flat_proxy_response_remains_supported() {
    let response = decode_proxy_response(serde_json::json!({
        "successful": true,
        "data": {"items": [1]}
    }))
    .unwrap();

    assert!(response.successful);
    assert_eq!(response.data["items"], serde_json::json!([1]));
}

/// The failure message now carries the response body, so a body that merely
/// mentions another status must not turn a permanent failure into a retry.
/// This is the hazard the needles were tightened against.
#[test]
fn a_surfaced_body_cannot_forge_a_retryable_status() {
    let retry = |m: &str| retryable_transport_error(&anyhow::anyhow!("{m}"));
    assert!(!retry(
        "Composio direct request failed with HTTP 400 Bad Request: upstream said HTTP 503"
    ));
    assert!(!retry(
        "Composio proxy request failed with HTTP 401 Unauthorized: retry after HTTP 429"
    ));
    // The real ones still classify.
    assert!(retry(
        "Composio direct request failed with HTTP 429 Too Many Requests: slow down"
    ));
}

/// Composio's structured error names the problem and how to fix it. This is the
/// payload from the report, verbatim.
#[test]
fn a_structured_error_body_reaches_the_message() {
    let body = r#"{"error":{"message":"Connected account user ID does not match the provided user ID.","code":1812,"slug":"ActionExecute_ConnectedAccountEntityIdMismatch","status":400,"suggested_fix":"The connected_account_id you provided belongs to a different entity."}}"#;
    let message = describe_failure("direct", reqwest::StatusCode::BAD_REQUEST, body);

    assert!(
        message.starts_with("Composio direct request failed with HTTP 400"),
        "the status clause must stay first and verbatim: {message}"
    );
    assert!(message.contains("Connected account user ID does not match"));
    assert!(message.contains("ActionExecute_ConnectedAccountEntityIdMismatch"));
    assert!(
        message.contains("belongs to a different entity"),
        "the suggested fix is the part that turns a dead end into an action: {message}"
    );
}

/// A bare `{"error": "..."}` string body is the other shape Composio returns.
#[test]
fn a_bare_error_string_body_reaches_the_message() {
    let body = r#"{"error":"You have exceeded your credits limit.","tag":"NO_MORE_CREDITS"}"#;
    let message = describe_failure("proxy", reqwest::StatusCode::PAYMENT_REQUIRED, body);
    assert!(message.contains("exceeded your credits limit"), "{message}");
}

/// An unrecognised body is echoed but bounded, so an unexpected payload cannot
/// pour arbitrary content into the logs.
#[test]
fn an_unrecognised_body_is_truncated() {
    let body = "x".repeat(5_000);
    let message = describe_failure("direct", reqwest::StatusCode::BAD_GATEWAY, &body);
    assert!(message.contains('…'), "expected an elision marker: {message}");
    assert!(
        message.chars().count() < 600,
        "the message grew to {} chars",
        message.chars().count()
    );
}

/// Truncation must cut on a char boundary — a multi-byte body must not panic
/// the error path.
#[test]
fn truncation_survives_multibyte_bodies() {
    let body = "é".repeat(5_000);
    let message = describe_failure("direct", reqwest::StatusCode::BAD_GATEWAY, &body);
    assert!(message.contains('…'), "{message}");
}

/// No body, no change: the status line stands on its own as before.
#[test]
fn an_empty_body_leaves_the_status_line_alone() {
    let message = describe_failure("direct", reqwest::StatusCode::NOT_FOUND, "   ");
    assert_eq!(message, "Composio direct request failed with HTTP 404 Not Found");
}
