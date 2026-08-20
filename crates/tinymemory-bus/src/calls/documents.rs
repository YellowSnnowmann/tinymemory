//! Namespace-scoped document storage and retrieval.
//!
//! One [`BusCall`] per member; see [`crate::calls`] for how they are used.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::calls::BusCall;
use crate::error::Error;
use crate::names::methods;
use crate::types;

/// Arguments for `PutDocument`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutDocument {
    /// The `input` argument — wire position 0.
    pub input: types::NamespaceDocumentInput,
}

impl BusCall for PutDocument {
    const METHOD: &'static str = methods::PUT_DOCUMENT;

    type Response = String;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.input,)).map_err(Error::Encode)
    }
}

/// Arguments for `GetDocument`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetDocument {
    /// The `namespace` argument — wire position 0.
    pub namespace: String,
    /// The `key` argument — wire position 1.
    pub key: String,
}

impl BusCall for GetDocument {
    const METHOD: &'static str = methods::GET_DOCUMENT;

    type Response = Option<types::StoredMemoryDocument>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.namespace, self.key)).map_err(Error::Encode)
    }
}

/// Arguments for `ListDocuments`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListDocuments {
    /// The `namespace` argument — wire position 0.
    pub namespace: Option<String>,
}

impl BusCall for ListDocuments {
    const METHOD: &'static str = methods::LIST_DOCUMENTS;

    type Response = Value;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.namespace,)).map_err(Error::Encode)
    }
}

/// Arguments for `ListNamespaces`.
///
/// Takes no arguments, so it encodes as an empty positional array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListNamespaces;

impl BusCall for ListNamespaces {
    const METHOD: &'static str = methods::LIST_NAMESPACES;

    type Response = Vec<String>;

    fn into_args(self) -> crate::Result<Value> {
        Ok(Value::Array(Vec::new()))
    }
}

/// Arguments for `DeleteDocument`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteDocument {
    /// The `namespace` argument — wire position 0.
    pub namespace: String,
    /// The `document_id` argument — wire position 1.
    pub document_id: String,
}

impl BusCall for DeleteDocument {
    const METHOD: &'static str = methods::DELETE_DOCUMENT;

    type Response = Value;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.namespace, self.document_id)).map_err(Error::Encode)
    }
}

/// Arguments for `ClearNamespace`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClearNamespace {
    /// The `namespace` argument — wire position 0.
    pub namespace: String,
}

impl BusCall for ClearNamespace {
    const METHOD: &'static str = methods::CLEAR_NAMESPACE;

    type Response = ();

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.namespace,)).map_err(Error::Encode)
    }
}

/// Arguments for `QueryDocuments`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryDocuments {
    /// The `namespace` argument — wire position 0.
    pub namespace: String,
    /// The `query` argument — wire position 1.
    pub query: String,
    /// The `limit` argument — wire position 2.
    pub limit: usize,
}

impl BusCall for QueryDocuments {
    const METHOD: &'static str = methods::QUERY_DOCUMENTS;

    type Response = types::NamespaceRetrievalContext;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.namespace, self.query, self.limit)).map_err(Error::Encode)
    }
}

/// Arguments for `RecallDocuments`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallDocuments {
    /// The `namespace` argument — wire position 0.
    pub namespace: String,
    /// The `limit` argument — wire position 1.
    pub limit: usize,
}

impl BusCall for RecallDocuments {
    const METHOD: &'static str = methods::RECALL_DOCUMENTS;

    type Response = types::NamespaceRetrievalContext;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.namespace, self.limit)).map_err(Error::Encode)
    }
}
