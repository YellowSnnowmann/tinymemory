//! What a connection remembers between sync runs: a cursor, a dedup set, and
//! a daily request budget.
//!
//! One [`SyncState`] per `(toolkit, connection)` pair, persisted as JSON under
//! [`STATE_NAMESPACE`] in whatever key/value store the driver provides. It is
//! the reason a second Gmail sync does not re-ingest the first sync's messages
//! and the reason a runaway pipeline stops at five hundred requests instead of
//! exhausting a user's quota.
//!
//! # Why this is contract vocabulary rather than engine state
//!
//! Both sides read it. The module advances the cursor and records requests; the
//! host shows "synced 4 minutes ago, 312 of 500 requests used today" and, on
//! disconnect, walks the dedup set to decide what to forget. A structural twin
//! on the host side would decode today and diverge the first time a field was
//! added — and this shape is *persisted*, so a divergence is not a wire bug
//! that reconnects away, it is a stranded cursor and a re-ingested inbox.
//!
//! # What is not here
//!
//! `SyncStateStore`, and the `load` / `save` that use it. This crate publishes
//! no traits and holds no I/O (see [`crate`]); the engine crate carries the
//! trait and offers the two methods as an extension trait over the type defined
//! here. Everything below is arithmetic on a struct.
//!
//! # Durability
//!
//! [`STATE_NAMESPACE`] and the serde field names are a compatibility surface:
//! changing the namespace strands every cursor, and renaming a field silently
//! resets it to its default on the next load. The engine keeps a
//! structurally-identical copy for its own internal pipelines, persisting under
//! the same namespace with the same shape, until those pipelines retire — the
//! pin tests below are what hold the two together.

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use serde::{Deserialize, Serialize};

/// The key/value namespace every persisted sync cursor lives under.
///
/// An alias for [`STATE_NAMESPACE`], kept because callers reach for one name or
/// the other depending on whether they are writing state or cleaning it up.
/// Durable: changing it strands every cursor.
pub const KV_NAMESPACE: &str = STATE_NAMESPACE;

/// Requests one connection may spend in a day before its budget is exhausted.
///
/// A backstop against a paginating pipeline that never terminates, not a
/// billing limit — the provider-reported cost is tallied separately.
pub const DEFAULT_DAILY_REQUEST_LIMIT: u32 = 500;

/// The key/value namespace every persisted sync cursor lives under.
///
/// Durable: changing it strands every cursor.
pub const STATE_NAMESPACE: &str = "composio-sync-state";

/// A per-connection, per-day request allowance.
///
/// `date` is the UTC day the counter belongs to. Every accessor compares it
/// against today and treats a stale date as a fresh allowance, so a state
/// loaded from yesterday reports a full budget without anyone having to reset
/// it — which is what makes a missed midnight rollover a non-event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyBudget {
    /// The UTC day this counter belongs to, as `YYYY-MM-DD`.
    pub date: String,
    /// Requests spent on [`date`](Self::date).
    pub requests_used: u32,
    /// Allowance for one day; defaults to [`DEFAULT_DAILY_REQUEST_LIMIT`].
    pub limit: u32,
}

impl Default for DailyBudget {
    fn default() -> Self {
        Self {
            date: today(),
            requests_used: 0,
            limit: DEFAULT_DAILY_REQUEST_LIMIT,
        }
    }
}

impl DailyBudget {
    /// Requests still available today.
    ///
    /// A counter from an earlier day reports the full limit rather than its
    /// stale remainder: the rollover happens on read, so nothing has to run at
    /// midnight for a budget to refresh.
    pub fn remaining(&self) -> u32 {
        if self.date != today() {
            self.limit
        } else {
            self.limit.saturating_sub(self.requests_used)
        }
    }

    /// Whether today's allowance is spent.
    pub fn is_exhausted(&self) -> bool {
        self.remaining() == 0
    }

    /// Charge `count` requests against today's allowance, rolling the counter
    /// over first if it belongs to an earlier day.
    pub fn record_requests(&mut self, count: u32) {
        self.roll_over_if_stale();
        self.requests_used = self.requests_used.saturating_add(count);
    }

    /// Reset the counter when it belongs to an earlier day.
    ///
    /// Every accessor already rolls over lazily, so this exists for the one
    /// caller that wants the *stored* value normalised rather than the answer:
    /// a state just loaded from yesterday, so that what is written back is
    /// today's row and not a stale one that later reads have to keep
    /// compensating for.
    pub fn roll_over_if_stale(&mut self) {
        let today = today();
        if self.date != today {
            self.date = today;
            self.requests_used = 0;
        }
    }

    /// Charge a single request. Shorthand for [`record_requests`](Self::record_requests).
    pub fn record_request(&mut self) {
        self.record_requests(1);
    }
}

