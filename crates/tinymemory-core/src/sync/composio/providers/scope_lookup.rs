//! Scope-lookup helpers, re-exported at their historical path.
//!
//! Moved to [`tinymemory_api::composio::catalogs`] with the catalogs they walk.
//!
//! One behaviour note, because it looks like a change and is not: the versions
//! here consulted `get_provider(..).curated_tools()` before falling back to
//! `catalog_for_toolkit`. Every native provider's `curated_tools()` returns
//! exactly the slice `catalog_for_toolkit` returns for the same toolkit — true
//! of all six — so the provider hop was pure indirection and the contract
//! versions drop it. That is what lets a host answer "may this action run"
//! without a provider registry.

pub use tinymemory_api::composio::catalogs::{curated_scope_for, toolkit_has_scope};
