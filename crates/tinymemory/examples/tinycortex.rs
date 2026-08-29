//! The embedded engine, end to end: admit, construct, audit, store, recall,
//! and the same store read back through the section surface.
//!
//! Run with:
//!
//! ```sh
//! cargo run --example tinycortex --features tinycortex
//! ```
//!
//! `examples/basic.rs` teaches the binding *shape* with the null driver; this
//! one proves the first real engine binds the same way and actually retains.
//! The backend is the engine's own in-memory store — a complete embedded
//! setup for the mandatory three families: no workspace, no host seams. (The
//! full eighteen-family `TinycortexProvider` additionally needs the host
//! seams installed; `crates/tinymemory-tinycortex/tests/full_provider_conformance.rs`
//! is the minimal working wiring for that.)

use std::sync::Arc;

use tinymemory::api::provider::{audit_provider, MemoryProvider};
use tinymemory::api::recall::OwnedRecallOpts;
use tinymemory::api::types::{MemoryCategory, MemoryTaint};
use tinymemory::namespace::MemorySection;
use tinymemory::registry::{ConfigLabels, DriverRegistry, TINYCORTEX_DRIVER_ID};
use tinymemory::sections::Sections;
use tinymemory::tinycortex::{provider, InMemoryMemoryStore};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Admission first: is the id real, and may it answer for memory.
    let registry = DriverRegistry::builtin();
    let admission = registry.admit(TINYCORTEX_DRIVER_ID, None, ConfigLabels::default())?;
    println!("admitted '{}' as {:?}", admission.id, admission.class);

    // 2. Construction: the engine's simplest backend, wrapped as a driver.
    let provider: Arc<dyn MemoryProvider> =
        Arc::new(provider(Arc::new(InMemoryMemoryStore::new())));

    // 3. The capability audit: advertised must equal reachable.
    audit_provider(provider.as_ref())?;
    println!(
        "driver '{}' serves {} families",
        provider.driver_id(),
        provider.capabilities().iter().count()
    );

    // 4. Store and recall through the contract — no engine type in sight.
    provider
        .store(
            "example",
            "greeting",
            "the embedded engine says hello",
            MemoryCategory::Core,
            None,
            MemoryTaint::Internal,
        )
        .await?;
    let opts = OwnedRecallOpts {
        namespace: Some("example".into()),
        ..OwnedRecallOpts::default()
    };
    let hits = provider.recall("hello", 8, &opts, None).await?;
    println!("recall found {} entr(y/ies)", hits.len());
    assert!(!hits.is_empty(), "the stored entry must be recallable");

    // 5. The same engine through the section surface: the caller names a
    //    scope, never a namespace, and asks the whole section one question.
    let sections = Sections::new(provider.as_ref());
    for (scope, note) in [
        ("rust-async", "pinning is not unpinning"),
        ("rust-macros", "hygiene is per-expansion"),
    ] {
        let namespace = sections
            .learnings()
            .put(
                scope,
                "note",
                note,
                MemoryCategory::Core,
                None,
                MemoryTaint::Internal,
            )
            .await?;
        println!("learning stored in '{namespace}'");
    }

    let found = sections
        .recall()
        .across_section(
            &MemorySection::Learning,
            "is",
            8,
            &OwnedRecallOpts::default(),
            None,
        )
        .await?;
    println!(
        "section recall searched {} namespace(s) and found {} hit(s)",
        found.namespaces_searched,
        found.hits.len()
    );
    assert_eq!(found.namespaces_searched, 2, "both scopes must be searched");
    assert!(!found.hits.is_empty(), "the section recall must find them");
    Ok(())
}
