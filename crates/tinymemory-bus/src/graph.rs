//! Domain types for the **graph view**: a bounded, renderable slice of the
//! relation graph.
//!
//! The traits that produce these types live in `tinymemory-api`, which this
//! crate sits underneath and therefore cannot name — the references to
//! `MemoryGraph` and `MemoryTree` below are deliberately unlinked for that
//! reason, not by oversight.
//!
//! `MemoryGraph::relations` answers "which edges match this filter" and returns
//! a flat list. That is the right shape for a query and the wrong shape for a
//! *view*: a caller that wants to draw a graph, or hand one to an agent, needs
//! the node set as well as the edge set, needs to know how far each node sits
//! from where it started, and needs the answer bounded so an over-connected hub
//! cannot return the whole store.
//!
//! This module is the graph counterpart of [`crate::tree`], and
//! `MemoryGraph::graph_view` is the counterpart of `MemoryTree::drill_down`:
//! one call returns a node together with its surroundings, already assembled,
//! so navigation is a sequence of view calls rather than a client-side join.
//!
//! ## What is a driver concern and what is not
//!
//! Traversal *strategy* is a driver concern — an engine with a native
//! multi-hop traversal should use it. Traversal *bounds* are not: they are on
//! [`GraphViewQuery`], because the caller is the only party that knows how big
//! an answer it can render. A driver must honour them and must set
//! [`GraphView::truncated`] when it drops anything.

use serde::{Deserialize, Serialize};

use crate::types::GraphRelationRecord;

/// Which direction a traversal follows out of a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphDirection {
    /// Follow edges where the node is the subject. Wire string `"out"`.
    #[default]
    Out,
    /// Follow edges where the node is the object. Wire string `"in"`.
    In,
    /// Follow edges in both directions. Wire string `"both"`.
    Both,
}

impl GraphDirection {
    /// Whether outbound edges are followed.
    pub fn follows_out(self) -> bool {
        matches!(self, Self::Out | Self::Both)
    }

    /// Whether inbound edges are followed.
    pub fn follows_in(self) -> bool {
        matches!(self, Self::In | Self::Both)
    }
}

/// What a node in a view stands for.
///
/// The default traversal cannot infer this — it only ever sees edge endpoint
/// strings — so it reports [`GraphNodeKind::Unknown`]. A driver whose store
/// knows the answer should populate it, because a renderer that has to guess
/// from the id guesses differently from every other renderer.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphNodeKind {
    /// Kind not reported by the driver. Wire string `"unknown"`.
    #[default]
    Unknown,
    /// An extracted entity — a person, place, organisation, concept. Wire
    /// string `"entity"`.
    Entity,
    /// A whole stored document. Wire string `"document"`.
    Document,
    /// A single chunk of a document. Wire string `"chunk"`.
    Chunk,
    /// A summary-tree node. Wire string `"tree_node"`.
    TreeNode,
    /// A key/value record. Wire string `"kv"`.
    Kv,
    /// A driver-specific kind, carried verbatim.
    Other(String),
}

/// One node in a rendered [`GraphView`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    /// Stable node identifier; matches the `subject`/`object` strings on
    /// [`GraphEdge`].
    pub id: String,
    /// Human-readable label. Falls back to [`Self::id`] when the driver has
    /// nothing better.
    pub label: String,
    /// What the node stands for, when the driver knows.
    #[serde(default)]
    pub kind: GraphNodeKind,
    /// Hops from the nearest seed. Seeds are `0`.
    pub depth: u32,
    /// Edges incident to this node **within this view**. Deliberately not the
    /// node's degree in the whole store: a bounded view cannot see that, and
    /// reporting a number that changes with the bounds would be worse than
    /// reporting a number that is honestly local.
    pub degree: u32,
    /// Arbitrary structured attributes attached to the node.
    #[serde(default)]
    pub attrs: serde_json::Value,
}

impl GraphNode {
    /// A node with no attributes, no known kind, and its id as its label.
    pub fn bare(id: impl Into<String>, depth: u32) -> Self {
        let id = id.into();
        Self {
            label: id.clone(),
            id,
            kind: GraphNodeKind::Unknown,
            depth,
            degree: 0,
            attrs: serde_json::Value::Null,
        }
    }
}

