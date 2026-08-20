//! The scored retrieval surface.
//!
//! One [`BusCall`] per member; see [`crate::calls`] for how they are used.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use tinymemory_api::provider::retrieval::{CoverWindowQuery, FastRetrieveQuery, RetrievalHit, RetrievalResponse, SourceRetrievalQuery};
use tinymemory_api::provider::types::SourceScope;

use crate::calls::BusCall;
use crate::error::Error;
use crate::names::methods;

/// Arguments for `FastRetrieve`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FastRetrieve {
    /// The `query` argument — wire position 0.
    pub query: String,
    /// The `options` argument — wire position 1.
    pub options: FastRetrieveQuery,
    /// The `scope` argument — wire position 2.
    pub scope: Option<SourceScope>,
}

impl BusCall for FastRetrieve {
    const METHOD: &'static str = methods::FAST_RETRIEVE;

    type Response = RetrievalResponse;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.query, self.options, self.scope)).map_err(Error::Encode)
    }
}

/// Arguments for `CoverWindow`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverWindow {
    /// The `window` argument — wire position 0.
    pub window: CoverWindowQuery,
    /// The `scope` argument — wire position 1.
    pub scope: Option<SourceScope>,
}

impl BusCall for CoverWindow {
    const METHOD: &'static str = methods::COVER_WINDOW;

    type Response = RetrievalResponse;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.window, self.scope)).map_err(Error::Encode)
    }
}

/// Arguments for `RetrieveSource`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrieveSource {
    /// The `query` argument — wire position 0.
    pub query: SourceRetrievalQuery,
    /// The `scope` argument — wire position 1.
    pub scope: Option<SourceScope>,
}

impl BusCall for RetrieveSource {
    const METHOD: &'static str = methods::RETRIEVE_SOURCE;

    type Response = RetrievalResponse;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.query, self.scope)).map_err(Error::Encode)
    }
}

/// Arguments for `RetrieveChildren`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrieveChildren {
    /// The `node_id` argument — wire position 0.
    pub node_id: String,
    /// The `max_depth` argument — wire position 1.
    pub max_depth: u32,
    /// The `query` argument — wire position 2.
    pub query: Option<String>,
    /// The `limit` argument — wire position 3.
    pub limit: Option<usize>,
    /// The `scope` argument — wire position 4.
    pub scope: Option<SourceScope>,
}

impl BusCall for RetrieveChildren {
    const METHOD: &'static str = methods::RETRIEVE_CHILDREN;

    type Response = Vec<RetrievalHit>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.node_id, self.max_depth, self.query, self.limit, self.scope)).map_err(Error::Encode)
    }
}

/// Arguments for `RetrieveLeaves`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrieveLeaves {
    /// The `chunk_ids` argument — wire position 0.
    pub chunk_ids: Vec<String>,
    /// The `scope` argument — wire position 1.
    pub scope: Option<SourceScope>,
}

impl BusCall for RetrieveLeaves {
    const METHOD: &'static str = methods::RETRIEVE_LEAVES;

    type Response = Vec<RetrievalHit>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.chunk_ids, self.scope)).map_err(Error::Encode)
    }
}
