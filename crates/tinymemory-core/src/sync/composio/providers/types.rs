//! Shared types for Composio provider implementations.
//!
//! # What is here, and what moved down (#5560)
//!
//! The *values* a provider exchanges — the run report, the task envelope, the
//! normalized profile — are defined in the contract crate
//! ([`tinymemory_api::composio`]) and re-exported below at their historical
//! paths. OpenHuman names every one of them in its own signatures, so they had
//! to be reachable without a compile-time link to this crate; they are inert
//! serde data, so moving them cost nothing.
//!
//! What stayed is [`ProviderContext`], and it stayed because it is not a value:
//! it holds an `Arc<Config>`, resolves a Composio client through the host seam
//! on every call, and awaits an HTTP round-trip. None of that may enter the
//! contract crate.

use std::sync::Arc;

// Test-only: the tests below build a `TestHostConfig` and call
// `MemoryHostConfig` methods on it directly. Production code in this module
// goes through `crate::Config`, so neither name is needed in a non-test build.
#[cfg(test)]
use tinymemory_api::host::{test_support::TestHostConfig, MemoryHostConfig};

use crate::composio_host::{self, ComposioExecuteResponse};
use crate::config_loader as config_rpc;
use crate::Config;

/// The Composio sync vocabulary, defined in the contract crate.
///
/// Re-exported at this path because roughly a hundred call sites here and in
/// OpenHuman already spell these `providers::SyncOutcome`,
/// `providers::NormalizedTask` and so on, and the move delivers the decoupling
/// without spending that churn.
pub use tinymemory_api::composio::{
    ComposioUsage, ComposioUsageHandle, GithubFetchMode, NormalizedTask, ProviderUserProfile,
    SyncOutcome, SyncReason, TaskContainer, TaskFetchFilter, TaskKind,
};

/// Per-call context handed to provider methods.
///
/// `connection_id` is `None` when a method runs in a "no specific connection"
/// mode (e.g. an across-the-board periodic sync that already iterated). For
/// per-connection paths it is always populated.
///
/// **Mode-aware dispatch (#1710)**: pre-fix, `ProviderContext` cached a
/// pre-baked `ComposioClient` built once at construction time. Toggling
/// `composio.mode = "direct"` mid-session left provider syncs still routing
/// through the backend tinyhumans tenant. The current shape keeps an
/// [`Arc<Config>`] and resolves the underlying client per call through
/// [`ProviderContext::execute`], mirroring the agent-tool migration in the
/// host's `integrations::composio::tools::ComposioExecuteTool`.
///
/// This is the one item in this module that is *not* contract vocabulary: a
/// context is a live handle onto the host seam, not something a frame can
/// carry. See the module docs.
#[derive(Clone)]
pub struct ProviderContext {
    pub config: Arc<Config>,
    pub toolkit: String,
    pub connection_id: Option<String>,
    /// Accumulates Composio billable-action usage across this context's
    /// lifetime. Defaulted at every construction site; only the sync path
    /// (`run_connection_sync`) reads it back. Non-sync callers (agent tools,
    /// task-source fetches) leave it at zero — harmless.
    pub usage: ComposioUsageHandle,
    /// Maximum items to fetch in a single sync pass.
    ///
    /// Set from the corresponding `MemorySourceEntry.max_items` field at
    /// sync-dispatch time. `None` means no cap beyond the provider's own
    /// internal upper bounds.
    pub max_items: Option<u32>,
    /// Maximum sync depth window in days.
    ///
    /// Set from `MemorySourceEntry.sync_depth_days`. When `Some(n)`, the
    /// provider only fetches items from the last `n` days. `None` means
    /// no additional depth restriction beyond the provider's cursor.
    pub sync_depth_days: Option<u32>,
}

