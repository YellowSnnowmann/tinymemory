//! TinyCortex as a TinyMemory driver.
//!
//! This crate is the seam between the TinyCortex engine and the TinyMemory
//! contract. The two describe the same values but are distinct crates, so
//! something has to convert — and it is much better for that to be one small
//! audited crate than a conversion scattered across every call site in a host.
//!
//! ## What is here
//!
//! - [`convert`] — total, exhaustively-destructuring value conversions in both
//!   directions. A field added to either contract becomes a compile error here
//!   instead of a silently dropped value.
//! - [`TinycortexMemory`] — wraps any TinyCortex [`tinycortex::memory::Memory`]
//!   backend as a TinyMemory
//!   [`Memory`](tinymemory_api::traits::Memory).
//! - [`provider`] — the one call that turns a TinyCortex backend into a bound
//!   driver, by pairing [`TinycortexMemory`] with
//!   [`MemoryTraitProvider`].
//!
//! ## Scope: the mandatory three, not the whole engine
//!
//! A driver built here advertises Core, Recall and Portability. TinyCortex can
//! do far more — trees, chunks, entities, a diff ledger — but those families
//! are reached through engine entry points that need a host's configuration,
//! embedding compute and job queue, none of which this crate has. A host that
//! provides them implements the optional families itself and delegates only the
//! mandatory three here.
//!
//! Advertising only what is reachable is deliberate, not a shortcut: a driver
//! whose capability set overstates its accessors fails
//! [`audit_provider`](tinymemory_api::provider::audit_provider), and a host that
//! filtered its RPC surface from an overstated set would register methods that
//! answer errors.

pub mod convert;
mod memory;

pub use memory::TinycortexMemory;

use std::sync::Arc;

use tinymemory::mandatory::MemoryTraitProvider;

/// The driver id this adapter binds under.
///
/// Matches [`tinymemory::registry::TINYCORTEX_DRIVER_ID`], which is where
/// admission reserves it — the constant lives there so a host that compiles
/// this adapter out still refuses to bind something else under the name.
pub use tinymemory::registry::TINYCORTEX_DRIVER_ID;

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
