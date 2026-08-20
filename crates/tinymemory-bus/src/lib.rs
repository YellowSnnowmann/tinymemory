//! The `TinyBus` wire contract for the TinyMemory module.
//!
//! TinyMemory ships as a loadable `TinyBus` module so a host does not compile
//! the engine: `crates/tinymemory-module` exports one object,
//! `/ai/tinyhumans/tinymemory/Memory`, with 89 members on it. A host that loads
//! that binary needs three things to talk to it — the member names, the types
//! on either side of each call, and the error-name table — and none of those
//! are in the module binary, which is a `cdylib`.
//!
//! This crate is those three things, as a library a host links:
//!
//! - [`names`] — the bus name, the object path, and one constant per member.
//! - [`types`] — every value type that crosses a frame.
//! - [`calls`] — one struct per member, carrying its arguments in wire order
//!   and its reply type.
//! - [`wire`] — the error names, and the mapping back to `MemoryError`.
//!
//! ```
//! use tinymemory_bus::calls::{core::Get, BusCall};
//! use tinymemory_bus::names::{BUS_NAME, OBJECT_PATH};
//!
//! let args = Get { namespace: "work".to_string(), key: "standup".to_string() }.into_args()?;
//!
//! // Everything a `Connection::call` needs, with nothing spelled by hand.
//! assert_eq!((BUS_NAME, OBJECT_PATH, Get::METHOD), (
//!     "ai.tinyhumans.tinymemory.Memory",
//!     "/ai/tinyhumans/tinymemory/Memory",
//!     "Get",
//! ));
//! assert_eq!(args.to_string(), r#"["work","standup"]"#);
//! # Ok::<(), tinymemory_bus::Error>(())
//! ```
//!
//! # There is no transport here, on purpose
//!
//! This crate does not depend on `tinybus`, and holds no connection, no client
//! and no `call()` that sends anything. Two reasons, and the second is the
//! blunt one.
//!
//! A host already owns its connection. It has its own reconnect policy, its own
//! timeouts, its own tracing, and its own idea of what a memory call costs it. A
//! client here would either duplicate that or fight it, and the useful part —
//! *what to send and what comes back* — is exactly what is in this crate.
//! Wiring it up is a dozen lines over a `Connection`; `README.md` has the shape.
//!
//! And structurally it could not work anyway. `tinybus` is vendored as a git
//! submodule whose manifest inherits fields from its own nested
//! `[workspace.package]`; a member of *this* workspace that depends on it makes
//! cargo resolve that inheritance against the wrong root and fail. That is why
//! `crates/tinymemory-module` is its own workspace root — see the root
//! manifest's note on `exclude`. A contract crate a host links has no business
//! being a separate workspace, so it stays transport-free and every member of
//! this workspace can depend on it.
//!
//! # Why this is not just `tinymemory-api`
//!
//! `tinymemory-api` is the **driver** contract: what an engine implements. It
//! carries `MemoryProvider` and its eighteen capability traits, the
//! mandatory-family composition, the null driver, and the `host::` config
//! sections a host persists in `config.toml`.
//!
//! A host that loads the module implements none of that. It makes calls. This
//! crate is the subset that crosses a frame, so what a host compiles against is
//! what it can actually send and receive — and a member that exists in the
//! trait but is not exported on the bus is absent here rather than tempting.
//!
//! The types themselves are **re-exported** from `tinymemory-api`, never
//! redefined. [`types`] explains why at length; the short version is that a
//! second definition would make `MemoryCategory` from the module a different
//! type from `MemoryCategory` in the host, which is a failure this repository
//! has already had once.
//!
//! # Staying in step with the module
//!
//! [`names::METHODS`] lists every member. `crates/tinymemory-module` asserts its
//! served members against that list, so a method added to the interface without
//! a constant and a call struct here fails that crate's tests rather than
//! turning up as an `UnknownMethod` at runtime in a host.

pub mod calls;
pub mod error;
pub mod names;
pub mod types;
pub mod wire;

pub use error::{Error, Result};
pub use names::{BUS_NAME, METHODS, OBJECT_PATH};
