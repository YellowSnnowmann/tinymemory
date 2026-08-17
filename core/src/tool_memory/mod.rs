//! Tool-scoped memory layer for durable learnings and high-priority rules.
//!
//! Implements the dedicated memory namespace requested in
//! [issue #1400](https://github.com/tinyhumansai/openhuman/issues/1400):
//! a first-class storage and retrieval surface for **actionable**
//! tool-specific guidance, distinct from the
//! `tool_effectiveness`
//! statistics namespace and from the generic `global` / `skill-*`
//! namespaces.
//!
//! ## Namespace convention
//!
//! Each tool gets its own namespace `tool-{tool_name}`. The prefix is
//! distinct from `global`, `skill-{id}`, `tool_effectiveness`, and the
//! learning namespaces so list/clear operations can reason about it
//! without ambiguity. Build the namespace string via
//! [`tool_memory_namespace`] — never hard-code the format.
//!
//! ## Components
//!
//! - [`crate::engine::backend::tool_memory::types`] owns [`ToolMemoryRule`],
//!   [`ToolMemoryPriority`], and [`ToolMemorySource`].
//! - [`crate::engine::backend::tool_memory::store`] owns [`ToolMemoryStore`], the
//!   put/list/delete/prompt API built on top of an `Arc<dyn Memory>`.
//! - `capture` — `ToolMemoryCaptureHook`, the post-turn
//!   `PostTurnHook` that records user edicts and repeated tool
//!   failures.
//! - `prompt`  — `ToolMemoryRulesSection`, the prompt section that
//!   pins Critical / High rules into the system prompt so they survive
//!   mid-session compression.
//! - `tools`   — agent-facing read/write tools:
//!   `tools::MemoryToolsListTool`, `tools::MemoryToolsPutTool`.
//!
//! `PostTurnHook`: the host's `agent::hooks::PostTurnHook`

mod store;
#[cfg(any(test, feature = "test-support"))]
pub mod test_helpers;

pub use crate::engine::backend::tool_memory::{
    store::{ToolMemoryStore, TOOL_MEMORY_PROMPT_CAP},
    types::{tool_memory_namespace, ToolMemoryPriority, ToolMemoryRule, ToolMemorySource},
};
pub use store::tool_memory_store;
