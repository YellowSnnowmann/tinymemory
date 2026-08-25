//! [`ComposioHost`] — the Composio integration, as the memory sync layer sees it.
//!
//! The sync pipelines need three things from Composio: which connections are
//! active, the ability to execute a tool against one, and the direct-mode API
//! key. Everything else about the integration — OAuth, the backend session
//! token, per-toolkit allowlists, HMAC-verified trigger fan-out, and the choice
//! between backend-proxied and direct mode — is host concern.
//!
//! # Why the client itself stayed in the host
//!
//! `ComposioClientKind::Direct` wraps an `Arc<openhuman::tools::ComposioTool>`,
//! a host *agent tool*. There is no way to name that from here, and no reason
//! to: mode dispatch reads `config.composio.mode` and fails loud on a typo,
//! which is exactly the kind of policy the README's split assigns to the host.
//!
//! So this trait is deliberately **behavioural, not structural**. It hides
//! `ComposioClientKind` entirely — the core never learns that two modes exist,
//! and the three call sites that used to do
//! `create_composio_client(config)?` → `match kind` → call collapse to one
//! method each.
//!
//! The value types those methods return did move, to
//! [`tinymemory_api::host::composio`], because the pipelines read their fields
//! directly.
//!
//! # Unwired is an error
//!
//! Same reasoning as [`crate::embedding_host`]: a sync run that quietly saw
//! zero connections would look like "nothing to sync" rather than "not wired
//! up", and the difference would only surface as missing memory days later.

use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;

use crate::Config;

pub use tinymemory_api::host::composio::{
    ComposioCapability, ComposioConnection, ComposioExecuteResponse,
};

/// The Composio operations the memory sync layer performs.
#[async_trait]
pub trait ComposioHost: Send + Sync + std::fmt::Debug {
    /// Every connection the signed-in user has, active or not.
    ///
    /// Filtering to active ones is the caller's job — `ComposioConnection`
    /// carries the status and treats an empty one as inactive, so a malformed
    /// upstream row is never presented as connected.
    ///
    /// # Errors
    ///
    /// Returns `Err` when no client can be built (no backend session, bad mode
    /// string) or the upstream call fails.
    async fn list_connections(&self, config: &Config) -> Result<Vec<ComposioConnection>, String>;

    /// Execute `tool` against a connection.
    ///
    /// # Errors
    ///
    /// Returns `Err` when no client can be built or the call fails. A provider
    /// that answers with `successful: false` is **not** an error — that is
    /// reported in the returned [`ComposioExecuteResponse`].
    async fn execute(
        &self,
        config: &Config,
        tool: &str,
        arguments: Option<serde_json::Value>,
        entity_id: &str,
        connection_id: Option<&str>,
    ) -> Result<ComposioExecuteResponse, String>;

    /// The direct-mode Composio API key from the host's credential store, or
    /// `None` when direct mode is not configured.
    fn api_key(&self, config: &Config) -> Option<String>;

    /// The OpenHuman backend bearer for proxied ("backend") mode.
    ///
    /// A seam rather than a config field, and that is the whole point of it.
    /// The bearer is an app-session JWT the host refreshes; a value captured
    /// once — at module load, say — works until it expires and then makes every
    /// sync fail with an auth error that reads as the user being signed out.
    /// Asking per call means the answer is always the one that is valid now.
    ///
    /// `None` means the host has no session to lend, which is a signed-out user
    /// rather than a broken one. The caller must not read that as "nothing to
    /// sync": [`composio_config`](crate::sync::pipelines::host::composio_config)
    /// turns it into a named refusal instead.
    ///
    /// Defaulted to `None` so a host that predates this member still compiles
    /// and simply falls back to whatever `Config::session_token` answers, which
    /// is exactly the behaviour it had before the member existed.
    fn session_bearer(&self, config: &Config) -> Option<String> {
        let _ = config;
        None
    }

    /// Whether *some* viable client resolves for the current config.
    ///
    /// The sync layer uses this as its "is the user signed in?" probe. It must
    /// answer for **either** mode: direct-mode users typically have no backend
    /// session token, and probing for one alone would falsely skip them.
    fn is_available(&self, config: &Config) -> bool;
}

static HOST: RwLock<Option<Arc<dyn ComposioHost>>> = RwLock::new(None);

const NOT_INSTALLED: &str =
    "no ComposioHost installed — the host must call memory::composio_host::set_composio_host \
     during startup wiring, before any sync runs";

/// Install the host's Composio integration. Called once during startup wiring.
pub fn set_composio_host(host: Arc<dyn ComposioHost>) {
    *HOST.write() = Some(host);
}

/// Remove any installed host. For tests.
pub fn clear_composio_host() {
    *HOST.write() = None;
}

/// The installed host, or `None` when nothing has been wired up.
#[must_use]
pub fn composio_host() -> Option<Arc<dyn ComposioHost>> {
    HOST.read().clone()
}

/// The installed host.
///
/// # Errors
///
/// Returns `Err` when no host has been installed.
pub fn require_composio_host() -> Result<Arc<dyn ComposioHost>, String> {
    composio_host().ok_or_else(|| NOT_INSTALLED.to_string())
}

/// Active-or-not connections for the signed-in user.
///
/// # Errors
///
/// Returns `Err` when no host is installed, or the upstream call fails.
pub async fn list_connections(config: &Config) -> Result<Vec<ComposioConnection>, String> {
    require_composio_host()?.list_connections(config).await
}

/// Execute a Composio tool.
///
/// # Errors
///
/// Returns `Err` when no host is installed, or the call fails.
pub async fn execute(
    config: &Config,
    tool: &str,
    arguments: Option<serde_json::Value>,
    entity_id: &str,
    connection_id: Option<&str>,
) -> Result<ComposioExecuteResponse, String> {
    require_composio_host()?
        .execute(config, tool, arguments, entity_id, connection_id)
        .await
}

/// The direct-mode API key, or `None` when unset or unwired.
#[must_use]
pub fn api_key(config: &Config) -> Option<String> {
    composio_host()?.api_key(config)
}

/// The backend bearer from the installed host, or `None` when no host is
/// installed or the host has no session.
///
/// The two are deliberately not distinguished here: both mean "this process
/// cannot authenticate a proxied Composio call right now", and the caller's
/// fallback and error message are the same either way.
#[must_use]
pub fn session_bearer(config: &Config) -> Option<String> {
    composio_host()?.session_bearer(config)
}

/// Whether a viable Composio client resolves. `false` when unwired.
#[must_use]
pub fn is_available(config: &Config) -> bool {
    composio_host().is_some_and(|host| host.is_available(config))
}
