//! Compatibility exports for sync state now owned by tinycortex.

pub use crate::engine::backend::sync::state::DEFAULT_DAILY_REQUEST_LIMIT;
pub use crate::engine::backend::sync::{DailyBudget, SyncState};

pub const KV_NAMESPACE: &str = crate::engine::HOST_SYNC_STATE_NAMESPACE;

pub fn extract_item_id(item: &serde_json::Value, paths: &[&str]) -> Option<String> {
    paths.iter().find_map(|path| {
        let value = path
            .split('.')
            .try_fold(item, |current, segment| current.get(segment))?;
        value
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

#[cfg(test)]
mod tests {
    /// The namespace is durable, so changing it is a data migration.
    ///
    /// `KV_NAMESPACE` now re-exports the engine's constant, which makes host
    /// and engine agree by construction — they previously agreed only because
    /// two separate `const`s happened to hold the same literal. This pins the
    /// *value* as well: every persisted Composio sync cursor lives under this
    /// string, so a change upstream silently strands all of them. Failing here
    /// turns that into a deliberate decision with a migration attached rather
    /// than a quiet loss discovered when a sync re-runs from the beginning.
    #[test]
    fn the_state_namespace_is_pinned() {
        assert_eq!(
            super::KV_NAMESPACE,
            "composio-sync-state",
            "the Composio sync-state KV namespace changed; every persisted \
             cursor is stored under the old value and needs migrating"
        );
    }
}
