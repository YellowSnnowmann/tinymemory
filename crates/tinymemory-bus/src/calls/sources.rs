//! Source snapshots, diffs, item acceptance and forgetting.
//!
//! One [`BusCall`] per member; see [`crate::calls`] for how they are used.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::calls::BusCall;
use crate::error::Error;
use crate::names::methods;
use crate::types;

/// Arguments for `CaptureSnapshot`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureSnapshot {
    /// The `source_id` argument — wire position 0.
    pub source_id: String,
}

impl BusCall for CaptureSnapshot {
    const METHOD: &'static str = methods::CAPTURE_SNAPSHOT;

    type Response = types::SnapshotRef;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.source_id,)).map_err(Error::Encode)
    }
}

/// Arguments for `Snapshots`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshots {
    /// The `source_id` argument — wire position 0.
    pub source_id: String,
    /// The `limit` argument — wire position 1.
    pub limit: usize,
}

impl BusCall for Snapshots {
    const METHOD: &'static str = methods::SNAPSHOTS;

    type Response = Vec<types::SnapshotRef>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.source_id, self.limit)).map_err(Error::Encode)
    }
}

/// Arguments for `Diff`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diff {
    /// The `source_id` argument — wire position 0.
    pub source_id: String,
    /// The `from` argument — wire position 1.
    pub from: Option<String>,
    /// The `to` argument — wire position 2.
    pub to: String,
}

impl BusCall for Diff {
    const METHOD: &'static str = methods::DIFF;

    type Response = types::DiffReport;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.source_id, self.from, self.to)).map_err(Error::Encode)
    }
}

/// Arguments for `AcceptSourceItems`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceptSourceItems {
    /// The `source_id` argument — wire position 0.
    pub source_id: String,
    /// The `source_kind` argument — wire position 1.
    pub source_kind: String,
    /// The `items` argument — wire position 2.
    pub items: Vec<types::SourceItem>,
    /// The `taint` argument — wire position 3.
    pub taint: types::MemoryTaint,
}

impl BusCall for AcceptSourceItems {
    const METHOD: &'static str = methods::ACCEPT_SOURCE_ITEMS;

    type Response = types::IngestOutcome;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.source_id, self.source_kind, self.items, self.taint)).map_err(Error::Encode)
    }
}

/// Arguments for `ForgetSource`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgetSource {
    /// The `source_id` argument — wire position 0.
    pub source_id: String,
}

impl BusCall for ForgetSource {
    const METHOD: &'static str = methods::FORGET_SOURCE;

    type Response = u64;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.source_id,)).map_err(Error::Encode)
    }
}