/// One edge in a rendered [`GraphView`].
///
/// A projection of [`GraphRelationRecord`] rather than the record itself: a
/// view repeats the namespace once on the [`GraphView`] instead of once per
/// edge, and carries a derived [`Self::weight`] a renderer can size a line by.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    /// Edge subject (head node id).
    pub subject: String,
    /// Relation type linking subject to object.
    pub predicate: String,
    /// Edge object (tail node id).
    pub object: String,
    /// Number of independent observations supporting this edge.
    pub evidence_count: u32,
    /// Relative confidence in `0.0..=1.0`, derived from
    /// [`Self::evidence_count`] by [`edge_weight`].
    pub weight: f64,
    /// Last-update time as a Unix timestamp (seconds).
    pub updated_at: f64,
    /// Documents that contributed evidence for this edge.
    #[serde(default)]
    pub document_ids: Vec<String>,
    /// Chunks that contributed evidence for this edge.
    #[serde(default)]
    pub chunk_ids: Vec<String>,
    /// Arbitrary structured attributes attached to the edge.
    #[serde(default)]
    pub attrs: serde_json::Value,
}

impl From<GraphRelationRecord> for GraphEdge {
    fn from(record: GraphRelationRecord) -> Self {
        Self {
            weight: edge_weight(record.evidence_count),
            subject: record.subject,
            predicate: record.predicate,
            object: record.object,
            evidence_count: record.evidence_count,
            updated_at: record.updated_at,
            document_ids: record.document_ids,
            chunk_ids: record.chunk_ids,
            attrs: record.attrs,
        }
    }
}

impl GraphEdge {
    /// The `(subject, predicate, object)` triple that identifies this edge.
    ///
    /// The same key `MemoryGraph::put_relation` upserts by,
    /// so deduplicating a view by it cannot merge two edges the store holds
    /// separately.
    pub fn key(&self) -> (&str, &str, &str) {
        (&self.subject, &self.predicate, &self.object)
    }
}

/// Map an observation count onto a `0.0..=1.0` weight.
///
/// Saturating rather than linear: the difference between one observation and
/// five is worth more than the difference between fifty and fifty-four, and a
/// linear scale would make every edge in a well-observed graph look identical.
/// An edge with no evidence at all still gets a non-zero weight, because it is
/// in the store and a renderer that drew it at zero width would hide it.
pub fn edge_weight(evidence_count: u32) -> f64 {
    let n = f64::from(evidence_count);
    (n / (n + 3.0)).mul_add(0.9, 0.1)
}

/// Counters describing what a traversal actually did.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GraphViewStats {
    /// Nodes in [`GraphView::nodes`].
    pub node_count: usize,
    /// Edges in [`GraphView::edges`].
    pub edge_count: usize,
    /// Greatest [`GraphNode::depth`] present, or `0` for an empty view.
    pub max_depth: u32,
    /// Distinct nodes that were reached but never expanded — either because
    /// they sit one hop past [`GraphViewQuery::depth`] or because a bound was
    /// hit.
    ///
    /// Non-zero does **not** imply [`GraphView::truncated`]: a traversal that
    /// stops exactly where it was told to stop is complete, not truncated.
    /// Read this as "the graph continues here" and `truncated` as "we could
    /// not fit what you asked for".
    pub frontier_remaining: usize,
}

/// What a `MemoryGraph::graph_view` call asks for.
///
/// Every bound has a default, so the cheapest useful call is
/// `GraphViewQuery::around("ada")` — the one-hop neighbourhood, capped.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphViewQuery {
    /// Namespace to read, or `None` for the global, namespace-less slice.
    #[serde(default)]
    pub namespace: Option<String>,
    /// Node ids to start from. Empty means "no particular starting point":
    /// the driver returns a representative slice of the namespace instead of
    /// traversing, which is what an overview screen wants.
    #[serde(default)]
    pub seeds: Vec<String>,
    /// How many hops to expand out of the seeds. `0` returns the seeds and the
    /// edges directly between them.
    #[serde(default = "default_depth")]
    pub depth: u32,
    /// Hard ceiling on [`GraphView::nodes`].
    #[serde(default = "default_max_nodes")]
    pub max_nodes: usize,
    /// Hard ceiling on [`GraphView::edges`].
    #[serde(default = "default_max_edges")]
    pub max_edges: usize,
    /// Restrict to these relation types. Empty means every predicate.
    #[serde(default)]
    pub predicates: Vec<String>,
    /// Which way to follow edges out of a node.
    #[serde(default)]
    pub direction: GraphDirection,
}

fn default_depth() -> u32 {
    1
}

fn default_max_nodes() -> usize {
    256
}

fn default_max_edges() -> usize {
    512
}

impl Default for GraphViewQuery {
    fn default() -> Self {
        Self {
            namespace: None,
            seeds: Vec::new(),
            depth: default_depth(),
            max_nodes: default_max_nodes(),
            max_edges: default_max_edges(),
            predicates: Vec::new(),
            direction: GraphDirection::default(),
        }
    }
}

