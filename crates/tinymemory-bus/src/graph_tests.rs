//! Tests for the bounded graph-view model.

// A failed assertion in a test is a panic either way; `unwrap`/`expect` here say
// what the invariant was. Same allowance every other test module in this crate
// takes.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use super::*;

fn edge(subject: &str, predicate: &str, object: &str) -> GraphEdge {
    GraphEdge {
        subject: subject.to_string(),
        predicate: predicate.to_string(),
        object: object.to_string(),
        evidence_count: 1,
        weight: edge_weight(1),
        updated_at: 0.0,
        document_ids: Vec::new(),
        chunk_ids: Vec::new(),
        attrs: serde_json::Value::Null,
    }
}

#[test]
fn direction_follows_the_sides_it_names() {
    assert!(GraphDirection::Out.follows_out());
    assert!(!GraphDirection::Out.follows_in());
    assert!(GraphDirection::In.follows_in());
    assert!(!GraphDirection::In.follows_out());
    assert!(GraphDirection::Both.follows_out());
    assert!(GraphDirection::Both.follows_in());
}

#[test]
fn direction_defaults_to_outbound() {
    assert_eq!(GraphDirection::default(), GraphDirection::Out);
}

#[test]
fn edge_weight_saturates_and_never_reaches_zero() {
    assert!(edge_weight(0) > 0.0);
    assert!(edge_weight(1) > edge_weight(0));
    assert!(edge_weight(50) < 1.0);
    // Saturating, not linear: the first observations are worth far more than
    // the fiftieth.
    assert!(edge_weight(1) - edge_weight(0) > edge_weight(50) - edge_weight(49));
}

#[test]
fn edge_projects_a_relation_record_and_derives_its_weight() {
    let record = GraphRelationRecord {
        namespace: Some("conversation:thread-1".into()),
        subject: "ada".into(),
        predicate: "works_with".into(),
        object: "charles".into(),
        attrs: serde_json::json!({ "since": 1843 }),
        updated_at: 12.5,
        evidence_count: 3,
        order_index: None,
        document_ids: vec!["doc-1".into()],
        chunk_ids: vec!["chunk-1".into()],
    };
    let edge = GraphEdge::from(record);
    assert_eq!(edge.key(), ("ada", "works_with", "charles"));
    assert_eq!(edge.evidence_count, 3);
    assert!((edge.weight - edge_weight(3)).abs() < f64::EPSILON);
    assert_eq!(edge.document_ids, vec!["doc-1".to_string()]);
}

#[test]
fn query_defaults_are_the_cheapest_useful_call() {
    let query = GraphViewQuery::around("ada");
    assert_eq!(query.seeds, vec!["ada".to_string()]);
    assert_eq!(query.depth, 1);
    assert_eq!(query.direction, GraphDirection::Out);
    assert!(query.namespace.is_none());
    assert!(query.predicates.is_empty());
}

#[test]
fn query_builders_compose() {
    let query = GraphViewQuery::around("ada")
        .in_namespace("document:papers")
        .with_depth(3)
        .with_direction(GraphDirection::Both)
        .with_predicates(vec!["cites".into()])
        .with_bounds(10, 20);
    assert_eq!(query.namespace.as_deref(), Some("document:papers"));
    assert_eq!(query.depth, 3);
    assert_eq!(query.direction, GraphDirection::Both);
    assert_eq!(query.max_nodes, 10);
    assert_eq!(query.max_edges, 20);
}

#[test]
fn an_empty_predicate_filter_accepts_everything() {
    let query = GraphViewQuery::default();
    assert!(query.accepts_predicate("cites"));
    assert!(query.accepts_predicate("anything"));
}

#[test]
fn a_predicate_filter_rejects_what_it_does_not_name() {
    let query = GraphViewQuery::default().with_predicates(vec!["cites".into()]);
    assert!(query.accepts_predicate("cites"));
    assert!(!query.accepts_predicate("works_with"));
}

#[test]
fn overview_scopes_to_a_namespace_without_seeding() {
    let query = GraphViewQuery::overview("learning:rust");
    assert_eq!(query.namespace.as_deref(), Some("learning:rust"));
    assert!(query.seeds.is_empty());
}

