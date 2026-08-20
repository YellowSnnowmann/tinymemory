//! The markdown summary tree: append, query, drill down, seal, cascade.
//!
//! One [`BusCall`] per member; see [`crate::calls`] for how they are used.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use tinymemory_api::chunks::Chunk;
use tinymemory_api::provider::types::SourceScope;
use tinymemory_api::tree::{IngestRequest, QueryResult, TreeStatus};

use crate::calls::BusCall;
use crate::error::Error;
use crate::names::methods;

/// Arguments for `Append`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Append {
    /// The `request` argument — wire position 0.
    pub request: IngestRequest,
}

impl BusCall for Append {
    const METHOD: &'static str = methods::APPEND;

    type Response = ();

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.request,)).map_err(Error::Encode)
    }
}

/// Arguments for `QuerySource`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuerySource {
    /// The `namespace` argument — wire position 0.
    pub namespace: String,
    /// The `source_id` argument — wire position 1.
    pub source_id: String,
    /// The `limit` argument — wire position 2.
    pub limit: usize,
    /// The `scope` argument — wire position 3.
    pub scope: Option<SourceScope>,
}

impl BusCall for QuerySource {
    const METHOD: &'static str = methods::QUERY_SOURCE;

    type Response = Vec<Chunk>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.namespace, self.source_id, self.limit, self.scope)).map_err(Error::Encode)
    }
}

/// Arguments for `DrillDown`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrillDown {
    /// The `namespace` argument — wire position 0.
    pub namespace: String,
    /// The `node_id` argument — wire position 1.
    pub node_id: String,
}

impl BusCall for DrillDown {
    const METHOD: &'static str = methods::DRILL_DOWN;

    type Response = QueryResult;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.namespace, self.node_id)).map_err(Error::Encode)
    }
}

/// Arguments for `Seal`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Seal {
    /// The `namespace` argument — wire position 0.
    pub namespace: String,
}

impl BusCall for Seal {
    const METHOD: &'static str = methods::SEAL;

    type Response = TreeStatus;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.namespace,)).map_err(Error::Encode)
    }
}

/// Arguments for `Cascade`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cascade {
    /// The `namespace` argument — wire position 0.
    pub namespace: String,
}

impl BusCall for Cascade {
    const METHOD: &'static str = methods::CASCADE;

    type Response = TreeStatus;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.namespace,)).map_err(Error::Encode)
    }
}
