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

use crate::error::MemoryError;

// The value types this family exchanges. They are defined in `tinymemory-bus`
// — they cross the module boundary, and a host that only makes calls must be
// able to name them without compiling this trait — and re-exported here so
// every historical path keeps resolving and the types stay the same types.
pub use tinymemory_bus::provider::profile::{FacetState, FacetType, ProfileFacet, UserState};

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
