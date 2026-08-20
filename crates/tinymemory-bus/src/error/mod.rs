//! The crate-wide [`Error`] and its [`Result`] alias.
//!
//! # This is not the memory error
//!
//! A failed *memory operation* is a [`MemoryError`], and it travels back from
//! the module as a `(name, message)` pair that [`crate::wire`] converts. That
//! is the interesting error, and it is not this one.
//!
//! [`Error`] covers the far narrower thing this crate does on its own: turning
//! a typed call into an argument array, and turning a reply body back into a
//! typed response. Both are `serde_json` operations, so both can fail, and both
//! failures mean the same thing — the contract and the peer disagree about a
//! payload's shape.
//!
//! Keeping the two apart matters at the call site. A host that gets a
//! [`MemoryError::NotFound`] has learned something about its data; a host that
//! gets an [`Error::Decode`] has learned that its build of this crate does not
//! match the module it is talking to, which is an operator problem and not a
//! caller one.
//!
//! [`MemoryError`]: tinymemory_api::error::MemoryError
//! [`MemoryError::NotFound`]: tinymemory_api::error::MemoryError::NotFound

/// A failure encoding a call's arguments or decoding its reply.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A call's arguments could not be serialized into a frame body.
    ///
    /// In practice this is unreachable for the payload types on this wire —
    /// they are plain data with derived `Serialize` impls. It stays a `Result`
    /// rather than an unwrap because "in practice unreachable" is not the same
    /// as unreachable, and a panic in a host's memory path is a worse answer
    /// than an error it can log.
    #[error("encoding call arguments failed: {0}")]
    Encode(#[source] serde_json::Error),

    /// A reply body did not match the response type this contract expects.
    ///
    /// The usual cause is a version skew: the module was built from a newer
    /// contract than the host. The message carries `serde_json`'s path into the
    /// offending value, which names the field but not user memory content.
    #[error("decoding a reply failed: {0}")]
    Decode(#[source] serde_json::Error),
}

/// The result type returned by every fallible function in this crate.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod test;
