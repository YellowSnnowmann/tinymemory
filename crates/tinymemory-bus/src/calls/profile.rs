//! Profile facets and their provenance.
//!
//! One [`BusCall`] per member; see [`crate::calls`] for how they are used.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::calls::BusCall;
use crate::error::Error;
use crate::names::methods;
use crate::types;

/// Arguments for `ListActiveFacets`.
///
/// Takes no arguments, so it encodes as an empty positional array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListActiveFacets;

impl BusCall for ListActiveFacets {
    const METHOD: &'static str = methods::LIST_ACTIVE_FACETS;

    type Response = Vec<types::ProfileFacet>;

    fn into_args(self) -> crate::Result<Value> {
        Ok(Value::Array(Vec::new()))
    }
}

/// Arguments for `ListAllFacets`.
///
/// Takes no arguments, so it encodes as an empty positional array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListAllFacets;

impl BusCall for ListAllFacets {
    const METHOD: &'static str = methods::LIST_ALL_FACETS;

    type Response = Vec<types::ProfileFacet>;

    fn into_args(self) -> crate::Result<Value> {
        Ok(Value::Array(Vec::new()))
    }
}

/// Arguments for `GetFacet`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetFacet {
    /// The `key` argument — wire position 0.
    pub key: String,
}

impl BusCall for GetFacet {
    const METHOD: &'static str = methods::GET_FACET;

    type Response = Option<types::ProfileFacet>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.key,)).map_err(Error::Encode)
    }
}

/// Arguments for `FacetsByType`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FacetsByType {
    /// The `facet_type` argument — wire position 0.
    pub facet_type: types::FacetType,
}

impl BusCall for FacetsByType {
    const METHOD: &'static str = methods::FACETS_BY_TYPE;

    type Response = Vec<types::ProfileFacet>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.facet_type,)).map_err(Error::Encode)
    }
}

/// Arguments for `UpsertFacet`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertFacet {
    /// The `facet` argument — wire position 0.
    pub facet: types::ProfileFacet,
}

impl BusCall for UpsertFacet {
    const METHOD: &'static str = methods::UPSERT_FACET;

    type Response = ();

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.facet,)).map_err(Error::Encode)
    }
}

/// Arguments for `UpsertProviderFacet`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertProviderFacet {
    /// The `facet_id` argument — wire position 0.
    pub facet_id: String,
    /// The `facet_type` argument — wire position 1.
    pub facet_type: types::FacetType,
    /// The `key` argument — wire position 2.
    pub key: String,
    /// The `value` argument — wire position 3.
    pub value: String,
    /// The `confidence` argument — wire position 4.
    pub confidence: f64,
    /// The `segment_id` argument — wire position 5.
    pub segment_id: Option<String>,
    /// The `observed_at` argument — wire position 6.
    pub observed_at: f64,
}

impl BusCall for UpsertProviderFacet {
    const METHOD: &'static str = methods::UPSERT_PROVIDER_FACET;

    type Response = ();

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.facet_id, self.facet_type, self.key, self.value, self.confidence, self.segment_id, self.observed_at)).map_err(Error::Encode)
    }
}

/// Arguments for `SetFacetUserState`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetFacetUserState {
    /// The `key` argument — wire position 0.
    pub key: String,
    /// The `user_state` argument — wire position 1.
    pub user_state: types::UserState,
}

impl BusCall for SetFacetUserState {
    const METHOD: &'static str = methods::SET_FACET_USER_STATE;

    type Response = bool;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.key, self.user_state)).map_err(Error::Encode)
    }
}

/// Arguments for `DeleteFacet`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteFacet {
    /// The `key` argument — wire position 0.
    pub key: String,
}

impl BusCall for DeleteFacet {
    const METHOD: &'static str = methods::DELETE_FACET;

    type Response = bool;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.key,)).map_err(Error::Encode)
    }
}

/// Arguments for `DeleteFacetById`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteFacetById {
    /// The `facet_id` argument — wire position 0.
    pub facet_id: String,
}

impl BusCall for DeleteFacetById {
    const METHOD: &'static str = methods::DELETE_FACET_BY_ID;

    type Response = bool;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.facet_id,)).map_err(Error::Encode)
    }
}

/// Arguments for `DropFacetsBelow`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropFacetsBelow {
    /// The `threshold` argument — wire position 0.
    pub threshold: f64,
}

impl BusCall for DropFacetsBelow {
    const METHOD: &'static str = methods::DROP_FACETS_BELOW;

    type Response = usize;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.threshold,)).map_err(Error::Encode)
    }
}

/// Arguments for `WorkflowIdentityMatches`.
///
/// Returns `bool`, not `BusResult<bool>` on the trait — but the wire needs a
/// result, so an absent family answers `false` rather than erroring, which
/// is the trait's documented reading of "cannot tell" for this predicate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowIdentityMatches {
    /// The `key_pattern` argument — wire position 0.
    pub key_pattern: String,
    /// The `canonical_value` argument — wire position 1.
    pub canonical_value: String,
}

impl BusCall for WorkflowIdentityMatches {
    const METHOD: &'static str = methods::WORKFLOW_IDENTITY_MATCHES;

    type Response = bool;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.key_pattern, self.canonical_value)).map_err(Error::Encode)
    }
}
