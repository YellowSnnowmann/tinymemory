//! TinyMemory — the engine-neutral memory layer.
//!
//! A host that embeds TinyMemory performs every memory operation through one
//! contract, and picks which engine answers it by configuration rather than by
//! recompiling. TinyCortex is the default embedded engine; a second engine
//! (`supermemory`, `mem0`, a self-hosted HTTP backend) implements the same
//! traits and binds in its place without the host learning anything new.
//!
//! ## What is here
//!
//! - **The contract** — [`tinymemory_api`], re-exported wholesale below, so a
//!   host takes one dependency and `tinymemory::provider::MemoryProvider` and
//!   `tinymemory_api::provider::MemoryProvider` are the same type. It is
//!   deliberately dependency-light: depending on the contract never drags in
//!   SQLite, git2, reqwest, or an async runtime.
//! - **[`mandatory`]** — the three mandatory capability families, composed
//!   once over the [`traits::Memory`] storage trait, so every backend that
//!   implements it inherits a correct `store` / `list` / `recall` / export
//!   rather than re-deriving the same four subtleties.
//! - **[`registry`]** — driver admission. Which driver ids exist, what class
//!   each binds as, and the fail-closed rule for out-of-process drivers.
//! - **Engine adapters** — one crate per engine under `adapters/`, each
//!   implementing [`provider::MemoryProvider`] over a concrete engine.
//!
//! ## What is deliberately *not* here
//!
//! Policy. A host that binds a memory driver is responsible for tier
//! enforcement, scope predicates, taint stamping, redaction, egress checks, and
//! audit — and it must apply them in a decorator it owns, on the path every
//! caller takes. Pushing any of that into the engine layer would mean a driver
//! could be swapped for one that does not enforce it, which is the whole reason
//! the policy layer exists.
//!
//! Also not here: the host's RPC surface, its agent tools, its credential
//! storage, and its schedulers. Those are what makes a host a host; an engine
//! that learned about them could no longer be replaced by a different engine.
//!
//! ## Binding, end to end
//!
//! ```no_run
//! use tinymemory::null::NullMemoryProvider;
//! use tinymemory::provider::MemoryProvider;
//! use tinymemory::registry::{ConfigLabels, DriverClass, DriverRegistry};
//! use std::sync::Arc;
//!
//! let registry = DriverRegistry::builtin();
//! let provider: Arc<dyn MemoryProvider> =
//!     match registry.admit("tinycortex", None, ConfigLabels::default()) {
//!         Ok(admitted) => match admitted.class {
//!             // The host constructs the engine adapter it compiled in.
//!             DriverClass::Embedded => unimplemented!("bind the engine adapter"),
//!             _ => Arc::new(NullMemoryProvider::new()),
//!         },
//!         // Refusal is not failure: stay bound, loudly.
//!         Err(fallback) => {
//!             eprintln!("{fallback}");
//!             Arc::new(NullMemoryProvider::new())
//!         }
//!     };
//!
//! // Ask once, at bind time, and cache: filtering an RPC surface from a set
//! // that can change underneath it is worse than not filtering at all.
//! let capabilities = provider.capabilities();
//! ```

pub mod mandatory;
pub mod registry;

// The contract, re-exported wholesale. Listed module by module rather than as a
// glob so the crate's own surface is visible in one place and rustdoc links
// resolve — and so adding a module to the contract is a deliberate act here too.
pub use tinymemory_api::{
    capabilities, chunks, error, goals, health, null, provider, recall, tool_memory, traits, tree,
    types,
};
pub use tinymemory_api::{is_compatible, CONTRACT_VERSION};

/// The contract crate itself, for callers that want to name it explicitly.
pub use tinymemory_api as api;
