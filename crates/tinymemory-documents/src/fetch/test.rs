//! Tests for URL intake.
//!
//! Only the guard and the argument handling are exercised here. Anything that
//! would actually reach the network is out of scope by the repository's testing
//! rules — the fetch itself is covered by `tinymemory-sources`' own reader
//! tests, which own the client this module borrows.

use super::*;

#[tokio::test]
async fn a_malformed_url_is_rejected_before_anything_is_fetched() {
    let error = fetch_url("not a url").await.unwrap_err();
    assert!(matches!(error, MemoryError::Invalid(_)), "got {error:?}");
    assert!(error.to_string().contains("invalid url"), "got {error}");
}

#[tokio::test]
async fn loopback_and_link_local_targets_are_refused() {
    for url in [
        "http://127.0.0.1/",
        "http://localhost:6379/",
        "http://169.254.169.254/latest/meta-data/",
        "http://[::1]/",
    ] {
        let error = fetch_url(url).await.unwrap_err();
        assert!(
            error.to_string().contains("not an allowed fetch target"),
            "{url} gave {error}"
        );
    }
}

#[tokio::test]
async fn a_non_http_scheme_is_refused() {
    for url in [
        "file:///etc/passwd",
        "ftp://example.com/x",
        "gopher://example.com/",
    ] {
        let error = fetch_url(url).await.unwrap_err();
        assert!(
            matches!(error, MemoryError::Invalid(_)),
            "{url} gave {error:?}"
        );
    }
}

#[test]
fn a_size_limit_failure_is_reported_as_budget_exceeded() {
    let error = read_error(
        "https://example.com/",
        "response body exceeds 8-byte limit (Content-Length=9)",
    );
    assert!(matches!(error, MemoryError::BudgetExceeded(_)), "got {error:?}");
}

#[test]
fn an_interrupted_read_is_reported_as_unreachable_not_budget_exceeded() {
    let error = read_error(
        "https://example.com/",
        "failed to read response body: connection reset",
    );
    assert!(matches!(error, MemoryError::Unreachable(_)), "got {error:?}");
}
