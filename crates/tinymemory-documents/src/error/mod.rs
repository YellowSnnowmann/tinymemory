//! The crate-wide result alias.
//!
//! There is deliberately no `tinymemory_documents::Error`. Everything this
//! crate produces is on its way into a [`tinymemory_api::provider::MemoryProvider`],
//! and every failure it can have — a format nothing can convert, a body over
//! the size cap, a URL the guard refuses, a backend that rejected the write —
//! already has a name in [`MemoryError`]. A second enum would mean every caller
//! converting between two vocabularies for the same failures, and the
//! conversion would lose the variant a retry policy keys on.
//!
//! Which variant means what here:
//!
//! - [`MemoryError::Invalid`] — the caller's input: an empty body, a format no
//!   converter handles, a namespace that fails validation.
//! - [`MemoryError::BudgetExceeded`] — a document larger than the cap.
//! - [`MemoryError::Unsupported`] — the *bound driver* cannot accept content at
//!   all, which is a deployment fact rather than a bad request.
//! - [`MemoryError::Unreachable`] / [`MemoryError::Backend`] — the URL fetch.

use tinymemory_api::error::MemoryError;

/// Result alias for this crate's fallible operations.
pub type Result<T> = std::result::Result<T, MemoryError>;
