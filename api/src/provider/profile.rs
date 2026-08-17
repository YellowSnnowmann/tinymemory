//! The profile family: learned facets about the user.
//!
//! A driver advertising [`Capability::Profile`](crate::capabilities::Capability::Profile)
//! stores *facets* — small learned claims like a preferred verbosity, a role,
//! a tool the user reaches for — each carrying the evidence behind it, a
//! stability score, and a lifecycle state.
//!
//! # The host owns the learning; the driver owns the rows
//!
//! Which facets to extract, how to score stability, when to promote or evict —
//! all of that is host policy and stays there. This family is the persistence
//! seam beneath it: read facets, write facets, set the user's override, drop
//! what fell below a threshold.
//!
//! That split is why [`ProfileFacet`] carries a `stability` and a `state` the
//! driver never computes. It records what the host decided; it does not decide.
//!
//! # `user_state` is the user's, and outranks the score
//!
//! [`UserState::Pinned`] and [`UserState::Forgotten`] are explicit user
//! decisions. A pinned facet stays active however low its stability falls, and
//! a forgotten one stays dropped however much new evidence arrives — a user who
//! says "forget that" must not have it re-learned.
//!
//! The two are **not** symmetric under
//! [`MemoryProfile::drop_facets_below`], and the asymmetry is deliberate: only
//! `Pinned` is protected from the sweep. A `Forgotten` facet is already in
//! [`FacetState::Dropped`] and is *meant* to be collected — protecting it would
//! keep the thing the user asked to forget on disk indefinitely.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::MemoryError;
use crate::host::EvidenceRef;

/// What kind of claim a facet makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FacetType {
    /// A stated or inferred preference.
    Preference,
    /// A way of working. Persisted as `skill` for historical reasons.
    Workflow,
    /// A role the user holds.
    Role,
    /// A personality trait.
    Personality,
    /// Ambient context about the user's situation.
    Context,
}

impl FacetType {
    /// The identifier persisted in the facet table and published on the RPC
    /// surface.
    ///
    /// **This is not the serde representation**, and the difference is
    /// deliberate: [`Self::Workflow`] serialises as `workflow` but persists as
    /// `skill`, a historical column value. Both forms are load-bearing — the
    /// serde one crosses the bus, this one reaches storage and the published
    /// JSON — so they are kept separate rather than reconciled.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preference => "preference",
            Self::Workflow => "skill",
            Self::Role => "role",
            Self::Personality => "personality",
            Self::Context => "context",
        }
    }

    /// Parse a persisted identifier; unknown values fall back to
    /// [`Self::Preference`], matching the engine's own lenient reader.
    #[must_use]
    pub fn parse_or_default(raw: &str) -> Self {
        match raw {
            "skill" => Self::Workflow,
            "role" => Self::Role,
            "personality" => Self::Personality,
            "context" => Self::Context,
            _ => Self::Preference,
        }
    }
}

/// Where a facet sits in its lifecycle, as the host's stability detector last
/// left it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FacetState {
    /// Cleared the promotion threshold; included in the ambient profile.
    #[default]
    Active,
    /// Between the provisional and promotion thresholds; included at lower
    /// weight.
    Provisional,
    /// Between eviction and provisional; held as a candidate.
    Candidate,
    /// Below the eviction threshold; removed on the next rebuild.
    Dropped,
}

impl FacetState {
    /// Stable identifier, matching the serde representation.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Provisional => "provisional",
            Self::Candidate => "candidate",
            Self::Dropped => "dropped",
        }
    }
}

/// The user's explicit override, which outranks [`FacetState`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserState {
    /// No override — the host's detector manages the lifecycle.
    #[default]
    Auto,
    /// Pinned by the user: stays active regardless of score.
    Pinned,
    /// Forgotten by the user: stays dropped, and new evidence must not
    /// re-promote it.
    Forgotten,
}

impl UserState {
    /// Stable identifier, matching the serde representation.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Pinned => "pinned",
            Self::Forgotten => "forgotten",
        }
    }
}

/// One learned claim about the user.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProfileFacet {
    /// Stable identity of this facet row.
    pub facet_id: String,
    /// What kind of claim it makes.
    pub facet_type: FacetType,
    /// The claim's key, e.g. `style/verbosity`.
    pub key: String,
    /// The claim's value.
    pub value: String,
    /// How confident the extraction was, in `[0, 1]`.
    pub confidence: f64,
    /// How many pieces of evidence support it.
    pub evidence_count: i32,
    /// Legacy segment-id references, when present.
    #[serde(default)]
    pub source_segment_ids: Option<String>,
    /// First observation, epoch seconds.
    pub first_seen_at: f64,
    /// Most recent observation, epoch seconds.
    pub last_seen_at: f64,
    /// Lifecycle state, assigned by the host.
    #[serde(default)]
    pub state: FacetState,
    /// Stability score from the host's last rebuild.
    #[serde(default)]
    pub stability: f64,
    /// The user's override.
    #[serde(default)]
    pub user_state: UserState,
    /// Where the evidence came from.
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
    /// Facet class derived from the key prefix (`style`, `identity`, …).
    /// `None` for rows whose key prefix matches no known class.
    #[serde(default)]
    pub class: Option<String>,
    /// Per-cue-family evidence counts, once the host has written a rebuild.
    #[serde(default)]
    pub cue_families: Option<HashMap<String, u32>>,
}

