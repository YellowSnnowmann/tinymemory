//! The long-term goals document.
//!
//! One [`BusCall`] per member; see [`crate::calls`] for how they are used.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use tinymemory_api::goals::GoalsDoc;

use crate::calls::BusCall;
use crate::error::Error;
use crate::names::methods;

/// Arguments for `Goals`.
///
/// Takes no arguments, so it encodes as an empty positional array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goals;

impl BusCall for Goals {
    const METHOD: &'static str = methods::GOALS;

    type Response = GoalsDoc;

    fn into_args(self) -> crate::Result<Value> {
        Ok(Value::Array(Vec::new()))
    }
}

/// Arguments for `SetGoals`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetGoals {
    /// The `goals` argument — wire position 0.
    pub goals: GoalsDoc,
}

impl BusCall for SetGoals {
    const METHOD: &'static str = methods::SET_GOALS;

    type Response = ();

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.goals,)).map_err(Error::Encode)
    }
}
