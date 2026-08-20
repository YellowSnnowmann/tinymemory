//! Semantic recall over stored entries.
//!
//! One [`BusCall`] per member; see [`crate::calls`] for how they are used.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::calls::BusCall;
use crate::error::Error;
use crate::names::methods;
use crate::types;

/// Arguments for `Recall`.
///
/// Ranked retrieval.
///
/// `scope` is a query predicate the driver applies internally, not a filter
/// the host may apply to the result: narrowing afterwards would let the
/// driver spend its `limit` on entries the caller is not allowed to see and
/// then return fewer than it could have.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recall {
    /// The `query` argument — wire position 0.
    pub query: String,
    /// The `limit` argument — wire position 1.
    pub limit: usize,
    /// The `opts` argument — wire position 2.
    pub opts: types::OwnedRecallOpts,
    /// The `scope` argument — wire position 3.
    pub scope: Option<types::SourceScope>,
}

impl BusCall for Recall {
    const METHOD: &'static str = methods::RECALL;

    type Response = Vec<types::MemoryEntry>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.query, self.limit, self.opts, self.scope)).map_err(Error::Encode)
    }
}

/// Arguments for `RecallNamespaceScored`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallNamespaceScored {
    /// The `namespace` argument — wire position 0.
    pub namespace: String,
    /// The `query` argument — wire position 1.
    pub query: String,
    /// The `limit` argument — wire position 2.
    pub limit: usize,
    /// The `exclude_session_id` argument — wire position 3.
    pub exclude_session_id: Option<String>,
}

impl BusCall for RecallNamespaceScored {
    const METHOD: &'static str = methods::RECALL_NAMESPACE_SCORED;

    type Response = Vec<types::NamespaceMemoryHit>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.namespace, self.query, self.limit, self.exclude_session_id)).map_err(Error::Encode)
    }
}
