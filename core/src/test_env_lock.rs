//! Serialises tests that mutate process environment.
//!
//! The host has a lock of the same name (`config::TEST_ENV_LOCK`) over the same
//! variables. They are deliberately *different* locks: each crate's tests link
//! into their own binary and therefore their own process, so a shared lock
//! would buy nothing and would mean this crate owning a mutex for the host's
//! benefit. Same reasoning as
//! [`crate::embedding_host::embedding_test_guard`].

use std::sync::Mutex;

/// Held for the duration of any test that sets or clears an env var the config
/// loader reads. Poison is deliberately ignored — a panicking test must not
/// cascade into every later one.
pub static TEST_ENV_LOCK: Mutex<()> = Mutex::new(());
