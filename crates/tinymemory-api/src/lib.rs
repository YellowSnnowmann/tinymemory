//! Stable public contracts for the TinyMemory memory system.
//!
//! This crate holds the traits a memory engine implements, the host seam it is
//! bound through, and — re-exported from [`tinymemory_bus`] — the value types,
//! error enum and capability vocabulary they exchange. It is engine-neutral on
//! purpose: `tinycortex` is the default embedded engine, not the owner of the
//! contract, and a second engine (`supermemory`, `mem0`, a self-hosted HTTP
//! backend) implements the same traits without either engine learning about the
//! other. It is deliberately dependency-light (serde / serde_json / anyhow /
//! async-trait / schemars / log, plus `tinymemory-bus`) so depending on the
//! contract never drags in SQLite, git2, reqwest, regex, or an async runtime.
//!
//! ## The vocabulary lives one layer down
//!
//! Every payload type is defined in [`tinymemory_bus`] and re-exported here at
//! its historical path, so `tinymemory_api::types::MemoryEntry` is the *same
//! item* as `tinymemory_bus::types::MemoryEntry`, not a structural twin.
//!
//! The split follows what a consumer actually needs. A **driver author**
//! implements [`provider::MemoryProvider`] and wants this crate: traits, the
//! null driver, the mandatory composition, the [`host`] seam. A **host** loads
//! `tinymemory-module` over `TinyBus` and only makes calls — it names
//! `MemoryEntry` and `MemoryCategory` and implements nothing — so it depends on
//! `tinymemory-bus` alone and compiles none of this.
//!
//! Defining a second set of payload types for that host was the alternative,
//! and it is the failure the root manifest's `[patch]` table exists to prevent:
//! `MemoryCategory` from the module would not be `MemoryCategory` in the host,
//! with a conversion at every call site that nothing type-checks.
//!
//! ## Self-contained by design
//!
//! Nothing here names a host type. A third-party memory driver must be able to
//! depend on this crate alone, and the *generic* subsystem/driver vocabulary of
//! the OpenHuman kernel (`Driver`, `DriverClass`, `SubsystemRegistry`, the
//! policy `Guard`) must not be inherited from a *memory* crate by whichever
//! subsystem is cut over next. So the contract carries its own identity,
//! capability, and health vocabulary, and the host's memory adapter converts at
//! the boundary — see [`health`] for the shape that conversion relies on.
//!
//! Driver *class* (embedded / external / null) is deliberately **absent**: that
//! is a host configuration fact about how a driver was bound, not something a
//! driver reports about itself.
//!
//! ## The TinyCortex engine's historical paths still resolve
//!
//! This contract used to live in the TinyCortex repository as `tinycortex-api`.
//! That crate is now a deprecated re-export of this one, and the engine crate
//! aliases these modules back into their historical paths
//! (`tinycortex::memory::{types, error, traits}`,
//! `tinycortex::memory::chunks::types`, `tinycortex::memory::tree::runtime::types`,
//! `tinycortex::memory::tool_memory::types`, `tinycortex::memory::goals::types`),
//! so every existing path keeps resolving unchanged.
//!
//! ## Module map
//!
//! - [`types`]: pure data contracts (entries, hits, taint, namespaces).
//! - [`recall`]: the borrowed [`recall::RecallOpts`] and owned, serde-derived
//!   [`recall::OwnedRecallOpts`] recall filters (both re-exported from
//!   [`types`]).
//! - [`capabilities`]: the eighteen [`capabilities::Capability`] families and
//!   the [`capabilities::Capabilities`] set negotiated at bind time.
//! - [`provider`]: the driver contract — [`provider::MemoryProvider`] plus the
//!   eighteen capability family traits and the value types they need.
//! - [`null`]: [`null::NullMemoryProvider`], the reference driver a
//!   compiled-out or unconfigured memory subsystem binds to.
//! - [`health`]: [`health::MemoryHealth`], the liveness state a driver reports.
//! - [`version`]: [`CONTRACT_VERSION`] and the [`is_compatible`] bind rule.
//! - [`error`]: the typed [`error::MemoryError`] enum and its result alias.
//! - [`traits`]: the [`traits::Memory`] storage-backend trait.
//! - [`chunks`]: the persisted chunk model ([`chunks::Chunk`], [`chunks::Metadata`],
//!   [`chunks::SourceRef`], …) and the deterministic [`chunks::chunk_id`].
//! - [`tree`]: the markdown summary-tree node model ([`tree::TreeNode`],
//!   [`tree::NodeLevel`], [`tree::TreeStatus`], …).
//! - [`tool_memory`]: tool-scoped rule contracts ([`tool_memory::ToolMemoryRule`], …).
//! - [`goals`]: the long-term goals document ([`goals::GoalsDoc`], [`goals::GoalItem`]).
//! - [`host`]: the **host seam** — [`host::MemoryHostConfig`],
//!   [`host::EmbeddingProvider`], [`host::MemoryEventSink`], and the memory
//!   config sections whose serde form is persisted in a host's `config.toml`.
//! - [`wire`]: the error-name table a driver reached over a bus or a socket
//!   round-trips [`error::MemoryError`] through. Shared by both ends of every
//!   such transport, so the names cannot drift apart.

