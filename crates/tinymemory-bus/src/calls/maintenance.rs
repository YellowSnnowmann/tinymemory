//! Re-embedding, compaction, consolidation and diagnosis.
//!
//! One [`BusCall`] per member; see [`crate::calls`] for how they are used.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use tinymemory_api::provider::types::MaintenanceReport;

use crate::calls::BusCall;
use crate::names::methods;

/// Arguments for `Reembed`.
///
/// Takes no arguments, so it encodes as an empty positional array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reembed;

impl BusCall for Reembed {
    const METHOD: &'static str = methods::REEMBED;

    type Response = MaintenanceReport;

    fn into_args(self) -> crate::Result<Value> {
        Ok(Value::Array(Vec::new()))
    }
}

/// Arguments for `Compact`.
///
/// Takes no arguments, so it encodes as an empty positional array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Compact;

impl BusCall for Compact {
    const METHOD: &'static str = methods::COMPACT;

    type Response = MaintenanceReport;

    fn into_args(self) -> crate::Result<Value> {
        Ok(Value::Array(Vec::new()))
    }
}

/// Arguments for `Consolidate`.
///
/// Takes no arguments, so it encodes as an empty positional array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Consolidate;

impl BusCall for Consolidate {
    const METHOD: &'static str = methods::CONSOLIDATE;

    type Response = MaintenanceReport;

    fn into_args(self) -> crate::Result<Value> {
        Ok(Value::Array(Vec::new()))
    }
}

/// Arguments for `Doctor`.
///
/// Takes no arguments, so it encodes as an empty positional array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Doctor;

impl BusCall for Doctor {
    const METHOD: &'static str = methods::DOCTOR;

    type Response = MaintenanceReport;

    fn into_args(self) -> crate::Result<Value> {
        Ok(Value::Array(Vec::new()))
    }
}
