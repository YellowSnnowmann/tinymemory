//! What one sync run cost.

use serde::{Deserialize, Serialize};

/// Per-run accumulator for a source's billable provider calls.
///
/// # Why it outlived the Composio tree
///
/// It is what the sync audit log records, and the audit log is this crate's.
/// A run against a connected account has a price attached — the provider
/// charges per action — and an operator asking "why did this month cost that"
/// is asking a question about stored rows, not about whoever fetched them.
///
/// Zero for the sources that cost nothing to read, which is most of them.
/// That is not a gap: a folder scan really did call nothing and spend nothing,
/// and the field says so rather than being absent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct ProviderUsage {
    /// Calls that returned a response this run.
    pub actions_called: u32,
    /// Sum of each response's provider-reported cost.
    pub cost_usd: f64,
}
