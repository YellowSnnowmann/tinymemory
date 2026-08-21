//! Behavioural tests for the default [`MemoryGraph::graph_view`] traversal.
//!
//! Driven through a fixed in-memory edge list rather than a real engine: the
//! point under test is the traversal the contract provides for free, and a
//! store that answers `relations` from a `Vec` is the smallest thing that can
//! exercise it deterministically.

use async_trait::async_trait;
use tinymemory_api::error::MemoryError;
use tinymemory_api::graph::{GraphDirection, GraphViewQuery};
use tinymemory_api::provider::MemoryGraph;
use tinymemory_api::types::{GraphRelationRecord, MemoryKvRecord};

/// A `MemoryGraph` whose relation tier is a fixed edge list.
///
/// Only `relations` is real; the key/value half is out of scope for the
/// traversal and reports `Unsupported`, exactly as a driver without one would.
struct EdgeList {
    edges: Vec<GraphRelationRecord>,
}

impl EdgeList {
    fn new(edges: &[(&str, &str, &str)]) -> Self {
        Self {
            edges: edges
                .iter()
                .map(|(subject, predicate, object)| GraphRelationRecord {
                    namespace: None,
                    subject: (*subject).to_string(),
                    predicate: (*predicate).to_string(),
                    object: (*object).to_string(),
                    attrs: serde_json::Value::Null,
                    updated_at: 0.0,
                    evidence_count: 1,
                    order_index: None,
                    document_ids: Vec::new(),
                    chunk_ids: Vec::new(),
                })
                .collect(),
        }
    }

    /// A path `n0 -> n1 -> … -> n{len}`, for depth-bound tests.
    fn chain(len: usize) -> Self {
        let names: Vec<String> = (0..=len).map(|i| format!("n{i}")).collect();
        let pairs: Vec<(&str, &str, &str)> = (0..len)
            .map(|i| (names[i].as_str(), "next", names[i + 1].as_str()))
            .collect();
        Self::new(&pairs)
    }
}

#[async_trait]
impl MemoryGraph for EdgeList {
    async fn kv_get(
        &self,
        _namespace: Option<&str>,
        _key: &str,
    ) -> Result<Option<MemoryKvRecord>, MemoryError> {
        Err(MemoryError::unsupported(
            tinymemory_api::capabilities::Capability::Graph,
        ))
    }

    async fn kv_put(
        &self,
        _namespace: Option<&str>,
        _key: &str,
        _value: serde_json::Value,
    ) -> Result<(), MemoryError> {
        Err(MemoryError::unsupported(
            tinymemory_api::capabilities::Capability::Graph,
        ))
    }

    async fn kv_delete(&self, _namespace: Option<&str>, _key: &str) -> Result<bool, MemoryError> {
        Err(MemoryError::unsupported(
            tinymemory_api::capabilities::Capability::Graph,
        ))
    }

    async fn kv_list(
        &self,
        _namespace: Option<&str>,
        _prefix: Option<&str>,
        _limit: usize,
    ) -> Result<Vec<MemoryKvRecord>, MemoryError> {
        Err(MemoryError::unsupported(
            tinymemory_api::capabilities::Capability::Graph,
        ))
    }

    async fn relations(
        &self,
        _namespace: Option<&str>,
        subject: Option<&str>,
        predicate: Option<&str>,
        limit: usize,
    ) -> Result<Vec<GraphRelationRecord>, MemoryError> {
        Ok(self
            .edges
            .iter()
            .filter(|e| subject.is_none_or(|s| e.subject == s))
            .filter(|e| predicate.is_none_or(|p| e.predicate == p))
            .take(limit)
            .cloned()
            .collect())
    }

    async fn put_relation(&self, _relation: GraphRelationRecord) -> Result<(), MemoryError> {
        Err(MemoryError::unsupported(
            tinymemory_api::capabilities::Capability::Graph,
        ))
    }
}

/// A graph that refuses every relation query, standing in for a driver with no
/// graph family at all.
struct NoGraph;

#[async_trait]
impl MemoryGraph for NoGraph {
    async fn kv_get(
        &self,
        _namespace: Option<&str>,
        _key: &str,
    ) -> Result<Option<MemoryKvRecord>, MemoryError> {
        Err(MemoryError::unsupported(
            tinymemory_api::capabilities::Capability::Graph,
        ))
    }

    async fn kv_put(
        &self,
        _namespace: Option<&str>,
        _key: &str,
        _value: serde_json::Value,
    ) -> Result<(), MemoryError> {
        Err(MemoryError::unsupported(
            tinymemory_api::capabilities::Capability::Graph,
        ))
    }

    async fn kv_delete(&self, _namespace: Option<&str>, _key: &str) -> Result<bool, MemoryError> {
        Err(MemoryError::unsupported(
            tinymemory_api::capabilities::Capability::Graph,
        ))
    }

    async fn kv_list(
        &self,
        _namespace: Option<&str>,
        _prefix: Option<&str>,
        _limit: usize,
    ) -> Result<Vec<MemoryKvRecord>, MemoryError> {
        Err(MemoryError::unsupported(
            tinymemory_api::capabilities::Capability::Graph,
        ))
    }

