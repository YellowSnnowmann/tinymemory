//! Bind a memory driver the way a host does: admit, then construct, then use.
//!
//! Run with:
//!
//! ```sh
//! cargo run --example basic
//! ```
//!
//! This uses the null driver so it needs no engine, no workspace, and no
//! network — the point is the *shape* of binding, which is identical for a real
//! engine. Swap `NullMemoryProvider` for an adapter's provider and nothing else
//! here changes.
//!
//! The order matters and is the reason this example exists. A host does not
//! construct a driver and then ask whether it was allowed; it admits an id
//! first, and only then builds the thing. Admission is engine-neutral and
//! answers one question — *is this driver id real, and may it answer for
//! memory* — while construction needs everything an engine needs.

use std::sync::Arc;

use tinymemory::api::null::NullMemoryProvider;
use tinymemory::api::provider::{audit_provider, MemoryProvider};
use tinymemory::api::types::{MemoryCategory, MemoryTaint, GLOBAL_NAMESPACE};
use tinymemory::registry::{ConfigLabels, DriverRegistry, NULL_DRIVER_ID};
use tinymemory::CONTRACT_VERSION;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("contract version: {CONTRACT_VERSION:?}");

    // 1. Admission. The host names a driver; the registry decides whether it is
    //    real and what class it binds as. A reserved embedded or null id needs
    //    no configuration entry, which is what lets an unconfigured host boot.
    let registry = DriverRegistry::builtin();
    let admission = registry.admit(NULL_DRIVER_ID, None, ConfigLabels::default())?;
    println!("admitted '{}' as {:?}", admission.id, admission.class);

    // 2. Construction. The host's job, not the registry's — see
    //    `tinymemory::registry`'s module docs for why the two are separate.
    let provider: Arc<dyn MemoryProvider> = Arc::new(NullMemoryProvider::new());

    // 3. Negotiation. `audit_provider` checks the driver advertises exactly the
    //    families it can actually serve. A driver whose capability set overstates
    //    its accessors would let a host register RPC methods that answer errors.
    audit_provider(provider.as_ref())?;
    // `Capabilities` is a set, not a string — render it by walking it, which is
    // also how a host filters its RPC surface from the negotiated set.
    let families: Vec<&str> = provider
        .capabilities()
        .iter()
        .map(tinymemory::capabilities::Capability::as_str)
        .collect();
    println!(
        "driver '{}' serves {} families: {}",
        provider.driver_id(),
        families.len(),
        families.join(", ")
    );

    // 4. Use. Every driver serves the three mandatory families, so this much
    //    works against any of them.
    provider
        .store(
            GLOBAL_NAMESPACE,
            "greeting",
            "hello from the basic example",
            MemoryCategory::Core,
            None,
            MemoryTaint::Internal,
        )
        .await?;

    // The null driver accepts writes and discards them — `/dev/null` semantics,
    // a legitimate binding for a deployment that wants the ports wired and
    // nothing retained. Reading back nothing here is correct, not a failure.
    match provider.get(GLOBAL_NAMESPACE, "greeting").await? {
        Some(entry) => println!("read back: {}", entry.content),
        None => println!("read back: nothing — the null driver retains no writes"),
    }

    Ok(())
}