impl ProviderContext {
    /// Build a context from the current config + a toolkit slug.
    ///
    /// Returns `None` only when we want to short-circuit early on the
    /// "user clearly not signed in" path. In the post-#1710 shape this
    /// is determined by attempting a factory resolve via
    /// [`composio_host::is_available`] and treating a `false` there as
    /// "skip silently" — the same UX as the pre-fix
    /// `build_composio_client(...).is_some()` probe, but routed
    /// through the mode-aware factory so direct-mode users (no backend
    /// session token, BYO key in keychain) aren't falsely treated as
    /// signed-out.
    pub fn from_config(
        config: Arc<Config>,
        toolkit: impl Into<String>,
        connection_id: Option<String>,
    ) -> Option<Self> {
        // Probe the factory: any successful resolve (Backend OR Direct)
        // means the user has *some* viable Composio client. Direct-mode
        // users typically have no backend session token, which would
        // make a `build_composio_client` probe return None and falsely
        // skip them.
        if composio_host::is_available(&*config) {
            Some(Self {
                config,
                toolkit: toolkit.into(),
                connection_id,
                usage: ComposioUsageHandle::default(),
                max_items: None,
                sync_depth_days: None,
            })
        } else {
            tracing::debug!(
                "[composio:provider_context] from_config: no viable Composio client; \
                 treating as not-signed-in"
            );
            None
        }
    }

    /// Resolve the underlying composio client via the mode-aware
    /// factory and dispatch a single action. This is the canonical
    /// way for provider implementations to execute a Composio action
    /// — going through here ensures the live `composio.mode` toggle is
    /// honoured on every call (#1710).
    ///
    /// Returns the same [`ComposioExecuteResponse`] shape that
    /// `ComposioClient::execute_tool` used to return so existing
    /// provider call-sites can swap `ctx.client.execute_tool(...)` for
    /// `ctx.execute(...)` with no other changes.
    pub async fn execute(
        &self,
        action: &str,
        arguments: Option<serde_json::Value>,
    ) -> anyhow::Result<ComposioExecuteResponse> {
        // [#1710 Wave 4] Reload config fresh per execute so a mid-session
        // `composio.mode` toggle takes effect at the very next call. The
        // Arc<Config> snapshot held by `self` was taken at agent-init time
        // and is otherwise stale relative to subsequent set_api_key /
        // clear_api_key RPCs.
        //
        // Use `reload_config_snapshot_with_timeout` (anchored to the snapshot's
        // `config_path`) rather than `load_config_with_timeout` (which
        // re-resolves `OPENHUMAN_WORKSPACE` from the process env). The config
        // path is stable for the lifetime of a `ProviderContext` — it is set
        // at context creation from the agent's scoped config — so reading from
        // it always reaches the correct user workspace and avoids a data-race
        // in tests that share the process env.
        let live_config = config_rpc::reload_config_snapshot_with_timeout(&*self.config)
            .await
            .map_err(|e| {
                tracing::warn!(
                    action = %action,
                    toolkit = %self.toolkit,
                    error = %e,
                    "[composio:provider_context] execute: reload_config failed"
                );
                anyhow::anyhow!("composio provider_context: failed to reload live config: {e}")
            })?;
        // Mode dispatch (backend tenant vs the user's own direct v3 tenant)
        // lives in the host's `ComposioHost` impl — this side just asks.
        let result = composio_host::execute(
            &*live_config,
            action,
            arguments,
            &live_config.composio().entity_id,
            self.connection_id.as_deref(),
        )
        .await
        .map_err(|e| anyhow::anyhow!(e));

        // Tally billable-action usage at the single chokepoint every provider
        // routes through (#3111). We count any *completed* round-trip — even a
        // provider-reported failure (`successful == false`) is a billable call
        // — and sum the backend-reported `cost_usd`. Transport errors (the
        // `Err` arm) never reached Composio, so they don't count. The lock is
        // held only for the increment, never across an `.await`.
        if let Ok(ref resp) = result {
            if let Ok(mut usage) = self.usage.lock() {
                usage.actions_called = usage.actions_called.saturating_add(1);
                usage.cost_usd += resp.cost_usd;
            }
        }
        result
    }

    /// Memory client handle if the global memory singleton is ready.
    /// Used by providers that want to persist sync snapshots.
    ///
    /// Under `cfg(test)` the global singleton is not booted, so build a
    /// workspace-scoped client directly instead.
    /// Memory client handle if the global memory singleton is ready.
    /// Used by providers that want to persist sync snapshots.
    #[cfg(not(test))]
    pub fn memory_client(&self) -> Option<crate::store::MemoryClientRef> {
        crate::global::client_if_ready()
    }
}

#[cfg(test)]
#[path = "types_test_support.rs"]
mod test_support;

#[cfg(test)]
#[path = "types_tests.rs"]
mod tests;
