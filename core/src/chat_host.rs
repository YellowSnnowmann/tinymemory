//! [`ChatHost`] — chat-model *construction*, which the host owns.
//!
//! The summary-tree summariser and the memory chat helper run LLM turns. Which
//! provider answers a given role, which model id that resolves to, and what
//! credentials it uses are host routing policy — the same policy that serves
//! every other role in the application, not something a memory engine should
//! re-derive.
//!
//! # Why this trait is here and not in `tinymemory-api`
//!
//! It names `tinyagents::harness::model::ChatModel`, and the contract crate is
//! deliberately dependency-light — it must not pull in tinyagents. This crate
//! already depends on tinyagents, so it is the one place that can name both the
//! model trait and the config seam. The host implements it here.
//!
//! Reached through a process-global for the same reason as
//! [`crate::embedding_host`]; see that module for the rationale, and for why an
//! unwired host fails loudly rather than degrading.

use std::sync::Arc;

use parking_lot::RwLock;
use tinyagents::harness::model::ChatModel;

use crate::Config;

pub use tinymemory_api::host::UsageInfo;

/// Builds chat models on the core's behalf.
pub trait ChatHost: Send + Sync + std::fmt::Debug {
    /// The provider slug that `role` currently routes to, e.g. `"cloud"`.
    ///
    /// Used for reporting and budget attribution, so it answers even when no
    /// model can actually be constructed.
    fn provider_for_role(&self, role: &str, config: &Config) -> String;

    /// Builds the chat model for `role`, returning it with its resolved model
    /// id.
    ///
    /// # Errors
    ///
    /// Returns `Err` when no provider is configured for `role`, or when the
    /// configured one cannot be constructed (missing credentials, unreachable
    /// local runtime).
    fn create_chat_model_with_model_id(
        &self,
        role: &str,
        config: &Config,
        temperature: f64,
    ) -> Result<(Arc<dyn ChatModel<()>>, String), String>;
}

static HOST: RwLock<Option<Arc<dyn ChatHost>>> = RwLock::new(None);

const NOT_INSTALLED: &str =
    "no ChatHost installed — the host must call memory::chat_host::set_chat_host during \
     startup wiring, before any summarisation runs";

/// Install the host's chat-model factory. Called once during startup wiring.
pub fn set_chat_host(host: Arc<dyn ChatHost>) {
    *HOST.write() = Some(host);
}

/// Remove any installed host. For tests.
pub fn clear_chat_host() {
    *HOST.write() = None;
}

/// The installed host, or `None` when nothing has been wired up.
#[must_use]
pub fn chat_host() -> Option<Arc<dyn ChatHost>> {
    HOST.read().clone()
}

/// The installed host.
///
/// # Errors
///
/// Returns `Err` when no host has been installed.
pub fn require_chat_host() -> Result<Arc<dyn ChatHost>, String> {
    chat_host().ok_or_else(|| NOT_INSTALLED.to_string())
}

/// The provider slug `role` routes to, or `"unknown"` with no host installed.
///
/// Unlike model construction this never fails: every caller is building a log
/// line or a status field, and an error there would be less useful than the
/// honest string.
#[must_use]
pub fn provider_for_role(role: &str, config: &Config) -> String {
    chat_host().map_or_else(
        || "unknown".to_string(),
        |host| host.provider_for_role(role, config),
    )
}

/// Builds the chat model for `role`.
///
/// # Errors
///
/// Returns `Err` when no host is installed, or when the host cannot build one.
pub fn create_chat_model_with_model_id(
    role: &str,
    config: &Config,
    temperature: f64,
) -> anyhow::Result<(Arc<dyn ChatModel<()>>, String)> {
    require_chat_host()
        .and_then(|host| host.create_chat_model_with_model_id(role, config, temperature))
        .map_err(|error| anyhow::anyhow!(error))
}

/// Serialises tests that mutate inference-related process environment. See
/// [`crate::embedding_host::embedding_test_guard`] for why this is a separate
/// lock from the host's.
#[must_use]
pub fn inference_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());
    GUARD.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
