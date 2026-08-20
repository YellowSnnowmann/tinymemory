//! The suite, run against the drivers this workspace ships as references.
//!
//! Two drivers, for two different reasons.
//!
//! `InMemoryProvider` is the calibration subject: its behaviour is obvious by
//! inspection, so a failure here means the *assertion* is wrong, not the
//! driver. Without it, a suite that only ever ran against real engines could
//! not tell those two cases apart.
//!
//! `NullMemoryProvider` is the opposite end — it accepts writes, discards them,
//! and reads back empty. Running the same assertions against it pins down which
//! parts of the contract a discard-everything driver must still uphold
//! (namespace isolation, an honest `forget`, a terminating export cursor,
//! errors that stay inside `MemoryError`) and which are vacuous for it.
//! A suite that could not run against `null` would be asserting storage rather
//! than the contract.

use std::sync::Arc;

use tinymemory_api::null::NullMemoryProvider;
use tinymemory_api::provider::MemoryProvider;
use tinymemory_conformance::{assert_provider, InMemoryProvider};

#[tokio::test]
async fn the_in_memory_reference_driver_conforms() {
    assert_provider(Arc::new(InMemoryProvider::new())).await;
}

#[tokio::test]
async fn the_null_driver_conforms() {
    assert_provider(Arc::new(NullMemoryProvider::new())).await;
}

#[tokio::test]
async fn the_reference_driver_advertises_exactly_the_mandatory_families() {
    let provider = InMemoryProvider::new();
    let caps = provider.capabilities();
    assert_eq!(
        caps.len(),
        3,
        "the reference driver must advertise only what it can serve, got {caps:?}"
    );
    // Every optional accessor stays `None`, which is what makes the audit pass.
    assert!(provider.as_tree().is_none());
    assert!(provider.as_graph().is_none());
    assert!(provider.as_ingest().is_none());
}
