//! Task-local deterministic GitHub transport used by reader tests.

tokio::task_local! {
    static TEST_RESPONSES: std::cell::RefCell<
        std::collections::VecDeque<Result<String, String>>
    >;
}

/// Run a future with a task-local sequence of GitHub responses.
pub(crate) async fn with_test_responses<F>(
    responses: Vec<Result<String, String>>,
    future: F,
) -> F::Output
where
    F: std::future::Future,
{
    TEST_RESPONSES
        .scope(std::cell::RefCell::new(responses.into()), future)
        .await
}

/// Return the next deterministic response and fail closed when exhausted.
pub(crate) async fn fetch_github(api_path: &str, _use_gh: bool) -> Result<String, String> {
    TEST_RESPONSES
        .try_with(|responses| responses.borrow_mut().pop_front())
        .map_err(|_| format!("no deterministic GitHub transport installed for {api_path}"))?
        .unwrap_or_else(|| {
            Err(format!(
                "no deterministic GitHub response queued for {api_path}"
            ))
        })
}
