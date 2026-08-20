//! How a failed call comes back, and how a host turns it into a
//! [`MemoryError`] again.
//!
//! A `TinyBus` error is a name and a message. The name is the contract; the
//! message is for a human and must never carry a namespace key, an entry's
//! content, a recall query, a credential or an absolute path.
//!
//! The table that maps names to [`MemoryError`] variants lives in
//! `tinymemory-api` and is used by **both** ends — the module maps out, the
//! host maps back. It is re-exported here rather than restated for the same
//! reason the payload types are: two copies of a name table drift, and the
//! symptom of drift is a security-relevant `PathEscape` silently reclassified
//! as a caller mistake.
//!
//! ```
//! use tinymemory_bus::wire;
//! use tinymemory_bus::types::MemoryError;
//!
//! // What a host does with the `(name, message)` pair a failed call returns.
//! let recovered = wire::from_wire(wire::NOT_FOUND, "no such source");
//! assert!(matches!(recovered, MemoryError::NotFound(_)));
//! ```
//!
//! # An unrecognised name is a backend failure, never a caller mistake
//!
//! [`from_wire`] maps a name it does not know to [`MemoryError::Other`]. A
//! module newer than the host's build may name an error this table has no
//! variant for, and answering "your input was wrong" when it was not sends a
//! caller into a rewrite loop over something already correct.
//!
//! [`MemoryError`]: tinymemory_api::error::MemoryError
//! [`MemoryError::Other`]: tinymemory_api::error::MemoryError::Other
//! [`MemoryError::NotFound`]: tinymemory_api::error::MemoryError::NotFound

pub use tinymemory_api::wire::{
    from_wire, wire_message, wire_name, BACKEND, BUDGET_EXCEEDED, INVALID, IO, NOT_FOUND, OTHER,
    PATH_ESCAPE, SERDE, TIMEOUT, UNAUTHORIZED, UNAVAILABLE, UNREACHABLE, UNSUPPORTED,
};

#[cfg(test)]
mod test;
