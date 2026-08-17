//! Capability negotiation: what a host may trust a driver's advertisement for,
//! and what happens when the advertisement is wrong.
//!
//! The contract's premise is that a host negotiates once at bind time and then
//! filters its own surface from the cached set. That is only safe if the set is
//! honest, which is what `audit_provider` is for — so these tests pin both the
//! honest path and the dishonest one.

// A failing assertion in a test *is* a panic; the crate-wide `expect_used` /
// `unwrap_used` / `panic` lints exist to keep the library from panicking, not
// the tests. Same allowance, and same reasoning, as `src/registry/test.rs`.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use tinymemory::api::capabilities::{Capabilities, Capability};
use tinymemory::api::health::MemoryHealth;
use tinymemory::api::null::NullMemoryProvider;
use tinymemory::api::provider::{audit_provider, MemoryProvider, MemoryTree};
use tinymemory_conformance::InMemoryProvider;

#[test]
fn the_reference_drivers_advertise_exactly_what_they_reach() {
    for provider in [
        Arc::new(InMemoryProvider::new()) as Arc<dyn MemoryProvider>,
        Arc::new(NullMemoryProvider::new()),
    ] {
        assert!(
            audit_provider(provider.as_ref()).is_ok(),
            "driver `{}` failed its audit",
            provider.driver_id()
        );
    }
}

#[test]
fn a_host_can_filter_its_surface_from_the_cached_capability_set() {
    // This is the whole point of negotiating once: a host reads the set at bind
    // time and never asks again, so the set has to answer both directions.
    let provider = InMemoryProvider::new();
    let caps = provider.capabilities();

    for mandatory in Capability::MANDATORY {
        assert!(
            caps.contains(mandatory),
            "{} must be advertised",
            mandatory.as_str()
        );
        assert!(
            provider.provides(mandatory),
            "{} must be reachable",
            mandatory.as_str()
        );
    }

    // An optional family this driver does not serve is absent from the set AND
    // unreachable through its accessor. A host that registered an RPC method
    // from the set alone would otherwise expose a method that answers errors.
    assert!(!caps.contains(Capability::Tree));
    assert!(provider.as_tree().is_none());
    assert!(!provider.provides(Capability::Tree));
}

/// A driver that claims a family it cannot serve.
///
/// Exists to prove the audit catches it. This is the failure mode the audit was
/// written for: the claim is cheap to make and, without a check, only surfaces
/// on the first call — which for a memory family may be days later, on a path
/// nobody is watching.
#[derive(Debug, Default)]
struct LyingProvider(InMemoryProvider);

#[async_trait::async_trait]
impl tinymemory::api::provider::MemoryCore for LyingProvider {
    async fn store(
        &self,
        namespace: &str,
        key: &str,
        content: &str,
        category: tinymemory::types::MemoryCategory,
        session_id: Option<&str>,
        taint: tinymemory::types::MemoryTaint,
    ) -> Result<(), tinymemory::error::MemoryError> {
        self.0
            .store(namespace, key, content, category, session_id, taint)
            .await
    }
    async fn get(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<tinymemory::types::MemoryEntry>, tinymemory::error::MemoryError> {
        self.0.get(namespace, key).await
    }
    async fn forget(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<bool, tinymemory::error::MemoryError> {
        self.0.forget(namespace, key).await
    }
    async fn list(
        &self,
        namespace: Option<&str>,
        category: Option<&tinymemory::types::MemoryCategory>,
        session_id: Option<&str>,
    ) -> Result<Vec<tinymemory::types::MemoryEntry>, tinymemory::error::MemoryError> {
        self.0.list(namespace, category, session_id).await
    }
    async fn namespaces(
        &self,
    ) -> Result<Vec<tinymemory::types::NamespaceSummary>, tinymemory::error::MemoryError> {
        self.0.namespaces().await
    }
}

#[async_trait::async_trait]
impl tinymemory::api::provider::MemoryRecall for LyingProvider {
    async fn recall(
        &self,
        query: &str,
        limit: usize,
        opts: &tinymemory::recall::OwnedRecallOpts,
        scope: Option<&tinymemory::api::provider::SourceScope>,
    ) -> Result<Vec<tinymemory::types::MemoryEntry>, tinymemory::error::MemoryError> {
        self.0.recall(query, limit, opts, scope).await
    }
}

#[async_trait::async_trait]
impl tinymemory::api::provider::MemoryPortability for LyingProvider {
    async fn export_page(
        &self,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<tinymemory::api::provider::ExportPage, tinymemory::error::MemoryError> {
        self.0.export_page(cursor, limit).await
    }
    async fn import_records(
        &self,
        records: Vec<tinymemory::api::provider::ExportRecord>,
    ) -> Result<tinymemory::api::provider::ImportOutcome, tinymemory::error::MemoryError> {
        self.0.import_records(records).await
    }
}

#[async_trait::async_trait]
impl MemoryProvider for LyingProvider {
    fn driver_id(&self) -> &'static str {
        "liar"
    }

    fn capabilities(&self) -> Capabilities {
        // Claims a summary tree it has no accessor for.
        Capabilities::mandatory().with(Capability::Tree)
    }

    async fn health(&self) -> MemoryHealth {
        MemoryHealth::Ready
    }

    // `as_tree` deliberately left at its `None` default.
}

#[test]
fn a_driver_that_advertises_a_family_it_cannot_serve_fails_the_audit() {
    let liar = LyingProvider::default();
    let audit = audit_provider(&liar).expect_err("the audit must catch an overstated capability");
    assert!(
        audit.advertised_but_absent.contains(&Capability::Tree),
        "the audit should name the family: {audit:?}"
    );
    assert!(
        audit.present_but_unadvertised.is_empty(),
        "nothing was under-advertised here: {audit:?}"
    );
}

#[test]
fn the_audit_failure_renders_something_an_operator_can_act_on() {
    let audit = audit_provider(&LyingProvider::default())
        .expect_err("the audit must fail")
        .to_string();
    assert!(
        audit.contains("tree"),
        "the message should name the family: {audit}"
    );
}

/// Compile-time proof that `as_tree` returning `Some` is what "reachable"
/// means, so the audit is checking the accessor and not a second declaration.
#[test]
fn reachability_is_the_accessor_not_a_second_declaration() {
    let provider = InMemoryProvider::new();
    let tree: Option<&dyn MemoryTree> = provider.as_tree();
    assert!(tree.is_none());
}
