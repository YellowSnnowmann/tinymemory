//! The process-global [`ErrorReporter`], and the `report_error` /
//! `report_error_or_expected` the extracted code calls in place of the host's
//! `core::observability`.
//!
//! Same global shape and same rationale as [`crate::events`] — including the
//! default. With no reporter installed the report is dropped after a local log
//! line, because every call site here is already handling the failure it is
//! reporting; telemetry is the side effect, not the recovery.

use std::sync::Arc;

use parking_lot::RwLock;

pub use tinymemory_api::host::ErrorReporter;

static REPORTER: RwLock<Option<Arc<dyn ErrorReporter>>> = RwLock::new(None);

/// Install the host's error reporter. Called once during startup wiring.
pub fn set_error_reporter(reporter: Arc<dyn ErrorReporter>) {
    *REPORTER.write() = Some(reporter);
}

/// Remove any installed reporter. For tests.
pub fn clear_error_reporter() {
    *REPORTER.write() = None;
}

/// The installed reporter, or `None` when no host has wired one up.
#[must_use]
pub fn error_reporter() -> Option<Arc<dyn ErrorReporter>> {
    REPORTER.read().clone()
}

/// Report `error` as a defect. A no-op beyond logging when nothing is installed.
pub fn report_error(error: &anyhow::Error, domain: &str, operation: &str, tags: &[(&str, &str)]) {
    match error_reporter() {
        Some(reporter) => reporter.report_error(error, domain, operation, tags),
        None => log::debug!(
            "[memory:observability] dropped report (no reporter installed) \
             domain={domain} operation={operation}: {error:#}"
        ),
    }
}

/// Report `error`, letting the host classify defect vs expected failure. A
/// no-op beyond logging when nothing is installed.
pub fn report_error_or_expected(
    error: &anyhow::Error,
    domain: &str,
    operation: &str,
    tags: &[(&str, &str)],
) {
    match error_reporter() {
        Some(reporter) => reporter.report_error_or_expected(error, domain, operation, tags),
        None => log::debug!(
            "[memory:observability] dropped classified report (no reporter installed) \
             domain={domain} operation={operation}: {error:#}"
        ),
    }
}