#[test]
fn recompute_stats_counts_degrees_within_the_view() {
    let mut view = GraphView {
        nodes: vec![
            GraphNode::bare("ada", 0),
            GraphNode::bare("charles", 1),
            GraphNode::bare("lovelace", 1),
        ],
        edges: vec![
            edge("ada", "works_with", "charles"),
            edge("ada", "known_as", "lovelace"),
        ],
        ..GraphView::default()
    };
    view.recompute_stats();
    assert_eq!(view.stats.node_count, 3);
    assert_eq!(view.stats.edge_count, 2);
    assert_eq!(view.stats.max_depth, 1);
    assert_eq!(view.nodes[0].degree, 2);
    assert_eq!(view.nodes[1].degree, 1);
    assert_eq!(view.nodes[2].degree, 1);
}

#[test]
fn recompute_stats_on_an_empty_view_reports_zero_depth() {
    let mut view = GraphView::empty(Some("conversation:thread-1".into()));
    view.recompute_stats();
    assert_eq!(view.stats.max_depth, 0);
    assert_eq!(view.stats.node_count, 0);
    assert_eq!(view.namespace.as_deref(), Some("conversation:thread-1"));
}

#[test]
fn prune_dangling_edges_enforces_the_self_contained_invariant() {
    let mut view = GraphView {
        nodes: vec![GraphNode::bare("ada", 0), GraphNode::bare("charles", 1)],
        edges: vec![
            edge("ada", "works_with", "charles"),
            edge("ada", "cites", "absent"),
            edge("absent", "cites", "ada"),
        ],
        ..GraphView::default()
    };
    assert_eq!(view.prune_dangling_edges(), 2);
    assert_eq!(view.edges.len(), 1);
    assert_eq!(view.edges[0].key(), ("ada", "works_with", "charles"));
}

#[test]
fn prune_dangling_edges_keeps_a_clean_view_untouched() {
    let mut view = GraphView {
        nodes: vec![GraphNode::bare("ada", 0), GraphNode::bare("charles", 1)],
        edges: vec![edge("ada", "works_with", "charles")],
        ..GraphView::default()
    };
    assert_eq!(view.prune_dangling_edges(), 0);
    assert_eq!(view.edges.len(), 1);
}

#[test]
fn a_bare_node_labels_itself_by_its_id() {
    let node = GraphNode::bare("ada", 2);
    assert_eq!(node.label, "ada");
    assert_eq!(node.depth, 2);
    assert_eq!(node.degree, 0);
    assert_eq!(node.kind, GraphNodeKind::Unknown);
}

#[test]
fn node_kind_round_trips_through_its_wire_strings() {
    for (kind, wire) in [
        (GraphNodeKind::Unknown, "\"unknown\""),
        (GraphNodeKind::Entity, "\"entity\""),
        (GraphNodeKind::Document, "\"document\""),
        (GraphNodeKind::Chunk, "\"chunk\""),
        (GraphNodeKind::TreeNode, "\"tree_node\""),
        (GraphNodeKind::Kv, "\"kv\""),
    ] {
        assert_eq!(serde_json::to_string(&kind).unwrap(), wire);
        assert_eq!(
            serde_json::from_str::<GraphNodeKind>(wire).unwrap(),
            kind,
            "round trip for {wire}"
        );
    }
}

#[test]
fn a_driver_specific_node_kind_is_carried_verbatim() {
    let kind = GraphNodeKind::Other("commit".into());
    let wire = serde_json::to_string(&kind).unwrap();
    assert_eq!(serde_json::from_str::<GraphNodeKind>(&wire).unwrap(), kind);
}

#[test]
fn a_query_deserializes_from_its_bounds_alone() {
    let query: GraphViewQuery = serde_json::from_str(r#"{"seeds":["ada"]}"#).unwrap();
    assert_eq!(query.depth, 1);
    assert_eq!(query.max_nodes, 256);
    assert_eq!(query.max_edges, 512);
    assert_eq!(query.direction, GraphDirection::Out);
}

#[test]
fn a_view_round_trips_through_json() {
    let mut view = GraphView {
        namespace: Some("learning:rust".into()),
        seeds: vec!["ada".into()],
        nodes: vec![GraphNode::bare("ada", 0), GraphNode::bare("charles", 1)],
        edges: vec![edge("ada", "works_with", "charles")],
        truncated: true,
        ..GraphView::default()
    };
    view.recompute_stats();
    let wire = serde_json::to_string(&view).unwrap();
    let decoded: GraphView = serde_json::from_str(&wire).unwrap();
    assert_eq!(decoded.nodes.len(), 2);
    assert_eq!(decoded.edges.len(), 1);
    assert!(decoded.truncated);
    assert_eq!(decoded.stats.edge_count, 1);
    assert_eq!(decoded.seeds, vec!["ada".to_string()]);
}
