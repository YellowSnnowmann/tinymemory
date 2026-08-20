//! Entities, relations and the namespaced key/value store.
//!
//! One [`BusCall`] per member; see [`crate::calls`] for how they are used.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::calls::BusCall;
use crate::error::Error;
use crate::names::methods;
use crate::types;

/// Arguments for `Entities`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entities {
    /// The `namespace` argument — wire position 0.
    pub namespace: String,
    /// The `query` argument — wire position 1.
    pub query: Option<String>,
    /// The `limit` argument — wire position 2.
    pub limit: usize,
}

impl BusCall for Entities {
    const METHOD: &'static str = methods::ENTITIES;

    type Response = Vec<types::EntityHit>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.namespace, self.query, self.limit)).map_err(Error::Encode)
    }
}

/// Arguments for `EntityEdges`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityEdges {
    /// The `namespace` argument — wire position 0.
    pub namespace: String,
    /// The `entity_id` argument — wire position 1.
    pub entity_id: String,
    /// The `limit` argument — wire position 2.
    pub limit: usize,
}

impl BusCall for EntityEdges {
    const METHOD: &'static str = methods::ENTITY_EDGES;

    type Response = Vec<types::GraphRelationRecord>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.namespace, self.entity_id, self.limit)).map_err(Error::Encode)
    }
}

/// Arguments for `TouchEntities`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TouchEntities {
    /// The `namespace` argument — wire position 0.
    pub namespace: String,
    /// The `entity_ids` argument — wire position 1.
    pub entity_ids: Vec<String>,
}

impl BusCall for TouchEntities {
    const METHOD: &'static str = methods::TOUCH_ENTITIES;

    type Response = ();

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.namespace, self.entity_ids)).map_err(Error::Encode)
    }
}

/// Arguments for `SearchEntities`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchEntities {
    /// The `query` argument — wire position 0.
    pub query: String,
    /// The `kinds` argument — wire position 1.
    pub kinds: Option<Vec<String>>,
    /// The `limit` argument — wire position 2.
    pub limit: usize,
}

impl BusCall for SearchEntities {
    const METHOD: &'static str = methods::SEARCH_ENTITIES;

    type Response = Vec<types::EntityMatch>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.query, self.kinds, self.limit)).map_err(Error::Encode)
    }
}

/// Arguments for `Relations`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relations {
    /// The `namespace` argument — wire position 0.
    pub namespace: Option<String>,
    /// The `subject` argument — wire position 1.
    pub subject: Option<String>,
    /// The `predicate` argument — wire position 2.
    pub predicate: Option<String>,
    /// The `limit` argument — wire position 3.
    pub limit: usize,
}

impl BusCall for Relations {
    const METHOD: &'static str = methods::RELATIONS;

    type Response = Vec<types::GraphRelationRecord>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.namespace, self.subject, self.predicate, self.limit)).map_err(Error::Encode)
    }
}

/// Arguments for `PutRelation`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutRelation {
    /// The `relation` argument — wire position 0.
    pub relation: types::GraphRelationRecord,
}

impl BusCall for PutRelation {
    const METHOD: &'static str = methods::PUT_RELATION;

    type Response = ();

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.relation,)).map_err(Error::Encode)
    }
}

/// Arguments for `KvGet`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvGet {
    /// The `namespace` argument — wire position 0.
    pub namespace: Option<String>,
    /// The `key` argument — wire position 1.
    pub key: String,
}

impl BusCall for KvGet {
    const METHOD: &'static str = methods::KV_GET;

    type Response = Option<types::MemoryKvRecord>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.namespace, self.key)).map_err(Error::Encode)
    }
}

/// Arguments for `KvPut`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvPut {
    /// The `namespace` argument — wire position 0.
    pub namespace: Option<String>,
    /// The `key` argument — wire position 1.
    pub key: String,
    /// The `value` argument — wire position 2.
    pub value: Value,
}

impl BusCall for KvPut {
    const METHOD: &'static str = methods::KV_PUT;

    type Response = ();

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.namespace, self.key, self.value)).map_err(Error::Encode)
    }
}

/// Arguments for `KvDelete`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvDelete {
    /// The `namespace` argument — wire position 0.
    pub namespace: Option<String>,
    /// The `key` argument — wire position 1.
    pub key: String,
}

impl BusCall for KvDelete {
    const METHOD: &'static str = methods::KV_DELETE;

    type Response = bool;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.namespace, self.key)).map_err(Error::Encode)
    }
}

/// Arguments for `KvList`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvList {
    /// The `namespace` argument — wire position 0.
    pub namespace: Option<String>,
    /// The `prefix` argument — wire position 1.
    pub prefix: Option<String>,
    /// The `limit` argument — wire position 2.
    pub limit: usize,
}

impl BusCall for KvList {
    const METHOD: &'static str = methods::KV_LIST;

    type Response = Vec<types::MemoryKvRecord>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.namespace, self.prefix, self.limit)).map_err(Error::Encode)
    }
}
