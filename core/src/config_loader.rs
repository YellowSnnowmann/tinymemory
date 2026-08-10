//! [`ConfigLoader`] — reading a fresh config, which only the host can do.
//!
//! [`crate::Config`] is a trait object: this crate can *read* a host config but
//! has no idea how one is produced — which file, which env overrides, which
//! migrations run on load. The background loops need a fresh one anyway, not
//! the snapshot they were spawned with: a mid-session settings change (a new
//! Composio API key, a toggled sync interval) has to take effect on the next
//! tick rather than at the next restart.
//!
//! # Two methods, and why the snapshot one matters
//!
//! [`ConfigLoader::load`] resolves the config the way startup would.
//! [`ConfigLoader::reload_snapshot`] re-reads *the same file the snapshot came
//! from*. The distinction is load-bearing: `load` follows the ambient
//! environment, so in a test process — or on a host with several workspaces —
//! it can land on a different workspace than the caller is working in. Anchor
//! to the snapshot whenever one is in hand.
//!
//! # Unwired is an error
//!
//! A loop that silently kept its stale snapshot would apply the user's settings
//! change never, and look like it was working.

use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;

use crate::Config;

/// Produces host configs on the core's behalf.
#[async_trait]
pub trait ConfigLoader: Send + Sync + std::fmt::Debug {
    /// Load the config the way startup does, following the ambient
    /// environment.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the config cannot be read or times out.
    async fn load(&self) -> Result<Arc<Config>, String>;

    /// Re-read the config from the same path `snapshot` was loaded from.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the config cannot be read or times out.
    async fn reload_snapshot(&self, snapshot: &Config) -> Result<Arc<Config>, String>;
}

static LOADER: RwLock<Option<Arc<dyn ConfigLoader>>> = RwLock::new(None);

const NOT_INSTALLED: &str =
    "no ConfigLoader installed — the host must call memory::config_loader::set_config_loader \
     during startup wiring, before any background loop runs";

/// Install the host's config loader. Called once during startup wiring.
pub fn set_config_loader(loader: Arc<dyn ConfigLoader>) {
    *LOADER.write() = Some(loader);
}

/// Remove any installed loader. For tests.
pub fn clear_config_loader() {
    *LOADER.write() = None;
}

/// The installed loader, or `None` when nothing has been wired up.
#[must_use]
pub fn config_loader() -> Option<Arc<dyn ConfigLoader>> {
    LOADER.read().clone()
}

/// Load a fresh config.
///
/// # Errors
///
/// Returns `Err` when no loader is installed, or the load fails.
pub async fn load_config_with_timeout() -> Result<Arc<Config>, String> {
    let loader = config_loader().ok_or_else(|| NOT_INSTALLED.to_string())?;
    loader.load().await
}

/// Re-read the config `snapshot` came from.
///
/// # Errors
///
/// Returns `Err` when no loader is installed, or the load fails.
pub async fn reload_config_snapshot_with_timeout(snapshot: &Config) -> Result<Arc<Config>, String> {
    let loader = config_loader().ok_or_else(|| NOT_INSTALLED.to_string())?;
    loader.reload_snapshot(snapshot).await
}