impl GraphViewQuery {
    /// The one-hop neighbourhood of a single node, with default bounds.
    pub fn around(seed: impl Into<String>) -> Self {
        Self {
            seeds: vec![seed.into()],
            ..Self::default()
        }
    }

    /// An unseeded overview of one namespace, with default bounds.
    pub fn overview(namespace: impl Into<String>) -> Self {
        Self {
            namespace: Some(namespace.into()),
            ..Self::default()
        }
    }

    /// Scope this query to `namespace`.
    #[must_use]
    pub fn in_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    /// Expand `depth` hops out of the seeds.
    #[must_use]
    pub fn with_depth(mut self, depth: u32) -> Self {
        self.depth = depth;
        self
    }

    /// Follow edges in `direction`.
    #[must_use]
    pub fn with_direction(mut self, direction: GraphDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Restrict the traversal to these relation types.
    #[must_use]
    pub fn with_predicates(mut self, predicates: Vec<String>) -> Self {
        self.predicates = predicates;
        self
    }

    /// Cap the view at `max_nodes` nodes and `max_edges` edges.
    #[must_use]
    pub fn with_bounds(mut self, max_nodes: usize, max_edges: usize) -> Self {
        self.max_nodes = max_nodes;
        self.max_edges = max_edges;
        self
    }

    /// Whether `predicate` passes this query's predicate filter.
    pub fn accepts_predicate(&self, predicate: &str) -> bool {
        self.predicates.is_empty() || self.predicates.iter().any(|p| p == predicate)
    }
}

/// A bounded, self-contained slice of the relation graph.
///
/// Self-contained in the sense that matters to a renderer: every id named by
/// an edge in [`Self::edges`] is present in [`Self::nodes`]. A driver that
/// cannot honour that must drop the edge rather than emit a dangling one.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GraphView {
    /// Namespace the view was read from, or `None` for the global slice.
    #[serde(default)]
    pub namespace: Option<String>,
    /// The seeds the traversal started from, echoed back verbatim.
    ///
    /// Echoed rather than filtered to the ones that exist: "this id has no
    /// edges" and "this id is not in the store" are different facts, and a
    /// traversal over an edge list cannot tell them apart. A seed that is
    /// absent from the store still appears in [`Self::nodes`] with a degree of
    /// zero, so a renderer draws the question the caller asked.
    #[serde(default)]
    pub seeds: Vec<String>,
    /// Every node reachable within the query's bounds.
    pub nodes: Vec<GraphNode>,
    /// Every edge between two nodes in [`Self::nodes`].
    pub edges: Vec<GraphEdge>,
    /// True when a bound was hit and the store holds more than is shown.
    ///
    /// Load-bearing: without it an empty-looking neighbourhood is
    /// indistinguishable from a truncated one, and a caller would stop paging.
    #[serde(default)]
    pub truncated: bool,
    /// Counters describing what the traversal did.
    #[serde(default)]
    pub stats: GraphViewStats,
}

impl GraphView {
    /// An empty view of one namespace.
    pub fn empty(namespace: Option<String>) -> Self {
        Self {
            namespace,
            ..Self::default()
        }
    }

    /// Recompute [`Self::stats`] and every [`GraphNode::degree`] from the
    /// current node and edge sets.
    ///
    /// Call this after assembling a view by hand; the default traversal already
    /// does.
    pub fn recompute_stats(&mut self) {
        for node in &mut self.nodes {
            node.degree = 0;
        }
        for edge in &self.edges {
            for node in &mut self.nodes {
                if node.id == edge.subject || node.id == edge.object {
                    node.degree = node.degree.saturating_add(1);
                }
            }
        }
        self.stats.node_count = self.nodes.len();
        self.stats.edge_count = self.edges.len();
        self.stats.max_depth = self.nodes.iter().map(|n| n.depth).max().unwrap_or(0);
    }

    /// Drop every edge whose endpoints are not both in [`Self::nodes`].
    ///
    /// The invariant this type promises, enforced. Returns how many edges were
    /// dropped so a caller can decide whether that counts as truncation.
    pub fn prune_dangling_edges(&mut self) -> usize {
        let before = self.edges.len();
        let ids: std::collections::HashSet<&str> =
            self.nodes.iter().map(|n| n.id.as_str()).collect();
        let keep: Vec<bool> = self
            .edges
            .iter()
            .map(|e| ids.contains(e.subject.as_str()) && ids.contains(e.object.as_str()))
            .collect();
        let mut index = 0;
        self.edges.retain(|_| {
            let keep = keep[index];
            index += 1;
            keep
        });
        before - self.edges.len()
    }
}

#[cfg(test)]
#[path = "graph_tests.rs"]
mod tests;
