//! TinyCortex as a TinyMemory driver.
//!
//! This crate is the seam between the TinyCortex engine and the TinyMemory
//! contract.
//!
//! It used to carry a conversion layer as well. The two crates described the
//! same values under two names, so every call across the seam translated, and a
//! field added to one contract had to be added to the other and to the
//! conversion — three places, or the value was silently dropped. Since
//! issue #18 §A1 `tinycortex-api` re-exports `tinymemory-api` rather than
//! redefining it, so both sides name one type and `convert` is gone. What
//! remains is the trait shape: the engine's storage trait and the contract's
//! are still separate traits over the same values.
//!
//! ## What is here
//! - [`TinycortexMemory`] — wraps any TinyCortex [`tinycortex::memory::Memory`]
//!   backend as a TinyMemory
//!   [`Memory`](tinymemory_api::traits::Memory).
//! - [`provider`] — the one call that turns a TinyCortex backend into a
//!   mandatory-only driver, by pairing [`TinycortexMemory`] with
//!   [`MemoryTraitProvider`]. Enough when a host wants store, recall and
//!   export and nothing else.
//! - [`engine`] — [`TinycortexProvider`](engine::TinycortexProvider), the whole
//!   engine behind the contract: trees, chunks, entities, the graph, goals,
//!   tool-memory, ingestion, sources, maintenance, people, retrieval, profile,
//!   episodic, and — with `memory-git` — the diff ledger.
//!
//! ## Two drivers, and why both
//!
//! [`provider`] advertises Core, Recall and Portability. That used to be the
//! only thing here, and it was the reason anything wanting a summary tree or a
//! diff ledger reached past the contract to the engine directly: the families
//! existed, but not through `MemoryProvider`. Issue #18 §C3 lifted those
//! implementations here from `tinymemory-module`, which had grown them because
//! it needed them and nowhere else had them.
//!
//! [`engine::TinycortexProvider`] needs what they need — a workspace, a host
//! configuration, and a `MemoryClient` — so it is the heavier of the two, and a
//! host that has none of that still has [`provider`].
//!
//! ## Capability honesty
//!
//! Both advertise exactly what they reach. That is deliberate, not a shortcut:
//! a driver whose capability set overstates its accessors fails
//! [`audit_provider`](tinymemory_api::provider::audit_provider), and a host that
//! filtered its RPC surface from an overstated set would register methods that
//! answer errors. It is also why the `memory-git` feature reaches
//! [`engine::advertised_capabilities`] and not just the accessor — a build
//! without the git-backed snapshot store must not claim a diff ledger.

pub mod engine;
mod memory;

pub use memory::TinycortexMemory;

use std::sync::Arc;

use tinymemory_api::mandatory::MemoryTraitProvider;

/// The driver id this adapter binds under.
///
/// Matches [`tinymemory_api::drivers::TINYCORTEX_DRIVER_ID`], which is where
/// admission reserves it — the constant lives there so a host that compiles
/// this adapter out still refuses to bind something else under the name.
pub use tinymemory_api::drivers::TINYCORTEX_DRIVER_ID;

/// The engine crate itself, re-exported so a consumer of this adapter can
/// name the [`tinycortex::memory::Memory`] argument type and construct a
/// backend without adding a second git dependency and its `[patch]` table.
/// `tinymemory::tinycortex::provider(...)` was unusable from outside this
/// workspace before this line: the feature compiled, the constructor
/// resolved, and its argument type was unnameable.
pub use tinycortex;

/// The engine's simplest backend, re-exported for first-run and test wiring:
/// `provider(Arc::new(InMemoryMemoryStore::new()))` is a complete embedded
/// setup for the mandatory three families.
pub use tinycortex::memory::store::InMemoryMemoryStore;

/// Wrap a TinyCortex backend as a bound memory driver.
///
/// The returned provider advertises the mandatory three families and nothing
/// else; see the crate docs.
#[must_use]
pub fn provider(memory: Arc<dyn tinycortex::memory::Memory>) -> MemoryTraitProvider {
    MemoryTraitProvider::new(
        Arc::new(TinycortexMemory::new(memory)),
        TINYCORTEX_DRIVER_ID,
    )
}

#[cfg(test)]
mod conformance_test;
