//! Engine-neutral sync pipelines (#18 §B1).
//!
//! The Composio orchestration that used to run inside the engine: fetch pages
//! within budget, normalise through `tinymemory-sync`, and write through the
//! [`traits::SyncContext`] sinks. A pipeline sees three capabilities — events,
//! documents, state — and whatever provider the host bound serves them, which
//! is the property §B5's acceptance test needs.
//!
//! The engine keeps its own copies for its internal pipelines (workspace
//! watcher, tree rebuild, repo summarisation — engine-tree features by
//! design). Sources of kind `Composio` route here; tree-coupled source kinds
//! still route through the engine seam.

pub mod composio;
pub mod dispatcher;
pub mod host;
pub mod traits;