pub mod drivers;
/// The process-global memory event sink, and the `publish` the engine calls in
/// place of the host's own bus.
///
/// This is **host policy, not engine substance** — `tinymemory-core`'s own
/// ownership note lists "the event bus" on the host side of the split. It lives
/// here so a host that talks to the memory module only over the bus can install
/// a sink and read sync events without linking the engine.
pub mod events;
pub mod host;
/// The ambient per-turn memory-source allowlist (`AgentProfile::memory_sources`).
///
/// Host policy in the same sense as [`events`]: the host decides which sources a
/// turn may recall from, and the engine merely reads the task-local. Kept here
/// so that decision is expressible without the engine crate.
///
/// Behind the `source-scope` feature: it is a `tokio::task_local!`, and the
/// module docs above promise driver authors that depending on the contract
/// never drags in an async runtime.
#[cfg(feature = "source-scope")]
pub mod source_scope;
/// The memory-sync lifecycle vocabulary and its emit helper.
pub mod sync_events;

// The wire vocabulary, re-exported from `tinymemory-bus`.
//
// These modules used to be defined here. They moved down a layer because a
// *host* needs them and needs nothing else in this crate: it loads
// `tinymemory-module` and makes calls, so it names `MemoryEntry` and
// `MemoryCategory` but implements no trait, binds no driver and parses no
// config. Making it depend on the whole driver contract to spell a payload type
// was the wrong shape.
//
// Re-exported rather than merely available, so every historical path still
// resolves — `tinymemory_api::types::MemoryEntry` is the same item as
// `tinymemory_bus::types::MemoryEntry`, not a twin of it. That identity is the
// point: a second definition would need a conversion at the module seam that
// nothing type-checks.
pub use tinymemory_bus::{
    capabilities, chunks, error, goals, health, recall, tool_memory, tree, types, version, wire,
};
/// The mandatory-family composition: wrap any [`traits::Memory`] backend as a
/// complete [`provider::MemoryProvider`].
///
/// Lives here rather than in the `tinymemory` facade because every adapter
/// needs it, and an adapter that reached for it in the facade made the facade
/// unable to depend on adapters in turn — a package cycle cargo forbids, and
/// the reason #18 §D1's engine features could not be declared. It costs this
/// crate nothing: the module names only `async_trait`, `std`, and this crate's
/// own contract types. The facade re-exports it, so `tinymemory::mandatory`
/// keeps resolving.
pub mod mandatory;
pub mod null;
pub mod provider;
pub mod traits;

pub use tinymemory_bus::{is_compatible, CONTRACT_VERSION};
