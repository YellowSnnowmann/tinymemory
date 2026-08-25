//! Every type that crosses the TinyMemory `TinyBus` boundary, and the names of
//! the members that carry them.
//!
//! TinyMemory ships as a loadable `TinyBus` module: `crates/tinymemory-module`
//! exports one object with 120 members on it, built as a `cdylib`. A host that
//! loads it — OpenHuman — can call into it but cannot `use` anything out of it,
//! so the payload vocabulary has to be published as an ordinary library. This
//! is that library.
//!
//! ## What is here
//!
//! - [`names`] — the bus name, the object path, and one constant per member.
//! - [`types`], [`chunks`], [`recall`], [`tree`], [`goals`], [`tool_memory`],
//!   [`health`], [`capabilities`], [`evidence`] — the value vocabulary.
//! - [`learning`] — the learning-candidate taxonomy ([`learning::FacetClass`],
//!   [`learning::CueFamily`], [`learning::LearningCandidate`]), whose producer
//!   and consumer sit on opposite sides of the module boundary.
//! - [`composio`] — the connector-sync vocabulary: what a provider run
//!   produces ([`composio::SyncOutcome`], [`composio::NormalizedTask`]), what
//!   it remembers between runs ([`composio::SyncState`]) and what the user has
//!   allowed it to do ([`composio::UserScopePref`]).
//! - [`graph`] — the bounded graph-view model ([`graph::GraphView`],
//!   [`graph::GraphViewQuery`]), the graph counterpart of [`tree`].
//! - [`namespace`] — the `<section>:<scope>` namespace convention
//!   ([`namespace::Namespace`], [`namespace::MemorySection`]) and its
//!   validator.
//! - [`provider`] — the value types the capability families exchange.
//! - [`error`] and [`wire`] — [`error::MemoryError`] and the name table it
//!   round-trips through when a driver is reached over a wire.
//! - [`version`] — [`CONTRACT_VERSION`] and the [`is_compatible`] bind rule.
//!
//! ## What is deliberately not here
//!
//! **No traits.** `MemoryProvider` and the twenty capability-family traits
//! are driver obligations: they describe what an engine must implement, not
//! what a frame carries. They stay in `tinymemory-api`, which depends on this
//! crate.
//!
//! **No transport.** This crate does not depend on `tinybus` and holds no
//! connection, client, or codec. A host already owns its connection — its
//! reconnect policy, its timeouts, its tracing — and the useful part is the
//! vocabulary, not another wrapper around it.
//!
//! That is also a structural necessity, not only a preference: `tinybus` is
//! vendored as a submodule whose manifest inherits fields from its own nested
//! `[workspace.package]`, so a member of this workspace that depends on it
//! makes cargo resolve that inheritance against the wrong root and fail. It is
//! why `crates/tinymemory-module` is its own workspace root — see the note on
//! `exclude` in the root `Cargo.toml`. A crate every workspace member can
//! depend on has to stay transport-free.
//!
//! **No host configuration, no null driver, no composition helpers.** Those are
//! `tinymemory-api`'s, and none of them cross a frame.
//!
//! ## This crate is underneath the contract, not beside it
//!
//! `tinymemory-api` **depends on this crate and re-exports all of it**, so
//! every historical path — `tinymemory_api::types::MemoryEntry`,
//! `tinymemory::MemoryCategory`, `tinycortex::memory::types::*` — keeps
//! resolving unchanged, and the types are the *same types*, not structural
//! twins.
//!
//! That direction is the whole point. Defining a parallel set of payload types
//! for hosts would mean `MemoryCategory` from the module was not
//! `MemoryCategory` in the host, with a conversion at every call site that
//! nothing checks — the exact failure the root manifest's `[patch]` table
//! exists to prevent, reintroduced deliberately. One definition, here, at the
//! bottom.
//!
//! A host that only makes calls therefore depends on this crate alone and
//! compiles no traits, no engine seam and no config surface. A driver author
//! depends on `tinymemory-api` and gets both.
//!
//! ## Staying in step with the module
//!
//! [`names::METHODS`] lists every member. `crates/tinymemory-module` asserts
//! its served members against that list, in order, so a method added to the
//! interface without an entry here fails that crate's tests rather than
//! surfacing as an `UnknownMethod` in a host at runtime.

pub mod capabilities;
pub mod chunks;
pub mod composio;
pub mod error;
pub mod evidence;
pub mod goals;
pub mod graph;
pub mod health;
pub mod learning;
pub mod names;
pub mod namespace;
pub mod provider;
pub mod recall;
pub mod tool_memory;
pub mod tree;
pub mod types;
pub mod version;
pub mod wire;

pub use names::{BUS_NAME, METHODS, OBJECT_PATH};
pub use version::{is_compatible, CONTRACT_VERSION};
