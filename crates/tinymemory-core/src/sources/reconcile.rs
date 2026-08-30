//! Startup reconciliation of Composio connections into the memory sources registry.
//!
//! Called once at boot to ensure all active Composio sync targets have
//! a corresponding `MemorySourceEntry` in config. This catches connections
//! created before the memory_sources domain existed.
//!
//! Also owns the retroactive caps migration
//! (`apply_composio_source_caps_migration`) that gives any cap-less Composio
//! source — enabled or disabled — conservative per-toolkit caps.

use crate::config_loader as config_rpc;
use crate::sources::registry;
use crate::sources::types::{MemorySourceEntry, SourceKind};

/// Current version of the caps migration. Bump when the migration logic changes
/// so installs that ran an earlier revision re-run it exactly once.
const CURRENT_CAPS_MIGRATION_VERSION: u32 = 1;

/// Apply conservative default caps in-place to every cap-less source.
///
/// For a Composio source with no `max_items`/`sync_depth_days`, writes the
/// per-toolkit defaults and enables it (a no-op when already enabled) — an
/// already-enabled, cap-less source would otherwise sync at the provider's large
/// internal ceiling instead of the cheap default. For other kinds, fills any unset
/// kind-specific caps via `apply_kind_defaults`. User-customised caps (non-None)
/// are never overwritten. Returns the number of Composio entries that received
/// defaults. Pure (no I/O) so it can be unit-tested directly.
fn apply_caps_defaults_to_entries(sources: &mut [MemorySourceEntry]) -> u32 {
    let mut applied = 0u32;
    for source in sources.iter_mut() {
        match source.kind {
            SourceKind::Composio => {
                // Apply to enabled AND disabled cap-less sources; skip entries the
                // user has already customised (any non-None cap).
                if source.max_items.is_none() && source.sync_depth_days.is_none() {
                    let toolkit = source.toolkit.as_deref().unwrap_or("");
                    let (max_items, sync_depth_days) =
                        registry::memory_sync_defaults_for_toolkit(toolkit);
                    tracing::debug!(
                        id = %source.id,
                        toolkit = %toolkit,
                        was_enabled = source.enabled,
                        max_items = ?max_items,
                        sync_depth_days = ?sync_depth_days,
                        "[memory_sources:reconcile] caps migration: applying conservative defaults"
                    );
                    source.enabled = true;
                    source.max_items = max_items;
                    source.sync_depth_days = sync_depth_days;
                    applied += 1;
                }
            }
            // Apply non-composio kind defaults for entries with all-None caps.
            _ => {
                // Use the rpc::apply_kind_defaults helper so the same
                // conservative values are applied consistently.
                crate::sources::apply_kind_defaults(source);
            }
        }
    }
    applied
}

/// Retroactive migration: give any cap-less Composio source — enabled or
/// disabled — conservative per-toolkit caps so its first sync stays cheap.
///
/// Version-gated by `Config.composio_source_caps_migration_version`: runs once per
/// `CURRENT_CAPS_MIGRATION_VERSION` bump (installs that ran an earlier revision
/// re-run it exactly once). Entries the user has already customised (non-None caps)
/// are left untouched.
pub async fn apply_composio_source_caps_migration() -> Result<(), String> {
    let _guard = registry::memory_sources_write_guard().await;
    let mut config = config_rpc::load_config_with_timeout().await?;

    if config.composio_source_caps_migration_version() >= CURRENT_CAPS_MIGRATION_VERSION {
        tracing::debug!(
            version = config.composio_source_caps_migration_version(),
            "[memory_sources:reconcile] caps migration already at current version; skipping"
        );
        return Ok(());
    }

    tracing::info!(
        from_version = config.composio_source_caps_migration_version(),
        to_version = CURRENT_CAPS_MIGRATION_VERSION,
        "[memory_sources:reconcile] applying composio source caps migration"
    );

    // The source registry crosses the host seam as JSON. `MemorySourceEntry` is
    // defined by the engine crate, which `tinymemory-api` must not depend on
    // (it would drag SQLite into the dependency-light contract crate), so the
    // host hands the registry over serialized and takes it back the same way.
    let mut entries: Vec<MemorySourceEntry> = serde_json::from_value(
        config
            .memory_sources_json()
            .map_err(|e| format!("caps migration: failed to read memory sources: {e:#}"))?,
    )
    .map_err(|e| format!("caps migration: failed to decode memory sources: {e:#}"))?;

    let migrated_count = apply_caps_defaults_to_entries(&mut entries);

    config
        .set_memory_sources_json(
            serde_json::to_value(&entries)
                .map_err(|e| format!("caps migration: failed to encode memory sources: {e:#}"))?,
        )
        .map_err(|e| format!("caps migration: failed to write memory sources: {e:#}"))?;
    config.set_composio_source_caps_migration_version(CURRENT_CAPS_MIGRATION_VERSION);
    config
        .save()
        .await
        .map_err(|e| format!("caps migration: failed to save config: {e:#}"))?;

    tracing::info!(
        migrated = migrated_count,
        "[memory_sources:reconcile] caps migration complete"
    );

    Ok(())
}

fn title_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().chain(chars).collect(),
    }
}

fn short_id(id: &str) -> &str {
    // Show only the last 8 Unicode scalar values to keep labels compact.
    // Byte-slicing would panic if the cut point isn't a UTF-8 boundary.
    let n = id.chars().count();
    if n <= 8 {
        return id;
    }
    let skip = n - 8;
    let start = id.char_indices().nth(skip).map(|(idx, _)| idx).unwrap_or(0);
    &id[start..]
}

#[cfg(test)]
#[path = "reconcile_tests.rs"]
mod tests;
