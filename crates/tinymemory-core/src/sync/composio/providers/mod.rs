//! Provider-specific code for Composio toolkits.
//!
//! Each Composio toolkit (gmail, notion, slack, …) can register a
//! [`ComposioProvider`] implementation that knows how to:
//!
//!   * Fetch a normalized **user profile** for a connected account.
//!   * Run an **initial / periodic sync** that pulls fresh data from the
//!     upstream service via the backend-proxied
//!     `ComposioClient`.
//!   * React to **trigger webhooks** that arrive over the
//!     `composio:trigger` Socket.IO bridge.
//!   * React to **OAuth handoff completion** so the very first sync can
//!     run as soon as a user connects an account.
//!
//! Providers are pure Rust — there is no JS sandbox involved. They are
//! the native counterpart to the QuickJS skill bundles in
//! `tinyhumansai/openhuman-skills`, but specialized for Composio's API
//! surface and run inside the core process directly.
//!
//! ## Registry & dispatch
//!
//! The [`registry`] module owns a process-global `HashMap<toolkit_slug,
//! Arc<dyn ComposioProvider>>`. The composio event bus subscriber
//! (`super::bus::ComposioTriggerSubscriber`) and the periodic sync
//! task both look up providers by toolkit slug and call into them.
//!
//! ## Why a trait, not a giant `match`
//!
//! Each provider has provider-specific shapes (gmail returns
//! emailAddress + messagesTotal, notion returns workspaces + pages, …)
//! and a different idea of what "sync" means. A trait keeps each
//! provider's implementation isolated, individually testable, and
//! easy to add without touching the dispatch layer.

mod descriptions;
pub(crate) mod helpers;
mod scope_lookup;
pub mod tool_scope;
mod traits;
mod types;
pub mod user_scopes;

pub mod catalogs;
mod catalogs_compat;
pub mod clickup;
pub mod github;
pub mod gmail;
pub mod linear;
pub mod notion;
pub mod profile;
pub mod profile_md;
pub mod registry;
pub mod slack;
pub mod sync_state;

// The capability matrix, the curated-catalog lookup and the visibility gate
// all moved to the contract crate (OpenHuman#5560) — see [`catalogs`] and
// [`tinymemory_api::host::composio::capability_matrix`]. They are re-exported
// at the bottom of this file, so every historical `providers::…` path keeps
// resolving and the wire surface is unchanged.

/// All toolkit slugs that have a curated agent-ready catalog.
///
/// Source of truth for the UI "preview / agent integration coming soon" badge:
/// any connected toolkit whose slug is NOT in this list can be authorized but
/// lacks a curated tool surface, so the agent can't use it productively.
///
/// Defined in the contract crate (#5560) because the *host* renders that badge
/// and reaching this crate to spell the list is one of the compile-time links
/// the issue removes. Re-exported here so every historical
/// `providers::agent_ready_toolkits()` call keeps resolving.
pub use tinymemory_api::composio::scopes::agent_ready_toolkits;

pub use descriptions::toolkit_description;
pub(crate) use helpers::{first_array_str, merge_extra};
pub use tinymemory_api::composio::catalogs::{catalog_for_toolkit, is_action_visible_with_pref};
pub use tinymemory_api::host::composio::capability_matrix;
// `pick_str` is a provider payload normaliser and lives in tinycortex; it is
// re-exported here so the ~40 in-tree call sites keep resolving unchanged.
// Note this is deliberately NOT `providers::common::pick_str`, which coerces
// numbers to strings — see the doc comments on both definitions.
pub use registry::{
    all_providers, get_provider, init_default_providers, register_provider, ProviderArc,
};
pub use scope_lookup::{curated_scope_for, toolkit_has_scope};
pub(crate) use tinymemory_sync::helpers::pick_str;
pub use tool_scope::{classify_unknown, find_curated, toolkit_from_slug, CuratedTool, ToolScope};
pub use traits::{resolve_sync_interval_secs, sync_interval_env_var, ComposioProvider};
pub use types::{
    ComposioUsage, ComposioUsageHandle, GithubFetchMode, NormalizedTask, ProviderContext,
    ProviderUserProfile, SyncOutcome, SyncReason, TaskContainer, TaskFetchFilter, TaskKind,
};
pub use user_scopes::{load_or_default as load_user_scope_or_default, UserScopePref};

#[cfg(test)]
#[path = "providers_tests.rs"]
mod tests;