/// Learned facets about the user.
///
/// Reached through [`MemoryProvider::as_profile`](super::MemoryProvider::as_profile).
#[async_trait]
pub trait MemoryProfile: Send + Sync {
    /// Facets in [`FacetState::Active`], most stable first.
    ///
    /// # Errors
    ///
    /// Backend failures only.
    async fn list_active_facets(&self) -> Result<Vec<ProfileFacet>, MemoryError>;

    /// Every facet regardless of state, most stable first.
    ///
    /// # Errors
    ///
    /// Backend failures only.
    async fn list_all_facets(&self) -> Result<Vec<ProfileFacet>, MemoryError>;

    /// One facet by key.
    ///
    /// # Errors
    ///
    /// Backend failures only; an unknown key yields `Ok(None)`.
    async fn get_facet(&self, key: &str) -> Result<Option<ProfileFacet>, MemoryError>;

    /// Facets of one type, most evidence first.
    ///
    /// # Errors
    ///
    /// Backend failures only.
    async fn facets_by_type(&self, facet_type: FacetType)
        -> Result<Vec<ProfileFacet>, MemoryError>;

    /// Insert or replace a facet wholesale, including host-computed fields.
    ///
    /// # Errors
    ///
    /// Backend failures only.
    async fn upsert_facet(&self, facet: &ProfileFacet) -> Result<(), MemoryError>;

    /// Confidence-aware upsert of a provider-sourced facet.
    ///
    /// Distinct from [`Self::upsert_facet`] because a provider supplies a claim
    /// and its confidence but none of the lifecycle fields; merging is the
    /// driver's, so a lower-confidence re-observation cannot overwrite a
    /// stronger one.
    ///
    /// # Errors
    ///
    /// Backend failures only.
    #[allow(
        clippy::too_many_arguments,
        reason = "each argument is a distinct column of the facet row a provider \
                  supplies; grouping them into a struct would move the same seven \
                  fields one level out without reducing what the caller must know"
    )]
    async fn upsert_provider_facet(
        &self,
        facet_id: &str,
        facet_type: FacetType,
        key: &str,
        value: &str,
        confidence: f64,
        segment_id: Option<&str>,
        observed_at: f64,
    ) -> Result<(), MemoryError>;

    /// Set the user's override on one facet. `false` when the key is unknown.
    ///
    /// # Errors
    ///
    /// Backend failures only.
    async fn set_facet_user_state(
        &self,
        key: &str,
        user_state: UserState,
    ) -> Result<bool, MemoryError>;

    /// Delete a facet by key. `false` when the key is unknown.
    ///
    /// # Errors
    ///
    /// Backend failures only.
    async fn delete_facet(&self, key: &str) -> Result<bool, MemoryError>;

    /// Delete a facet by its `facet_id`. `false` when unknown.
    ///
    /// # Errors
    ///
    /// Backend failures only.
    async fn delete_facet_by_id(&self, facet_id: &str) -> Result<bool, MemoryError>;

    /// Drop facets whose stability is below `threshold`, returning the count.
    ///
    /// Sweeps only facets already in [`FacetState::Dropped`]: an `Active` facet
    /// below the threshold stays, because promotion and eviction are the host's
    /// decision and this call only collects what the host already evicted.
    /// [`UserState::Pinned`] is exempt; [`UserState::Forgotten`] is not — see
    /// the module docs for why those differ.
    ///
    /// # Errors
    ///
    /// Backend failures only.
    async fn drop_facets_below(&self, threshold: f64) -> Result<usize, MemoryError>;

    /// Whether any [`FacetType::Workflow`] facet's key matches `key_pattern`
    /// (a SQL `LIKE` pattern) with exactly `canonical_value`.
    ///
    /// Answers "is this row the user?". Deliberately returns `bool` rather than
    /// `Result<bool>`: every caller is a predicate whose only sane reading of a
    /// backend error is "no", and threading a `Result` through them would
    /// invite an `unwrap_or(true)` somewhere.
    async fn workflow_identity_matches(&self, key_pattern: &str, canonical_value: &str) -> bool;
}