    async fn relations(
        &self,
        _namespace: Option<&str>,
        _subject: Option<&str>,
        _predicate: Option<&str>,
        _limit: usize,
    ) -> Result<Vec<GraphRelationRecord>, MemoryError> {
        Err(MemoryError::unsupported(
            tinymemory_api::capabilities::Capability::Graph,
        ))
    }

    async fn put_relation(&self, _relation: GraphRelationRecord) -> Result<(), MemoryError> {
        Err(MemoryError::unsupported(
            tinymemory_api::capabilities::Capability::Graph,
        ))
    }
}

fn ids(view: &tinymemory_api::graph::GraphView) -> Vec<&str> {
    let mut ids: Vec<&str> = view.nodes.iter().map(|n| n.id.as_str()).collect();
    ids.sort_unstable();
    ids
}

#[tokio::test]
async fn a_one_hop_view_returns_the_seed_and_its_neighbours() {
    let store = EdgeList::new(&[
        ("ada", "works_with", "charles"),
        ("ada", "wrote", "notes"),
        ("charles", "designed", "engine"),
    ]);
    let view = store
        .graph_view(&GraphViewQuery::around("ada"))
        .await
        .unwrap();

    assert_eq!(ids(&view), vec!["ada", "charles", "notes"]);
    assert_eq!(view.edges.len(), 2);
    assert!(!view.truncated);
    assert_eq!(view.stats.max_depth, 1);
    assert_eq!(view.seeds, vec!["ada".to_string()]);
}

#[tokio::test]
async fn a_seed_with_no_edges_is_still_a_node() {
    let store = EdgeList::new(&[("charles", "designed", "engine")]);
    let view = store
        .graph_view(&GraphViewQuery::around("ada"))
        .await
        .unwrap();

    assert_eq!(ids(&view), vec!["ada"]);
    assert!(view.edges.is_empty());
    assert_eq!(view.nodes[0].degree, 0);
}

#[tokio::test]
async fn depth_zero_returns_only_edges_between_the_seeds() {
    let store = EdgeList::new(&[
        ("ada", "works_with", "charles"),
        ("ada", "wrote", "notes"),
        ("charles", "designed", "engine"),
    ]);
    let query = GraphViewQuery {
        seeds: vec!["ada".into(), "charles".into()],
        depth: 0,
        ..GraphViewQuery::default()
    };
    let view = store.graph_view(&query).await.unwrap();

    assert_eq!(ids(&view), vec!["ada", "charles"]);
    assert_eq!(view.edges.len(), 1);
    assert_eq!(view.edges[0].key(), ("ada", "works_with", "charles"));
    // `notes` and `engine` sit past the requested depth. That is the caller
    // getting what they asked for, so the view is not truncated — but it does
    // say the graph continues there.
    assert!(!view.truncated);
    assert_eq!(view.stats.frontier_remaining, 2);
}

#[tokio::test]
async fn the_outermost_hop_closes_edges_between_nodes_already_in_the_view() {
    // A triangle: at depth 1 from `ada` both `b` and `c` are in the view, and
    // the b -> c edge must be drawn even though it adds no node.
    let store = EdgeList::new(&[("ada", "e", "b"), ("ada", "e", "c"), ("b", "e", "c")]);
    let view = store
        .graph_view(&GraphViewQuery::around("ada"))
        .await
        .unwrap();

    assert_eq!(ids(&view), vec!["ada", "b", "c"]);
    assert_eq!(view.edges.len(), 3);
    assert!(view.edges.iter().any(|e| e.key() == ("b", "e", "c")));
}

#[tokio::test]
async fn depth_bounds_the_traversal() {
    let store = EdgeList::chain(5);
    for depth in 0..=4 {
        let view = store
            .graph_view(&GraphViewQuery::around("n0").with_depth(depth))
            .await
            .unwrap();
        assert_eq!(
            view.nodes.len(),
            depth as usize + 1,
            "depth {depth} should reach {} nodes",
            depth + 1
        );
        assert_eq!(view.stats.max_depth, depth);
    }
}

#[tokio::test]
async fn a_predicate_filter_excludes_other_relation_types() {
    let store = EdgeList::new(&[
        ("ada", "works_with", "charles"),
        ("ada", "wrote", "notes"),
        ("ada", "wrote", "letters"),
    ]);
    let query = GraphViewQuery::around("ada").with_predicates(vec!["wrote".into()]);
    let view = store.graph_view(&query).await.unwrap();

    assert_eq!(ids(&view), vec!["ada", "letters", "notes"]);
    assert!(view.edges.iter().all(|e| e.predicate == "wrote"));
}

#[tokio::test]
async fn several_predicates_are_unioned() {
    let store = EdgeList::new(&[
        ("ada", "works_with", "charles"),
        ("ada", "wrote", "notes"),
        ("ada", "read", "papers"),
    ]);
    let query = GraphViewQuery::around("ada").with_predicates(vec!["wrote".into(), "read".into()]);
    let view = store.graph_view(&query).await.unwrap();

    assert_eq!(ids(&view), vec!["ada", "notes", "papers"]);
    assert_eq!(view.edges.len(), 2);
}

