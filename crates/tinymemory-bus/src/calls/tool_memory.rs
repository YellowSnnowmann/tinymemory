//! Tool-scoped memory rules.
//!
//! One [`BusCall`] per member; see [`crate::calls`] for how they are used.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use tinymemory_api::tool_memory::ToolMemoryRule;

use crate::calls::BusCall;
use crate::error::Error;
use crate::names::methods;

/// Arguments for `ToolRules`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRules {
    /// The `tool_name` argument — wire position 0.
    pub tool_name: String,
}

impl BusCall for ToolRules {
    const METHOD: &'static str = methods::TOOL_RULES;

    type Response = Vec<ToolMemoryRule>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.tool_name,)).map_err(Error::Encode)
    }
}

/// Arguments for `PutToolRule`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutToolRule {
    /// The `rule` argument — wire position 0.
    pub rule: ToolMemoryRule,
}

impl BusCall for PutToolRule {
    const METHOD: &'static str = methods::PUT_TOOL_RULE;

    type Response = ();

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.rule,)).map_err(Error::Encode)
    }
}

/// Arguments for `DeleteToolRule`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteToolRule {
    /// The `tool_name` argument — wire position 0.
    pub tool_name: String,
    /// The `rule_id` argument — wire position 1.
    pub rule_id: String,
}

impl BusCall for DeleteToolRule {
    const METHOD: &'static str = methods::DELETE_TOOL_RULE;

    type Response = bool;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.tool_name, self.rule_id)).map_err(Error::Encode)
    }
}
