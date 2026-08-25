//! Cursor, dedup and daily-budget state for Composio sync (#18 §B2).
//!
//! Engine-neutral, persisted through the [`SyncStateStore`] KV seam — any
//! provider whose KV family can get and set a JSON value can carry sync state.
//!
//! The engine keeps its own copy of the shape for its internal pipelines until
//! §B1's orchestrator move retires them. The two persist under the same KV
//! namespace with the same serde form; the pin tests either side hold this copy
//! to that contract.
//!
//! # Where the shape lives (#5560)
//!
//! [`SyncState`], [`DailyBudget`], the namespaces and [`extract_item_id`] are
//! defined in [`tinymemory_api::composio::state`] and re-exported here. Both
//! sides read them: the module advances the cursor and spends the budget, while
//! OpenHuman renders "312 of 500 requests used today" and, on disconnect, walks
//! the dedup set to decide what to forget. A host-side twin would decode today
//! and diverge on the first added field — and because this shape is
//! *persisted*, divergence is a stranded cursor and a re-ingested inbox rather
//! than a wire error someone notices.
//!
//! What stayed here is the I/O: the [`SyncStateStore`] seam and the two methods
//! that use it, offered as the [`PersistedSyncState`] extension trait because
//! an inherent `impl` has to live in the crate that defines the type. Call
//! sites are unchanged — `SyncState::load(store, …)` and `state.save(store)`
//! still resolve — but the trait has to be in scope, so the four call sites in
//! this crate import it alongside the type.

use async_trait::async_trait;

/// The persisted sync-state shape, defined in the contract crate.
///
/// Re-exported at this path so every historical
/// `providers::sync_state::SyncState` reference keeps resolving.
pub use tinymemory_api::composio::state::{
    extract_item_id, DailyBudget, SyncState, DEFAULT_DAILY_REQUEST_LIMIT, KV_NAMESPACE,
    STATE_NAMESPACE,
};

/// The key/value seam a [`SyncState`] is persisted through.
///
/// Deliberately narrower than a memory client: get and set one JSON value by
/// `(namespace, key)`. That is the whole requirement, and stating it as two
/// methods is what lets a non-TinyCortex driver carry Composio sync state
/// without implementing anything else.
#[async_trait]
pub trait SyncStateStore: Send + Sync {
    /// Read the value at `(namespace, key)`, or `None` when nothing is stored.
    ///
    /// # Errors
    ///
    /// Returns an error when the underlying store cannot be reached. "Nothing
    /// stored" is `Ok(None)`, not an error — a first sync is the normal case.
    async fn get(&self, namespace: &str, key: &str) -> anyhow::Result<Option<serde_json::Value>>;

    /// Write `value` at `(namespace, key)`, replacing anything already there.
    ///
    /// # Errors
    ///
    /// Returns an error when the underlying store rejects or cannot persist the
    /// write.
    async fn set(
        &self,
        namespace: &str,
        key: &str,
        value: &serde_json::Value,
    ) -> anyhow::Result<()>;
}

/// Loading and saving a [`SyncState`] through a [`SyncStateStore`].
///
/// An extension trait rather than an inherent `impl` because the type is
/// defined in the contract crate, which holds no I/O and publishes no traits.
/// The method names and signatures are the ones the inherent versions had, so
/// existing call sites only need this trait in scope.
#[async_trait]
pub trait PersistedSyncState: Sized {
    /// Load the state for one `(toolkit, connection)` pair.
    ///
    /// A connection with nothing stored yields a fresh state rather than an
    /// error — that is a first sync, not a failure. A loaded state has its
    /// daily budget rolled forward before it is returned, so what a caller
    /// spends and later writes back is today's row rather than yesterday's.
    ///
    /// # Errors
    ///
    /// Returns an error when the store cannot be reached, or when the stored
    /// value is not a decodable state. Both are genuine faults: silently
    /// starting from a fresh state would re-ingest everything the connection
    /// had already synced.
    async fn load(
        store: &dyn SyncStateStore,
        toolkit: &str,
        connection_id: &str,
    ) -> anyhow::Result<Self>;

    /// Persist this state under its `(toolkit, connection)` key.
    ///
    /// # Errors
    ///
    /// Returns an error when the state cannot be serialised or the store
    /// rejects the write.
    async fn save(&self, store: &dyn SyncStateStore) -> anyhow::Result<()>;
}

#[async_trait]
impl PersistedSyncState for SyncState {
    async fn load(
        store: &dyn SyncStateStore,
        toolkit: &str,
        connection_id: &str,
    ) -> anyhow::Result<Self> {
        let key = Self::key(toolkit, connection_id);
        match store.get(STATE_NAMESPACE, &key).await? {
            Some(value) => {
                let mut state: Self = serde_json::from_value(value)?;
                state.daily_budget.roll_over_if_stale();
                Ok(state)
            }
            None => Ok(Self::new(toolkit, connection_id)),
        }
    }

    async fn save(&self, store: &dyn SyncStateStore) -> anyhow::Result<()> {
        let value = serde_json::to_value(self)?;
        store
            .set(
                STATE_NAMESPACE,
                &Self::key(&self.toolkit, &self.connection_id),
                &value,
            )
            .await
    }
}

#[cfg(test)]
#[path = "sync_state_tests.rs"]
mod tests;
