//! Which identities on a stored row are the user's own.
//!
//! # Why this is here and not with the connector
//!
//! It reads *this crate's* profile store — the `skill:<toolkit>:<connection>:
//! <kind>` rows written when a connected account's profile is ingested — and
//! answers a question the memory tree asks while building entity rows: is this
//! email address the user themselves?
//!
//! It lived under the Composio sync tree only because that is what wrote the
//! rows. Writing them is the connector module's job now; reading them was
//! never anything but memory's, so it stays.

use serde::{Deserialize, Serialize};

/// The facet kinds a connected account's profile is stored as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityKind {
    /// Platform-canonical immutable id — Slack `U123ABC`, Notion UUID.
    UserId,
    Email,
    /// `@`-style screen name, canonicalised without the leading `@`.
    Handle,
    /// E.164 phone number.
    Phone,
    /// Human display label. Weak signal — never auto-promotes to is_self.
    DisplayName,
    /// Not for matching; kept for UI / prompt rendering.
    AvatarUrl,
    /// Not for matching; kept for UI / prompt rendering.
    ProfileUrl,
}

impl IdentityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserId => "user_id",
            Self::Email => "email",
            Self::Handle => "handle",
            Self::Phone => "phone",
            Self::DisplayName => "display_name",
            Self::AvatarUrl => "avatar_url",
            Self::ProfileUrl => "profile_url",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "user_id" => Self::UserId,
            "email" => Self::Email,
            "handle" => Self::Handle,
            "phone" => Self::Phone,
            "display_name" => Self::DisplayName,
            "avatar_url" => Self::AvatarUrl,
            "profile_url" => Self::ProfileUrl,
            _ => return None,
        })
    }

    /// Confidence the matcher records on the row. Hard kinds auto-promote
    /// a chunk to `is_self`; weak kinds require corroboration.
    pub fn confidence(self) -> f64 {
        match self {
            Self::UserId | Self::Phone => 1.00,
            Self::Email => 0.95,
            Self::Handle => 0.70,
            Self::DisplayName => 0.40,
            Self::AvatarUrl | Self::ProfileUrl => 0.50,
        }
    }

    /// True if this kind is a real identity signal worth running through
    /// the matcher (vs. UI-only fields).
    pub fn is_matchable(self) -> bool {
        matches!(
            self,
            Self::UserId | Self::Email | Self::Handle | Self::Phone | Self::DisplayName
        )
    }
}

/// Canonicalize a raw value for storage and lookup. The same routine runs
/// on the entity side at match time, so equality of canonical forms is the
/// matcher's only test — no `COLLATE NOCASE`, no per-call lowercasing.
pub fn canonicalize(kind: IdentityKind, raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(match kind {
        IdentityKind::Email => trimmed.to_lowercase(),
        IdentityKind::Handle => trimmed.trim_start_matches('@').to_lowercase(),
        IdentityKind::Phone => trimmed
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '+')
            .collect(),
        IdentityKind::DisplayName => trimmed.split_whitespace().collect::<Vec<_>>().join(" "),
        IdentityKind::UserId | IdentityKind::AvatarUrl | IdentityKind::ProfileUrl => {
            trimmed.to_string()
        }
    })
}

/// Cross-toolkit variant — matches against every connected provider's
/// rows of this kind. Used for marking memory-tree entity rows: an email
/// in a Slack message that matches the user's Gmail address is still
/// "me," regardless of which source produced the chunk.
pub fn is_self_identity_any_toolkit(kind: IdentityKind, raw_value: &str) -> bool {
    if !kind.is_matchable() {
        return false;
    }
    let Some(canonical) = canonicalize(kind, raw_value) else {
        return false;
    };
    let Some(client) = crate::global::client_if_ready() else {
        return false;
    };
    let key_pattern = format!("skill:%:%:{}", kind.as_str());
    client
        .profile_store()
        .skill_identity_matches(&key_pattern, &canonical)
}

/// Render a compact section for prompt injection. Skips `user_id` (not
/// human-readable), prefixes `handle` with `@`.
/// Fold a token to the shape used in a profile-store key.
pub fn normalize_token(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() || lower == '-' || lower == '_' {
            out.push(lower);
        } else {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

/// Fold a connection identifier the same way.
#[must_use]
pub fn normalize_connection_identifier(raw: &str) -> String {
    normalize_token(raw)
}
