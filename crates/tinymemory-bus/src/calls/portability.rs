//! Paged export and bulk import of raw records.
//!
//! One [`BusCall`] per member; see [`crate::calls`] for how they are used.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::calls::BusCall;
use crate::error::Error;
use crate::names::methods;
use crate::types;

/// Arguments for `ExportPage`.
///
/// Read one page of the export, continuing from `cursor`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportPage {
    /// The `cursor` argument — wire position 0.
    pub cursor: Option<String>,
    /// The `limit` argument — wire position 1.
    pub limit: usize,
}

impl BusCall for ExportPage {
    const METHOD: &'static str = methods::EXPORT_PAGE;

    type Response = types::ExportPage;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.cursor, self.limit)).map_err(Error::Encode)
    }
}

/// Arguments for `ImportRecords`.
///
/// Write a batch of previously-exported records.
///
/// Partial success is reported inside `ImportOutcome` rather than as an
/// error, so a million-record restore is not aborted by one bad record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportRecords {
    /// The `records` argument — wire position 0.
    pub records: Vec<types::ExportRecord>,
}

impl BusCall for ImportRecords {
    const METHOD: &'static str = methods::IMPORT_RECORDS;

    type Response = types::ImportOutcome;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.records,)).map_err(Error::Encode)
    }
}
