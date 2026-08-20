//! Field rules for a configured source (#18 §B4).
//!
//! Moved from the engine with the types they validate. `ensure_within_base`
//! came with them: it returned the engine's `SourceResult`, and the
//! contract's `MemoryError` already carries the `PathEscape` variant it needs,
//! so the retype is exact rather than a widening.

use std::path::{Path, PathBuf};

use tinymemory_api::error::MemoryError;

use super::types::{MemorySourceEntry, SourceKind};

/// Validate required fields for `entry` based on its [`SourceKind`].
///
/// Returns a human-readable error message describing the first failing rule.
/// `id` and `label` are required for every kind; kind-specific fields follow.
///
pub fn validate_entry(entry: &MemorySourceEntry) -> Result<(), String> {
    if entry.id.trim().is_empty() {
        return Err("id is required".to_string());
    }
    if entry.id.contains(':') || entry.id.chars().any(char::is_control) {
        return Err("id must not contain ':' or control characters".to_string());
    }
    if entry.label.is_empty() {
        return Err("label is required".to_string());
    }
    match entry.kind {
        SourceKind::Composio => {
            require_field(&entry.toolkit, "toolkit")?;
            require_field(&entry.connection_id, "connection_id")?;
        }
        SourceKind::Conversation => {
            // No kind-specific required fields — just enabled/disabled.
        }
        SourceKind::Folder => {
            require_field(&entry.path, "path")?;
        }
        SourceKind::GithubRepo => {
            require_field(&entry.url, "url")?;
        }
        SourceKind::TwitterQuery => {
            require_field(&entry.query, "query")?;
        }
        SourceKind::RssFeed => {
            require_field(&entry.url, "url")?;
        }
        SourceKind::WebPage => {
            require_field(&entry.url, "url")?;
        }
    }
    Ok(())
}

/// Require that `value` is present and non-empty, naming it `name` in errors.
fn require_field(value: &Option<String>, name: &str) -> Result<(), String> {
    match value {
        Some(v) if !v.is_empty() => Ok(()),
        _ => Err(format!("{name} is required for this source kind")),
    }
}

/// Canonicalize `target` and ensure it stays within canonicalized `base`.
///
/// This is the shared path-traversal guard for local readers. Both paths must
/// exist (they are passed through [`std::fs::canonicalize`], which resolves
/// symlinks and `..` segments). If the resolved target escapes the base
/// directory, a [`MemoryError::PathEscape`] carrying `"path traversal denied"`
/// is returned.
pub fn ensure_within_base(base: &Path, target: &Path) -> Result<PathBuf, MemoryError> {
    let canonical_base = std::fs::canonicalize(base)?;
    let canonical_target = std::fs::canonicalize(target)?;
    if !canonical_target.starts_with(&canonical_base) {
        return Err(MemoryError::PathEscape("path traversal denied".to_string()));
    }
    Ok(canonical_target)
}

#[cfg(test)]
#[path = "validation_tests.rs"]
mod tests;
