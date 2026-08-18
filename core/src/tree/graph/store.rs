//! `Config` adapters for tinycortex-owned persisted graph edges.

use anyhow::Result;
use rusqlite::Transaction;

use crate::engine::engine_config;
use crate::Config;

pub use crate::engine::backend::graph::pairs_from_entities;

pub fn upsert_edges_tx(
    transaction: &Transaction<'_>,
    pairs: &[(String, String)],
    timestamp_ms: i64,
) -> Result<usize> {
    crate::engine::backend::graph::upsert_edges_tx(transaction, pairs, timestamp_ms)
}

pub fn upsert_edges(
    config: &Config,
    pairs: &[(String, String)],
    timestamp_ms: i64,
) -> Result<usize> {
    crate::engine::backend::graph::upsert_edges(&engine_config(config), pairs, timestamp_ms)
}

pub fn neighbors(config: &Config, entity_id: &str) -> Result<Vec<(String, i64)>> {
    crate::engine::backend::graph::edge_neighbors(&engine_config(config), entity_id)
}

pub fn clear_edges_for_entities_tx(
    transaction: &Transaction<'_>,
    entity_ids: &[String],
) -> Result<usize> {
    crate::engine::backend::graph::clear_edges_for_entities_tx(transaction, entity_ids)
}

pub fn count_edges(config: &Config) -> Result<u64> {
    crate::engine::backend::graph::count_edges(&engine_config(config))
}
