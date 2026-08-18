//! Composio provider payload normalisers, engine-neutral by construction.
//!
//! Issue #18 §B3: "Payload normalisers … are pure `Value → Value` transforms
//! with no engine dependency. Move them back into a `tinymemory-sync` crate …
//! so a non-TinyCortex engine gets Composio sync for free."
//!
//! They lived inside the TinyCortex engine, and `tinymemory-core` reached into
//! it to use them — which meant a host binding a *different* memory engine
//! could not have Composio sync at all, despite none of this code caring which
//! engine is bound. Nothing here reads a database, opens a socket, or names an
//! engine type; the dependency list is `serde_json`, two logging facades, and
//! `chrono`.
//!
//! One caveat on "pure", because it is load-bearing and easy to miss.
//! [`gmail_post_process::format_email_local_time`] renders in `chrono::Local`,
//! so it reads the host's timezone — every other normaliser here is a function
//! of its input alone. The raw UTC field is preserved alongside it, so sorting
//! and deduplication stay UTC-based; what varies by host is only the
//! presentation string.
//!
//! These are pure `serde_json::Value` → `Value` transforms: given a raw
//! Composio action response, pull out the fields that make up a task, an
//! issue, a page or a message. They hold no credentials, touch no network,
//! and make no scheduling decisions — provider-specific normalisation is
//! driver-side by definition (see the host's `docs/specs/kernel.md` §4).
//!

pub mod clickup;
pub mod github;
pub mod helpers;
pub mod linear;
pub mod notion;

// The `_post_process` suffix is kept from the engine layout it came from, where
// `slack.rs` and `github.rs` one directory up already held those names. Renaming
// on the way out would have made this a rename *and* a move in one diff.
pub mod gmail_post_process;
pub mod slack_post_process;