#[tokio::test]
async fn inbound_expansion_follows_edges_the_seed_is_the_object_of() {
    let store = EdgeList::new(&[("charles", "cites", "ada"), ("ada", "cites", "babbage")]);
    let view = store
        .graph_view(&GraphViewQuery::around("ada").with_direction(GraphDirection::In))
        .await
        .unwrap();

    assert_eq!(ids(&view), vec!["ada", "charles"]);
    assert_eq!(view.edges[0].key(), ("charles", "cites", "ada"));
}

#[tokio::test]
async fn both_directions_reach_either_side() {
    let store = EdgeList::new(&[("charles", "cites", "ada"), ("ada", "cites", "babbage")]);
    let view = store
        .graph_view(&GraphViewQuery::around("ada").with_direction(GraphDirection::Both))
        .await
        .unwrap();

    assert_eq!(ids(&view), vec!["ada", "babbage", "charles"]);
    assert_eq!(view.edges.len(), 2);
    assert_eq!(view.nodes.iter().find(|n| n.id == "ada").unwrap().degree, 2);
}

#[tokio::test]
async fn a_cycle_terminates_and_visits_each_node_once() {
    let store = EdgeList::new(&[("a", "e", "b"), ("b", "e", "c"), ("c", "e", "a")]);
    let view = store
        .graph_view(&GraphViewQuery::around("a").with_depth(10))
        .await
        .unwrap();

    assert_eq!(ids(&view), vec!["a", "b", "c"]);
    assert_eq!(view.edges.len(), 3);
}

#[tokio::test]
async fn the_node_ceiling_truncates_rather_than_erroring() {
    let store = EdgeList::new(&[
        ("hub", "e", "a"),
        ("hub", "e", "b"),
        ("hub", "e", "c"),
        ("hub", "e", "d"),
    ]);
    let query = GraphViewQuery::around("hub").with_bounds(3, 512);
    let view = store.graph_view(&query).await.unwrap();

    assert_eq!(view.nodes.len(), 3);
    assert!(view.truncated);
    assert!(view.stats.frontier_remaining > 0);
}

#[tokio::test]
async fn the_edge_ceiling_truncates_rather_than_erroring() {
    let store = EdgeList::new(&[("hub", "e", "a"), ("hub", "e", "b"), ("hub", "e", "c")]);
    let query = GraphViewQuery::around("hub").with_bounds(256, 2);
    let view = store.graph_view(&query).await.unwrap();

    assert_eq!(view.edges.len(), 2);
    assert!(view.truncated);
}

#[tokio::test]
async fn every_edge_in_a_view_has_both_endpoints_in_its_node_set() {
    let store = EdgeList::new(&[
        ("hub", "e", "a"),
        ("hub", "e", "b"),
        ("hub", "e", "c"),
        ("a", "e", "deep"),
    ]);
    for bound in 1..=5 {
        let query = GraphViewQuery::around("hub")
            .with_depth(2)
            .with_bounds(bound, bound);
        let mut view = store.graph_view(&query).await.unwrap();
        assert_eq!(
            view.prune_dangling_edges(),
            0,
            "view bounded at {bound} emitted a dangling edge"
        );
    }
}

#[tokio::test]
async fn an_unseeded_query_returns_an_overview_of_the_slice() {
    let store = EdgeList::new(&[("ada", "works_with", "charles"), ("charles", "e", "engine")]);
    let view = store
        .graph_view(&GraphViewQuery::overview("learning:history"))
        .await
        .unwrap();

    assert_eq!(view.namespace.as_deref(), Some("learning:history"));
    assert!(view.seeds.is_empty());
    assert_eq!(ids(&view), vec!["ada", "charles", "engine"]);
    assert_eq!(view.edges.len(), 2);
    assert!(view.nodes.iter().all(|n| n.depth == 0));
}

#[tokio::test]
async fn an_unseeded_query_honours_its_predicate_filter() {
    let store = EdgeList::new(&[("ada", "works_with", "charles"), ("charles", "e", "engine")]);
    let query =
        GraphViewQuery::overview("learning:history").with_predicates(vec!["works_with".into()]);
    let view = store.graph_view(&query).await.unwrap();

    assert_eq!(ids(&view), vec!["ada", "charles"]);
    assert_eq!(view.edges.len(), 1);
}

#[tokio::test]
async fn a_driver_without_a_graph_family_reports_unsupported_not_an_empty_view() {
    let error = NoGraph
        .graph_view(&GraphViewQuery::around("ada"))
        .await
        .unwrap_err();
    assert!(
        matches!(error, MemoryError::Unsupported { .. }),
        "expected Unsupported, got {error:?}"
    );
}

#[tokio::test]
async fn a_view_is_reachable_through_a_trait_object() {
    let store: Box<dyn MemoryGraph> = Box::new(EdgeList::new(&[("ada", "e", "charles")]));
    let view = store
        .graph_view(&GraphViewQuery::around("ada"))
        .await
        .unwrap();
    assert_eq!(view.nodes.len(), 2);
}
