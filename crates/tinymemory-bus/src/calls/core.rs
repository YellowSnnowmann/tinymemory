//! The mandatory key/value surface every driver implements.
//!
//! One [`BusCall`] per member; see [`crate::calls`] for how they are used.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::calls::BusCall;
use crate::error::Error;
use crate::names::methods;
use crate::types;

/// Arguments for `Store`.
///
/// Upsert an entry keyed by `(namespace, key)`.
///
/// `taint` is a required argument rather than a defaulted one, mirroring the
/// contract: a driver that could default provenance would be able to launder
/// externally-sourced content into internal-trust content, which is the one
/// failure mode the host's policy guard exists to prevent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Store {
    /// The `namespace` argument — wire position 0.
    pub namespace: String,
    /// The `key` argument — wire position 1.
    pub key: String,
    /// The `content` argument — wire position 2.
    pub content: String,
    /// The `category` argument — wire position 3.
    pub category: types::MemoryCategory,
    /// The `session_id` argument — wire position 4.
    pub session_id: Option<String>,
    /// The `taint` argument — wire position 5.
    pub taint: types::MemoryTaint,
}

impl BusCall for Store {
    const METHOD: &'static str = methods::STORE;

    type Response = ();

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.namespace, self.key, self.content, self.category, self.session_id, self.taint)).map_err(Error::Encode)
    }
}

/// Arguments for `Get`.
///
/// Fetch the entry at an exact `(namespace, key)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Get {
    /// The `namespace` argument — wire position 0.
    pub namespace: String,
    /// The `key` argument — wire position 1.
    pub key: String,
}

impl BusCall for Get {
    const METHOD: &'static str = methods::GET;

    type Response = Option<types::MemoryEntry>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.namespace, self.key)).map_err(Error::Encode)
    }
}

/// Arguments for `Forget`.
///
/// Delete the entry at `(namespace, key)`, reporting whether it existed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Forget {
    /// The `namespace` argument — wire position 0.
    pub namespace: String,
    /// The `key` argument — wire position 1.
    pub key: String,
}

impl BusCall for Forget {
    const METHOD: &'static str = methods::FORGET;

    type Response = bool;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.namespace, self.key)).map_err(Error::Encode)
    }
}

/// Arguments for `List`.
///
/// List entries, narrowing by namespace, category and session.
///
/// Bounded by `MAX_RESPONSE_BYTES`: unlike `Recall` and `ExportPage`, this
/// method takes no limit and no cursor, so the caller has no way to ask for
/// less. See `ensure_response_fits` for why the answer is a named refusal
/// rather than a truncation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct List {
    /// The `namespace` argument — wire position 0.
    pub namespace: Option<String>,
    /// The `category` argument — wire position 1.
    pub category: Option<types::MemoryCategory>,
    /// The `session_id` argument — wire position 2.
    pub session_id: Option<String>,
}

impl BusCall for List {
    const METHOD: &'static str = methods::LIST;

    type Response = Vec<types::MemoryEntry>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.namespace, self.category, self.session_id)).map_err(Error::Encode)
    }
}

/// Arguments for `Namespaces`.
///
/// Enumerate namespaces with their aggregate counts.
///
/// Takes no arguments, so it encodes as an empty positional array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Namespaces;

impl BusCall for Namespaces {
    const METHOD: &'static str = methods::NAMESPACES;

    type Response = Vec<types::NamespaceSummary>;

    fn into_args(self) -> crate::Result<Value> {
        Ok(Value::Array(Vec::new()))
    }
}
