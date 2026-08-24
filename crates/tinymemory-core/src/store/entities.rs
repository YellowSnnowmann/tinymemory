//! Host adapters for tinycortex's entity occurrence index.

use std::sync::Arc;

use crate::engine::backend::store::entity_index::{
    CanonicalEntity, EntityIndex, EntityKind, SelfIdentity,
};
use anyhow::Result;

use crate::engine::memory_config_from;
use crate::sync::composio::providers::profile::{is_self_identity_any_toolkit, IdentityKind};
use crate::Config;

/// Aggregate entity-index row for capability providers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopEntity {
    /// Canonical entity id.
    pub id: String,
    /// Stable entity kind string.
    pub kind: String,
    /// Representative observed surface form.
    pub name: String,
    /// Number of indexed observations.
    pub mentions: u32,
}

/// Entity row scoped to one memory-tree namespace.
pub fn namespace_entities(
    config: &Config,
    namespace: &str,
    query: Option<&str>,
    limit: usize,
) -> Result<Vec<TopEntity>> {
    let memory = memory_config_from(config, config.workspace_dir().clone());
    let connection = crate::engine::backend::chunks::shared_connection(&memory)?;
    let guard = connection.lock();
    let pattern = query.map(|value| format!("%{}%", value.to_ascii_lowercase()));
    let mut statement = guard.prepare(
        "SELECT entity_id, entity_kind, MAX(surface), COUNT(*)
           FROM mem_tree_entity_index
          WHERE tree_id = ?1
            AND (?2 IS NULL OR LOWER(entity_id) LIKE ?2 OR LOWER(surface) LIKE ?2)
          GROUP BY entity_id, entity_kind
          ORDER BY COUNT(*) DESC, MAX(timestamp_ms) DESC
          LIMIT ?3",
    )?;
    let rows = statement
        .query_map(
            rusqlite::params![namespace, pattern, i64::try_from(limit).unwrap_or(i64::MAX)],
            |row| {
                let mentions: i64 = row.get(3)?;
                Ok(TopEntity {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    name: row.get(2)?,
                    mentions: u32::try_from(mentions.max(0)).unwrap_or(u32::MAX),
                })
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Co-occurrence edges scoped to one memory-tree namespace.
pub fn namespace_entity_edges(
    config: &Config,
    namespace: &str,
    entity_id: &str,
    limit: usize,
) -> Result<Vec<(String, u32)>> {
    let memory = memory_config_from(config, config.workspace_dir().clone());
    let connection = crate::engine::backend::chunks::shared_connection(&memory)?;
    let guard = connection.lock();
    let mut statement = guard.prepare(
        "SELECT b.entity_id, COUNT(*)
           FROM mem_tree_entity_index a
           JOIN mem_tree_entity_index b
             ON a.node_id = b.node_id AND a.tree_id = b.tree_id
          WHERE a.tree_id = ?1 AND a.entity_id = ?2 AND b.entity_id <> a.entity_id
          GROUP BY b.entity_id
          ORDER BY COUNT(*) DESC
          LIMIT ?3",
    )?;
    let rows = statement
        .query_map(
            rusqlite::params![
                namespace,
                entity_id,
                i64::try_from(limit).unwrap_or(i64::MAX)
            ],
            |row| {
                let count: i64 = row.get(1)?;
                Ok((row.get(0)?, u32::try_from(count.max(0)).unwrap_or(u32::MAX)))
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub use crate::engine::backend::store::entity_index::EntityHit;

#[derive(Debug)]
struct HostSelfIdentity;

impl SelfIdentity for HostSelfIdentity {
    fn is_self(&self, kind: EntityKind, surface: &str) -> bool {
        let identity_kind = match kind {
            EntityKind::Email => IdentityKind::Email,
            EntityKind::Handle => IdentityKind::Handle,
            _ => return false,
        };
        is_self_identity_any_toolkit(identity_kind, surface)
    }
}

fn index(config: &Config) -> Result<EntityIndex> {
    let memory = memory_config_from(config, config.workspace_dir().clone());
    let connection = crate::engine::backend::chunks::shared_connection(&memory)?;
    EntityIndex::from_shared_connection(connection, Arc::new(HostSelfIdentity))
}

pub fn index_entity(
    config: &Config,
    entity: &CanonicalEntity,
    node_id: &str,
    node_kind: &str,
    timestamp_ms: i64,
    tree_id: Option<&str>,
) -> Result<()> {
    log::debug!("[memory:entities] index one node_kind={node_kind}");
    index(config)?.index_entity(entity, node_id, node_kind, timestamp_ms, tree_id)
}

pub fn index_entities(
    config: &Config,
    entities: &[CanonicalEntity],
    node_id: &str,
    node_kind: &str,
    timestamp_ms: i64,
    tree_id: Option<&str>,
) -> Result<usize> {
    log::debug!(
        "[memory:entities] index batch count={} node_kind={node_kind}",
        entities.len()
    );
    index(config)?.index_entities(entities, node_id, node_kind, timestamp_ms, tree_id)
}

pub fn clear_entity_index_for_node(config: &Config, node_id: &str) -> Result<usize> {
    index(config)?.clear_entity_index_for_node(node_id)
}

pub fn lookup_entity(
    config: &Config,
    entity_id: &str,
    limit: Option<usize>,
) -> Result<Vec<EntityHit>> {
    index(config)?.lookup_entity(entity_id, limit)
}

pub fn list_entity_ids_for_node(config: &Config, node_id: &str) -> Result<Vec<String>> {
    index(config)?.list_entity_ids_for_node(node_id)
}

pub fn count_entity_index(config: &Config) -> Result<u64> {
    index(config)?.count_entity_index()
}

/// One aggregated row of `mem_tree_entity_index`, named for the columns it
/// holds rather than for what a caller might render.
///
/// [`TopEntity`] carries the same four values under a `name` that is really a
/// `MAX(surface)` sample. That reading is fine where the caller only needs a
/// label, and wrong where it crosses the driver contract, which promises a
/// canonical name under `name`. This type keeps `surface` called `surface` so
/// the promise is not made by accident.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityIndexRow {
    /// Canonical entity id.
    pub entity_id: String,
    /// Stable entity kind string, as stored.
    pub entity_kind: String,
    /// An observed surface form — a sample of one row in the group, not a
    /// canonical name.
    pub surface: String,
    /// Number of index rows aggregated into this one.
    pub mentions: u32,
}

/// Store-wide entity rows, most-observed first, optionally one kind only.
///
/// Deliberately **not** tree-scoped: `namespace_entities` answers the scoped
/// question, and this one exists for the workspace-wide view where the caller
/// is asking what the store holds at all. Recency breaks ties, so two entities
/// seen the same number of times order by which was seen last.
///
/// `kind` is matched against the stored `entity_kind` verbatim; validating it
/// belongs to the caller, which knows the vocabulary it accepts.
pub fn top_entity_rows(
    config: &Config,
    kind: Option<&str>,
    limit: usize,
) -> Result<Vec<EntityIndexRow>> {
    let memory = memory_config_from(config, config.workspace_dir().clone());
    let connection = crate::engine::backend::chunks::shared_connection(&memory)?;
    let guard = connection.lock();
    // The `?1 IS NULL OR` form keeps one prepared statement for both the
    // filtered and unfiltered call, rather than concatenating SQL per call.
    let mut statement = guard.prepare(
        "SELECT entity_id, entity_kind, MAX(surface), COUNT(*)
           FROM mem_tree_entity_index
          WHERE (?1 IS NULL OR entity_kind = ?1)
          GROUP BY entity_id, entity_kind
          ORDER BY COUNT(*) DESC, MAX(timestamp_ms) DESC
          LIMIT ?2",
    )?;
    let rows = statement
        .query_map(
            rusqlite::params![kind, i64::try_from(limit).unwrap_or(i64::MAX)],
            index_row,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The entity rows recorded against one tree node, most-observed first.
///
/// Grouped by surface as well as by id: one entity seen under two forms is two
/// rows, because the form is the evidence of how this node's text named it.
///
/// The count is `1` for every row under the current schema — the primary key
/// is `(entity_id, node_id)`, so one node cannot hold two rows for the same
/// entity — and is reported anyway: it is the same aggregate
/// [`top_entity_rows`] returns, and an index that later keys occurrences per
/// span would make it meaningful without a shape change here.
pub fn node_entity_rows(config: &Config, node_id: &str) -> Result<Vec<EntityIndexRow>> {
    let memory = memory_config_from(config, config.workspace_dir().clone());
    let connection = crate::engine::backend::chunks::shared_connection(&memory)?;
    let guard = connection.lock();
    let mut statement = guard.prepare(
        "SELECT entity_id, entity_kind, surface, COUNT(*)
           FROM mem_tree_entity_index
          WHERE node_id = ?1
          GROUP BY entity_id, entity_kind, surface
          ORDER BY COUNT(*) DESC, entity_id ASC",
    )?;
    let rows = statement
        .query_map(rusqlite::params![node_id], index_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Ids of the **leaf** nodes one entity was observed in, newest first.
///
/// `node_kind = 'leaf'` is the filter the scorer's write path defines: it
/// stamps `leaf` for a scored chunk and `summary` for a summariser-curated
/// node. Summary nodes are excluded because their ids are not chunk ids — a
/// caller filtering a chunk list by them would match nothing.
///
/// `GROUP BY` rather than `SELECT DISTINCT` so the sort key is a selected
/// aggregate: the primary key already makes `node_id` unique per entity, but a
/// `DISTINCT` ordered by an unselected column is the kind of query that
/// depends on how permissive the engine happens to be.
pub fn entity_leaf_node_ids(config: &Config, entity_id: &str, limit: usize) -> Result<Vec<String>> {
    let memory = memory_config_from(config, config.workspace_dir().clone());
    let connection = crate::engine::backend::chunks::shared_connection(&memory)?;
    let guard = connection.lock();
    let mut statement = guard.prepare(
        "SELECT node_id, MAX(timestamp_ms) AS seen_at
           FROM mem_tree_entity_index
          WHERE entity_id = ?1 AND node_kind = 'leaf'
          GROUP BY node_id
          ORDER BY seen_at DESC
          LIMIT ?2",
    )?;
    let rows = statement
        .query_map(
            rusqlite::params![entity_id, i64::try_from(limit).unwrap_or(i64::MAX)],
            |row| row.get::<_, String>(0),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Read one aggregated row in the shape both grouped queries above select.
fn index_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EntityIndexRow> {
    let mentions: i64 = row.get(3)?;
    Ok(EntityIndexRow {
        entity_id: row.get(0)?,
        entity_kind: row.get(1)?,
        surface: row.get(2)?,
        mentions: u32::try_from(mentions.max(0)).unwrap_or(u32::MAX),
    })
}

/// Most frequently observed entities, with recency as the tie-breaker.
///
/// The [`TopEntity`] view of [`top_entity_rows`], kept for callers that were
/// written against it. New callers should take the rows: this shape puts a
/// `MAX(surface)` sample in a field called `name`, and the two are not the
/// same claim.
pub fn top_entities(config: &Config, limit: usize) -> Result<Vec<TopEntity>> {
    Ok(top_entity_rows(config, None, limit)?
        .into_iter()
        .map(|row| TopEntity {
            id: row.entity_id,
            kind: row.entity_kind,
            name: row.surface,
            mentions: row.mentions,
        })
        .collect())
}

#[cfg(test)]
#[path = "entities_tests.rs"]
mod tests;
