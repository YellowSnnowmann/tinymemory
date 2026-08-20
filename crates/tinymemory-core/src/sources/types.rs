//! The memory-source contracts, from the engine-neutral crate that owns them.
//!
//! Issue #18 §B4. These were re-exported from `crate::engine::backend::sources`,
//! so the shapes a reader exchanged were the *engine's* — a host binding a
//! different driver could not describe a source at all. They live in
//! `tinymemory-sources` now, which names no engine.
//!
//! Still a re-export, deliberately: `crate::sources::types::SourceKind` is the
//! path ~150 call sites in this crate and 24 in OpenHuman already use, and the
//! move delivers the decoupling without spending that churn.
pub use tinymemory_sources::{
    ContentType, MemorySourceEntry, MemorySourcePatch, SourceContent, SourceItem, SourceKind,
};
