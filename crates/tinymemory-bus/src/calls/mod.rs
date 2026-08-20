//! One typed struct per member, and the [`BusCall`] trait that ties it to its
//! name and its reply type.
//!
//! # Why arguments get a struct at all
//!
//! `#[tinybus::interface]` puts a method's arguments on the wire as a
//! **positional JSON array**, decoded on the far side into a tuple. That is a
//! fine encoding and a bad thing to write by hand:
//!
//! ```json
//! ["work", "standup", "…", "Fact", null, "Untrusted"]
//! ```
//!
//! Two of those six are `Option`s, two are enums that serialize as strings, and
//! swapping `namespace` with `key` produces a call that succeeds and writes the
//! entry to the wrong place. Nothing on the module side can catch it: both are
//! `String`, in the right position count, and the engine has no way to know
//! which one the caller meant.
//!
//! So a caller fills in named fields and this crate does the positioning:
//!
//! ```
//! use tinymemory_bus::calls::{core::Store, BusCall};
//! use tinymemory_bus::types::{MemoryCategory, MemoryTaint};
//!
//! let args = Store {
//!     namespace: "work".to_string(),
//!     key: "standup".to_string(),
//!     content: "shipped the loader".to_string(),
//!     category: MemoryCategory::Fact,
//!     session_id: None,
//!     taint: MemoryTaint::Trusted,
//! }
//! .into_args()?;
//!
//! assert_eq!(Store::METHOD, "Store");
//! assert_eq!(args[0], "work");
//! assert_eq!(args[1], "standup");
//! # Ok::<(), tinymemory_bus::Error>(())
//! ```
//!
//! # The reply type travels with the call
//!
//! [`BusCall::Response`] is the other half, and it is the half a host would
//! otherwise get wrong quietly. `Get` answers `Option<MemoryEntry>` while
//! `Forget` answers `bool`; both are perfectly good JSON, and decoding one as
//! the other fails at a point far from the call. Binding the response type to
//! the call type means a host writes the method once and the compiler knows
//! what comes back.
//!
//! # What this is not
//!
//! Not a client. There is no connection here, no `call()` that sends anything —
//! see [`crate`] for why the transport is deliberately out of scope. A host
//! writes one small generic helper over its own `tinybus::Connection`; the
//! shape is in this crate's `README.md`.

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::{Error, Result};

pub mod chunks;
pub mod core;
pub mod documents;
pub mod driver;
pub mod episodic;
pub mod goals;
pub mod graph;
pub mod ingest;
pub mod maintenance;
pub mod people;
pub mod portability;
pub mod profile;
pub mod recall;
pub mod retrieval;
pub mod sources;
pub mod tool_memory;
pub mod tree;

/// One member of the `TinyMemory` interface, as a typed request.
///
/// An implementor names the member ([`METHOD`](Self::METHOD)), knows what comes
/// back ([`Response`](Self::Response)), and can lay its own fields out in the
/// positional order the module decodes them from
/// ([`into_args`](Self::into_args)).
///
/// Implementors are generated from the module's `#[tinybus::interface]` block,
/// so the field order below is the wire order by construction rather than by
/// review.
pub trait BusCall {
    /// The member name, as it travels in a frame.
    ///
    /// Always one of [`crate::names::METHODS`].
    const METHOD: &'static str;

    /// What the module replies with on success.
    type Response: DeserializeOwned;

    /// Lay the arguments out as the positional array the module decodes.
    ///
    /// The result is always a JSON array — an empty one for a member that takes
    /// no arguments, because `#[tinybus::interface]` skips decoding entirely in
    /// that case and every caller sends `[]`.
    ///
    /// # Errors
    ///
    /// [`Error::Encode`] if a field fails to serialize. Unreachable for the
    /// payload types on this wire, which are plain derived data; see
    /// [`crate::error`].
    fn into_args(self) -> Result<Value>;

    /// Decode a successful reply body into this call's response type.
    ///
    /// # Errors
    ///
    /// [`Error::Decode`] if the body does not match
    /// [`Response`](Self::Response) — in practice, a module built from a
    /// different revision of this contract.
    fn decode_response(body: Value) -> Result<Self::Response> {
        serde_json::from_value(body).map_err(Error::Decode)
    }
}

#[cfg(test)]
mod test;
