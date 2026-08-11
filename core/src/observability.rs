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
///
/// Generic over `Display` exactly like the host's own `report_error`, and
/// rendered with `{:#}` so an `anyhow::Error` carries its full context chain
/// across the seam rather than just its outermost message.
pub fn report_error<E: std::fmt::Display + ?Sized>(
    error: &E,
    domain: &str,
    operation: &str,
    tags: &[(&str, &str)],
) {
    let rendered = format!("{error:#}");
    match error_reporter() {
        Some(reporter) => reporter.report_error(&rendered, domain, operation, tags),
        None => log::debug!(
            "[memory:observability] dropped report (no reporter installed) \
             domain={domain} operation={operation}: {rendered}"
        ),
    }
}

/// Report `error`, letting the host classify defect vs expected failure. A
/// no-op beyond logging when nothing is installed.
pub fn report_error_or_expected<E: std::fmt::Display + ?Sized>(
    error: &E,
    domain: &str,
    operation: &str,
    tags: &[(&str, &str)],
) {
    let rendered = format!("{error:#}");
    match error_reporter() {
        Some(reporter) => reporter.report_error_or_expected(&rendered, domain, operation, tags),
        None => log::debug!(
            "[memory:observability] dropped classified report (no reporter installed) \
             domain={domain} operation={operation}: {rendered}"
        ),
    }
}
