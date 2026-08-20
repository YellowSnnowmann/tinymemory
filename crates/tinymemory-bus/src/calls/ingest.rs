//! Document and chat ingestion through the summary pipeline.
//!
//! One [`BusCall`] per member; see [`crate::calls`] for how they are used.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::calls::BusCall;
use crate::error::Error;
use crate::names::methods;
use crate::types;

/// Arguments for `IngestDocument`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestDocument {
    /// The `item` argument — wire position 0.
    pub item: types::IngestItem,
}

impl BusCall for IngestDocument {
    const METHOD: &'static str = methods::INGEST_DOCUMENT;

    type Response = types::IngestOutcome;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.item,)).map_err(Error::Encode)
    }
}

/// Arguments for `IngestChat`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestChat {
    /// The `messages` argument — wire position 0.
    pub messages: Vec<types::IngestItem>,
}

impl BusCall for IngestChat {
    const METHOD: &'static str = methods::INGEST_CHAT;

    type Response = types::IngestOutcome;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.messages,)).map_err(Error::Encode)
    }
}