/// Everything one `(toolkit, connection)` pair carries between sync runs.
///
/// The two `#[serde(skip)]` fields are per-run counters rather than state: they
/// exist so a finished run can report what it spent, and persisting them would
/// make the tally cumulative, which is not what any caller reads it as.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncState {
    /// Composio toolkit slug, e.g. `"gmail"`.
    pub toolkit: String,
    /// The connection this state belongs to.
    pub connection_id: String,
    /// Provider-native pagination cursor, when the provider issues one.
    #[serde(default)]
    pub cursor: Option<String>,
    /// Upstream item ids already ingested, so a re-run does not duplicate them.
    #[serde(default)]
    pub synced_ids: HashSet<String>,
    /// Item id to version string, for providers whose items can be edited after
    /// they were first seen.
    #[serde(default)]
    pub item_versions: HashMap<String, String>,
    /// Today's request allowance.
    #[serde(default)]
    pub daily_budget: DailyBudget,
    /// Newest item id seen, for providers that page newest-first.
    #[serde(default)]
    pub last_seen_id: Option<String>,
    /// When the last run finished, epoch milliseconds.
    #[serde(default)]
    pub last_sync_at_ms: Option<u64>,
    /// Requests spent by the *current* run. Not persisted.
    #[serde(skip)]
    pub run_requests: u32,
    /// Provider-reported cost accumulated by the *current* run. Not persisted.
    #[serde(skip)]
    pub run_provider_cost_usd: f64,
}

impl SyncState {
    /// A fresh state for a connection that has never synced.
    pub fn new(toolkit: impl Into<String>, connection_id: impl Into<String>) -> Self {
        Self {
            toolkit: toolkit.into(),
            connection_id: connection_id.into(),
            cursor: None,
            synced_ids: HashSet::new(),
            item_versions: HashMap::new(),
            daily_budget: DailyBudget::default(),
            last_seen_id: None,
            last_sync_at_ms: None,
            run_requests: 0,
            run_provider_cost_usd: 0.0,
        }
    }

    /// The key/value key a state is stored under, within [`STATE_NAMESPACE`].
    ///
    /// Durable, like the namespace: a different separator strands every cursor.
    pub fn key(toolkit: &str, connection_id: &str) -> String {
        format!("{toolkit}:{connection_id}")
    }

    /// Whether this item has already been ingested.
    pub fn is_synced(&self, id: &str) -> bool {
        self.synced_ids.contains(id)
    }

    /// Record an item as ingested.
    pub fn mark_synced(&mut self, id: impl Into<String>) {
        self.synced_ids.insert(id.into());
    }

    /// Move the pagination cursor forward.
    pub fn advance_cursor(&mut self, cursor: impl Into<String>) {
        self.cursor = Some(cursor.into());
    }

    /// Record the newest item id this run saw.
    pub fn set_last_seen_id(&mut self, id: impl Into<String>) {
        self.last_seen_id = Some(id.into());
    }

    /// Stamp when the run finished, epoch milliseconds.
    pub fn set_last_sync_at_ms(&mut self, timestamp_ms: u64) {
        self.last_sync_at_ms = Some(timestamp_ms);
    }

    /// Whether today's request allowance is spent.
    pub fn budget_exhausted(&self) -> bool {
        self.daily_budget.is_exhausted()
    }

    /// Requests still available today.
    pub fn budget_remaining(&self) -> u32 {
        self.daily_budget.remaining()
    }

    /// Charge `count` requests against both the daily allowance and this run's
    /// counter.
    pub fn record_requests(&mut self, count: u32) {
        self.daily_budget.record_requests(count);
        self.run_requests = self.run_requests.saturating_add(count);
    }

    /// Record one completed Composio action: its request attempts and the
    /// provider-reported cost.
    ///
    /// `attempts` is floored at one — an action that reached the provider spent
    /// at least one request however the caller counted its retries. A cost that
    /// is negative, infinite or `NaN` is discarded rather than propagated into
    /// a total someone reads as money.
    pub fn record_action(&mut self, attempts: u32, cost_usd: f64) {
        self.record_requests(attempts.max(1));
        if cost_usd.is_finite() && cost_usd > 0.0 {
            self.run_provider_cost_usd += cost_usd;
        }
    }
}

/// First non-empty string found at any of `paths` — dot-separated — in `item`.
///
/// Providers disagree about where an item's stable id lives (`id`, `messageId`,
/// `data.id`, …), so a pipeline hands this the candidates in priority order and
/// takes the first that is actually populated. Whitespace-only values count as
/// absent: an id of `" "` dedupes nothing and would poison the synced set.
pub fn extract_item_id(item: &serde_json::Value, paths: &[&str]) -> Option<String> {
    paths.iter().find_map(|path| {
        let value = path
            .split('.')
            .try_fold(item, |current, segment| current.get(segment))?;
        value
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

fn today() -> String {
    Utc::now().format("%Y-%m-%d").to_string()
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
