//! Per-action scope classification (read / write / admin) plus the
//! [`CuratedTool`] catalog type that providers use to whitelist the
//! actions they want surfaced to the agent.
//!
//! Composio publishes 60+ actions per toolkit; most are noise for the
//! agent's planning loop. Each provider exports a hand-curated
//! [`CuratedTool`] slice via [`super::ComposioProvider::curated_tools`]
//! that pares the surface down to a useful subset and tags every action
//! with a [`ToolScope`] so per-user scope preferences can gate execution.
//!
//! # Where the definitions live (#5560)
//!
//! All of it moved to [`tinymemory_api::composio::scopes`] and is re-exported
//! here at its historical path. The reason is that the *same verdict* has to be
//! reached on both sides of the module boundary: OpenHuman filters the agent's
//! visible tool list with [`classify_unknown`] and [`find_curated`], and the
//! sync pipelines gate execution with them inside the module. Two copies of the
//! verb-precedence rule would be two different answers to "may this action
//! run", which is not a shape mismatch but a permissions bug.
//!
//! The curated catalogs themselves stay in this crate — see
//! [`super::catalogs`] and the per-toolkit modules. They are provider data
//! rather than contract vocabulary, they change whenever a provider does, and
//! nothing about them has to cross a frame.

pub use tinymemory_api::composio::scopes::{
    classify_unknown, find_curated, toolkit_from_slug, CuratedTool, ToolScope,
};

#[cfg(test)]
#[path = "tool_scope_tests.rs"]
mod tests;
