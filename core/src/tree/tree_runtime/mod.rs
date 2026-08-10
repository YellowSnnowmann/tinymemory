//! Hierarchical time-based summary tree.
//!
//! Organizes summaries as a tree: root → year → month → day → hour (leaf).
//! Each hour, a background job drains buffered raw content, summarizes it into
//! the hour leaf, and propagates updated summaries upward through the tree.
//! Stored as markdown files in `memory/namespaces/{ns}/tree/`.
//!
//! This module was renamed from `memory::summarizer` to
//! `memory_tree::tree_runtime` so it no longer collides conceptually with
//! [`crate::tree::summarise`], which is only the single-call
//! LLM fold primitive used during seals.

pub mod engine;
pub mod store;


// Runtime tree types are engine-owned.
pub use tinycortex::memory::tree::runtime::*;
