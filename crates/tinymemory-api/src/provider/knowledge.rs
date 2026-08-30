//! Optional families that expose *derived structure* over stored memory:
//! [`MemoryEntities`], [`MemoryGraph`], and [`MemoryDiff`].
//!
//! Each is independently optional. A driver may have a key/value graph but no
//! entity index, or track source snapshots without either. The kernel filters
//! RPC registration and agent-tool assembly per family, so an absent family is
//! invisible rather than present-and-failing.
//!
//! As in [`crate::provider::content`], no configuration crosses this boundary:
//! extraction models, hotness decay curves, and snapshot retention are driver
//! concerns and appear in none of these signatures.

use std::collections::BTreeSet;

use async_trait::async_trait;

use crate::error::MemoryError;
use crate::graph::{GraphEdge, GraphNode, GraphView, GraphViewQuery};
use crate::provider::types::{
    ChunkEntityOccurrence, DiffReport, EntityHit, EntityOccurrence, SnapshotRef,
};
use crate::types::{GraphRelationRecord, MemoryKvRecord};

/// How many edges the default [`MemoryGraph::graph_view`] traversal scans per
/// predicate when it has to resolve *inbound* edges.
///
/// [`MemoryGraph::relations`] filters by subject and predicate but not by
/// object, so inbound expansion has no indexed form in this contract and the
/// default traversal falls back to a bounded scan. The bound exists so a graph
/// larger than memory cannot be pulled into a view; hitting it sets
/// [`GraphView::truncated`]. A driver whose store indexes the object column
/// should override `graph_view` and skip this path entirely.
pub const INBOUND_SCAN_LIMIT: usize = 4_096;

/// The entity index: who and what the stored memory is about.
///
/// ## Two readings of one index, and why both are here
///
/// [`Self::entities`] and [`Self::entity_edges`] read the index the way an
/// agent does: inside one namespace, ranked by what is warm.
/// [`Self::top_entities`], [`Self::chunk_entities`] and
/// [`Self::entity_chunk_ids`] read it the way a browser does: across the whole
/// store, ranked by what is actually there, and joined back to the chunks the
/// observations came from. Neither reading is derivable from the other — each
/// method says which assumption breaks — so the family carries both rather
/// than one call with a mode flag.
///
/// ## Why the second three are defaulted and the first three are not
///
/// The first three have been in this trait since it existed; every driver that
/// compiles implements them. The second three arrived later, and making them
/// required would break every out-of-tree driver at its next `cargo build` for
/// a capability it may genuinely not have — the trait equivalent of a
/// non-additive wire change, which this contract does not make.
///
/// So they default to [`MemoryError::Unsupported`], carrying
/// `entities.<method>` rather than the bare family name: the family *is*
/// supported, and an operator reading "unsupported capability: entities" from
/// a driver whose entity list works would be chasing the wrong thing. A driver
/// that can answer these should override them; the embedded engine does.
///
/// ## `chunk_entities` changed shape after it was written, and that was allowed
///
/// It landed taking one `chunk_id` and returning [`EntityOccurrence`]. It now
/// takes a batch and returns [`ChunkEntityOccurrence`]. Re-cutting a member's
/// signature is normally out of bounds here — it breaks every driver at once,
/// and family-granular version negotiation cannot see it — and it is
/// legitimate exactly once, because this member has never been in a release.
/// It was added after `v1.4.0` and ships for the first time alongside this
/// change, so no driver anywhere implements the one-chunk form.
///
/// The alternative was to keep it and add a batched member beside it, leaving
/// two ways to ask one question and a per-chunk one no caller should reach
/// for. Correcting an unreleased shape costs nothing; carrying it costs every
/// reader after.
#[async_trait]
pub trait MemoryEntities: Send + Sync {
    /// List entities in a namespace, ranked by hotness when `query` is `None`
    /// and by match quality otherwise.
    ///
    /// # Errors
    ///
    /// Backend failures only; an unknown namespace yields an empty vector.
    async fn entities(
        &self,
        namespace: &str,
        query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<EntityHit>, MemoryError>;

    /// Edges incident to one entity, most relevant first.
    ///
    /// Returns [`GraphRelationRecord`] — the same shape [`MemoryGraph`] uses —
    /// so a caller that has both families does not have to reconcile two edge
    /// representations.
    ///
    /// # Errors
    ///
    /// Backend failures only; an unknown `entity_id` yields an empty vector
    /// rather than [`MemoryError::NotFound`], because "no edges" and "no such
    /// entity" are the same answer to this question.
    async fn entity_edges(
        &self,
        namespace: &str,
        entity_id: &str,
        limit: usize,
    ) -> Result<Vec<GraphRelationRecord>, MemoryError>;

    /// Record that these entities were just observed, updating hotness.
    ///
    /// Separate from the read path because hotness is a *write* the host
    /// triggers at known moments (a turn referenced these entities), not
    /// something a driver should infer from being queried — otherwise merely
    /// browsing the index would reshape ranking.
    ///
    /// # Errors
    ///
    /// Backend failures only. Unknown ids are ignored, not rejected.
    async fn touch_entities(
        &self,
        namespace: &str,
        entity_ids: &[String],
    ) -> Result<(), MemoryError>;

    /// The most-observed entities in the **whole store**, optionally narrowed
    /// to one kind.
    ///
    /// # Why this is not [`Self::entities`] with a wider scope
    ///
    /// [`Self::entities`] is namespace-scoped and hotness-ranked. This is
    /// neither: it reads the occurrence index as it stands — every namespace
    /// at once, ordered by how often each entity was indexed — which is what
    /// "who and what does this store know about at all" asks for. A caller
    /// cannot assemble that from the namespace-scoped call: it would have to
    /// enumerate namespaces and merge their rankings on hotness, and hotness
    /// is a per-driver, per-namespace number that does not survive a merge.
    ///
    /// Rows are [`EntityOccurrence`] rather than [`EntityHit`] for the reason
    /// that type's docs give — the index holds a surface sample and a count,
    /// and no hotness at all.
    ///
    /// `kind` is validated, not merely applied: an unrecognised kind is
    /// [`MemoryError::Invalid`], never an empty vector, because a misspelled
    /// filter that matched nothing is indistinguishable from a store that
    /// holds nothing. That is
    /// [`crate::provider::MemoryRetrieval::search_entities`]'s rule, kept the
    /// same here so one filter does not behave two ways.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Invalid`] for a `kind` the driver does not recognise,
    /// otherwise backend failures. An empty index yields an empty vector.
    async fn top_entities(
        &self,
        kind: Option<&str>,
        limit: usize,
    ) -> Result<Vec<EntityOccurrence>, MemoryError> {
        // Discarded rather than underscore-prefixed in the signature: the
        // parameter names are what rustdoc shows a driver author, and
        // `_kind` reads as vestigial where `kind` reads as the contract.
        let _ = (kind, limit);
        Err(MemoryError::unsupported_raw("entities.top_entities"))
    }

    /// Every entity indexed against these chunks, most-observed first.
    ///
    /// The inverse of [`Self::entity_chunk_ids`], and the only read in this
    /// family that starts from content instead of from an entity: it is how a
    /// caller labels chunks it already has — a page of retrieval hits, a
    /// screen of a browser — without re-running extraction over the text.
    ///
    /// Summary-node ids are accepted too, and answered from the same index.
    /// `chunk_ids` is named for the common case, not to exclude the other:
    /// refusing an id the index can answer would send the caller to raw SQL
    /// for the difference.
    ///
    /// # Why this takes a batch
    ///
    /// It is asked once per rendered list, not once per chunk opened. A
    /// caller labelling fifteen hundred rows one call at a time makes fifteen
    /// hundred bus round trips to read one index — exactly the fan-out
    /// [`ChunkDetail`]'s docs were written to prevent, at the scale that makes
    /// it fatal rather than merely wasteful. The caller bounds the work by
    /// choosing the batch, which is why there is still no `limit` (see below);
    /// a driver that considers a batch too large refuses it with
    /// [`MemoryError::Invalid`] rather than answering for part of it, because
    /// a refusal is visible to the caller and a truncation is not.
    ///
    /// Rows are [`ChunkEntityOccurrence`] rather than [`EntityOccurrence`]
    /// because a flat list over many chunks has no other way back to the chunk
    /// a row describes. **Group by [`ChunkEntityOccurrence::chunk_id`]; never
    /// index by position.** A chunk the extractor has not reached contributes
    /// no rows at all, so the result covers fewer chunks than were asked for
    /// and says nothing about their order.
    ///
    /// One entity may still appear more than once for the same chunk, once per
    /// distinct [`EntityOccurrence::surface`], because the two forms are the
    /// evidence a caller has for how that chunk actually named it.
    /// Deduplicating by id here would throw that away and leave `surface`
    /// meaning "whichever row sorted last".
    ///
    /// # Filtering by kind
    ///
    /// `None` returns every kind. `Some` narrows to the kinds listed, and the
    /// list is **validated, not merely applied**: an unrecognised kind is
    /// [`MemoryError::Invalid`], never an empty result, because a misspelled
    /// filter that matched nothing is indistinguishable from a chunk nothing
    /// was extracted from. That is [`Self::top_entities`]'s rule, kept the same
    /// here so one filter does not behave two ways.
    ///
    /// `Some(&[])` is a filter admitting no kind and yields an empty vector.
    /// It is not a second spelling of `None` — `None` is already how a caller
    /// says "no filter", so reading an empty slice as "everything" would leave
    /// the `Option` meaning nothing.
    ///
    /// # Why there is still no `limit`
    ///
    /// Every other list in this family is bounded by one, and this one is
    /// bounded by the thing it reads: a chunk's rows are what that chunk's own
    /// extraction produced, and the caller chose how many chunks to ask about.
    /// There is no ranking for a cut-off to respect — a truncated answer would
    /// silently describe some of the batch and not the rest, with nothing on
    /// the wire to say which.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Invalid`] for an unrecognised kind, or for a batch the
    /// driver declines to answer whole. Otherwise backend failures: unknown
    /// ids yield no rows rather than
    /// [`MemoryError::NotFound`], for [`Self::entity_edges`]'s reason —
    /// "nothing was extracted from it" and "there is no such chunk" are the
    /// same answer to this question.
    ///
    /// [`ChunkDetail`]: crate::provider::ChunkDetail
    async fn chunk_entities(
        &self,
        chunk_ids: &[String],
        kinds: Option<&[String]>,
    ) -> Result<Vec<ChunkEntityOccurrence>, MemoryError> {
        let _ = (chunk_ids, kinds);
        Err(MemoryError::unsupported_raw("entities.chunk_entities"))
    }

    /// The ids of the chunks one entity was observed in, newest first.
    ///
    /// The inverse of [`Self::chunk_entities`], and the member that makes an
    /// entity usable as a filter: a caller that has resolved a name to a
    /// canonical id gets the content behind it and reads that content through
    /// [`crate::provider::MemoryChunks`], which is where chunk bodies belong.
    /// Returning the chunks themselves would duplicate that family's shape
    /// here and double the bytes for a caller that already holds them.
    ///
    /// [`Self::entity_edges`] does not cover this and cannot: it answers
    /// entity-to-entity, and there is no path from an edge back to the text
    /// the co-occurrence was observed in.
    ///
    /// **Chunks only.** A driver that also indexes derived nodes — summaries,
    /// rollups — leaves them out: they are not chunks, and a caller filtering
    /// a chunk list by these ids would find ids that match nothing.
    ///
    /// # Errors
    ///
    /// Backend failures only; an unknown `entity_id` yields an empty vector.
    async fn entity_chunk_ids(
        &self,
        entity_id: &str,
        limit: usize,
    ) -> Result<Vec<String>, MemoryError> {
        let _ = (entity_id, limit);
        Err(MemoryError::unsupported_raw("entities.entity_chunk_ids"))
    }
}

/// The key/value and relation graph tier.
///
/// `namespace` is `Option<&str>` throughout: `None` addresses the global,
/// namespace-less slice, matching the storage shape of
/// [`MemoryKvRecord::namespace`] and [`GraphRelationRecord::namespace`].
#[async_trait]
pub trait MemoryGraph: Send + Sync {
    /// Read one key/value record.
    ///
    /// # Errors
    ///
    /// A missing key is `Ok(None)`; `Err` is reserved for backend failures.
    async fn kv_get(
        &self,
        namespace: Option<&str>,
        key: &str,
    ) -> Result<Option<MemoryKvRecord>, MemoryError>;

    /// Upsert one key/value record.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Invalid`] for a rejected key, otherwise backend failures.
    async fn kv_put(
        &self,
        namespace: Option<&str>,
        key: &str,
        value: serde_json::Value,
    ) -> Result<(), MemoryError>;

    /// Delete one key/value record, reporting whether it existed.
    ///
    /// # Errors
    ///
    /// Backend failures only.
    async fn kv_delete(&self, namespace: Option<&str>, key: &str) -> Result<bool, MemoryError>;

    /// List key/value records, optionally restricted to a key prefix.
    ///
    /// # Errors
    ///
    /// Backend failures only.
    async fn kv_list(
        &self,
        namespace: Option<&str>,
        prefix: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryKvRecord>, MemoryError>;

    /// Query relations, narrowing by subject and/or predicate.
    ///
    /// Both filters are `None`-able so one method covers "everything about this
    /// subject", "every edge of this type", and "the whole slice", instead of
    /// three near-identical methods.
    ///
    /// # Errors
    ///
    /// Backend failures only.
    async fn relations(
        &self,
        namespace: Option<&str>,
        subject: Option<&str>,
        predicate: Option<&str>,
        limit: usize,
    ) -> Result<Vec<GraphRelationRecord>, MemoryError>;

    /// Upsert one relation, keyed by `(namespace, subject, predicate, object)`.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Invalid`] for a malformed edge, otherwise backend
    /// failures.
    async fn put_relation(&self, relation: GraphRelationRecord) -> Result<(), MemoryError>;

    /// Assemble a bounded, renderable slice of the graph.
    ///
    /// This is to [`Self::relations`] what
    /// [`crate::provider::MemoryTree::drill_down`] is to a raw node read: one
    /// call returns a node *together with its surroundings*, already joined
    /// into a node set and an edge set, so navigating a graph is a sequence of
    /// view calls rather than a client-side reassembly that every caller would
    /// write differently.
    ///
    /// # The default implementation
    ///
    /// Provided, not required: it breadth-first expands
    /// [`GraphViewQuery::seeds`] using [`Self::relations`] alone, so every
    /// existing driver gains a graph view without writing one, and a driver
    /// that advertises no [`crate::capabilities::Capability::Graph`] family
    /// still surfaces the same [`MemoryError::Unsupported`] its `relations`
    /// returns.
    ///
    /// It costs one `relations` call per node visited, including one final
    /// round at the outermost hop that adds no nodes and exists only to close
    /// edges *between* nodes already in the view — without it the outer ring
    /// renders as a star rather than as the graph it is. A driver with a native
    /// multi-hop traversal should override this and use it.
    ///
    /// Inbound expansion has no indexed form here — `relations` cannot filter
    /// by object — so [`crate::graph::GraphDirection::In`] and
    /// [`crate::graph::GraphDirection::Both`] fall back to a scan capped at
    /// [`INBOUND_SCAN_LIMIT`] per predicate.
    ///
    /// # Errors
    ///
    /// Whatever [`Self::relations`] returns. Bounds are never an error: a
    /// traversal that hits one returns the partial view with
    /// [`GraphView::truncated`] set.
    async fn graph_view(&self, query: &GraphViewQuery) -> Result<GraphView, MemoryError> {
        let namespace = query.namespace.as_deref();
        let mut view = GraphView {
            namespace: query.namespace.clone(),
            seeds: query.seeds.clone(),
            ..GraphView::default()
        };

        // Each predicate needs its own call: `relations` takes one, not a set.
        // An empty filter becomes the single unfiltered call rather than a
        // special case further down.
        let predicates: Vec<Option<&str>> = if query.predicates.is_empty() {
            vec![None]
        } else {
            query.predicates.iter().map(|p| Some(p.as_str())).collect()
        };

        // Nodes reached but never expanded, either because a bound was hit or
        // because they sit one hop past the requested depth. A set, not a
        // counter: the same boundary node is commonly reached from several
        // directions, and counting it twice would overstate what is left.
        let mut unexpanded: BTreeSet<String> = BTreeSet::new();

        // Unseeded: an overview, not a traversal. One bounded scan of the
        // slice, and the node set is whatever the returned edges touch.
        if query.seeds.is_empty() {
            for predicate in &predicates {
                // One past the ceiling: a call that asked for exactly
                // `max_edges` and got them cannot tell a full slice from a
                // truncated one.
                let records = self
                    .relations(
                        namespace,
                        None,
                        *predicate,
                        query.max_edges.saturating_add(1),
                    )
                    .await?;
                for record in records {
                    push_view_edge(&mut view, record, &mut unexpanded, query, 0);
                }
            }
            view.stats.frontier_remaining = unexpanded.len();
            view.recompute_stats();
            return Ok(view);
        }

        // Inbound edges are resolved from one scan per predicate rather than
        // one per node: the scan is the expensive part, and repeating it for
        // every node visited would multiply it by `max_nodes`.
        let mut inbound: Vec<GraphRelationRecord> = Vec::new();
        if query.direction.follows_in() {
            for predicate in &predicates {
                let records = self
                    .relations(namespace, None, *predicate, INBOUND_SCAN_LIMIT)
                    .await?;
                if records.len() >= INBOUND_SCAN_LIMIT {
                    view.truncated = true;
                }
                inbound.extend(records);
            }
        }

        let mut frontier: Vec<String> = Vec::new();
        for seed in &query.seeds {
            if view.nodes.iter().any(|n| &n.id == seed) {
                continue;
            }
            if view.nodes.len() >= query.max_nodes {
                unexpanded.insert(seed.clone());
                view.truncated = true;
                continue;
            }
            view.nodes.push(GraphNode::bare(seed.clone(), 0));
            frontier.push(seed.clone());
        }

        for hop in 0..=query.depth {
            if frontier.is_empty() {
                break;
            }
            let mut next: Vec<String> = Vec::new();
            for node_id in &frontier {
                let mut incident: Vec<GraphRelationRecord> = Vec::new();
                if query.direction.follows_out() {
                    for predicate in &predicates {
                        incident.extend(
                            self.relations(
                                namespace,
                                Some(node_id),
                                *predicate,
                                query.max_edges.saturating_add(1),
                            )
                            .await?,
                        );
                    }
                }
                if query.direction.follows_in() {
                    incident.extend(
                        inbound
                            .iter()
                            .filter(|record| &record.object == node_id)
                            .cloned(),
                    );
                }

                for record in incident {
                    if !query.accepts_predicate(&record.predicate) {
                        continue;
                    }
                    let other = if &record.subject == node_id {
                        record.object.clone()
                    } else {
                        record.subject.clone()
                    };
                    let known = view.nodes.iter().any(|n| n.id == other);
                    if !known {
                        // An edge to a node the view will not hold would
                        // dangle, so it is dropped either way — but *why* it
                        // was dropped matters. Reaching the requested depth is
                        // the caller getting what they asked for; hitting the
                        // node ceiling is not, and only the second makes the
                        // view truncated. Conflating them would set the flag on
                        // every finite traversal of a connected graph and leave
                        // it saying nothing.
                        if hop >= query.depth {
                            unexpanded.insert(other);
                            continue;
                        }
                        if view.nodes.len() >= query.max_nodes {
                            unexpanded.insert(other);
                            view.truncated = true;
                            continue;
                        }
                        view.nodes.push(GraphNode::bare(other.clone(), hop + 1));
                        next.push(other);
                    }
                    push_view_edge(&mut view, record, &mut unexpanded, query, hop);
                }
            }
            frontier = next;
        }

        view.stats.frontier_remaining = unexpanded.len();
        view.recompute_stats();
        Ok(view)
    }
}

/// Add one relation to a view, deduplicating by triple and honouring
/// [`GraphViewQuery::max_edges`].
///
/// A separate function rather than a closure so the borrow of `view` ends
/// between calls, which the traversal above needs while it is also pushing
/// nodes.
fn push_view_edge(
    view: &mut GraphView,
    record: GraphRelationRecord,
    unexpanded: &mut BTreeSet<String>,
    query: &GraphViewQuery,
    depth: u32,
) {
    if !query.accepts_predicate(&record.predicate) {
        return;
    }
    let triple = (
        record.subject.clone(),
        record.predicate.clone(),
        record.object.clone(),
    );
    if view
        .edges
        .iter()
        .any(|e| e.key() == (&triple.0, &triple.1, &triple.2))
    {
        return;
    }
    if view.edges.len() >= query.max_edges {
        unexpanded.insert(triple.0);
        unexpanded.insert(triple.2);
        view.truncated = true;
        return;
    }
    // The unseeded overview derives its node set from the edges it found; the
    // seeded traversal has already placed both endpoints.
    for id in [triple.0.clone(), triple.2.clone()] {
        if view.nodes.iter().any(|n| n.id == id) {
            continue;
        }
        if view.nodes.len() >= query.max_nodes {
            unexpanded.insert(id);
            view.truncated = true;
            return;
        }
        view.nodes.push(GraphNode::bare(id, depth));
    }
    view.edges.push(GraphEdge::from(record));
}

/// Snapshot capture and change computation over synced sources.
#[async_trait]
pub trait MemoryDiff: Send + Sync {
    /// Capture a snapshot of one source's current items.
    ///
    /// # Errors
    ///
    /// [`MemoryError::NotFound`] for an unknown `source_id`, otherwise backend
    /// failures.
    async fn capture_snapshot(&self, source_id: &str) -> Result<SnapshotRef, MemoryError>;

    /// List snapshots for one source, newest first.
    ///
    /// # Errors
    ///
    /// Backend failures only; an unknown `source_id` yields an empty vector.
    async fn snapshots(
        &self,
        source_id: &str,
        limit: usize,
    ) -> Result<Vec<SnapshotRef>, MemoryError>;

    /// Compute the change set between two snapshots of one source.
    ///
    /// `from` is `Option<&str>` so the first-ever diff — where there is no
    /// baseline and every item is an addition — is expressible without a
    /// separate method or a sentinel id.
    ///
    /// # Errors
    ///
    /// [`MemoryError::NotFound`] when either snapshot id is unknown, otherwise
    /// backend failures.
    async fn diff(
        &self,
        source_id: &str,
        from: Option<&str>,
        to: &str,
    ) -> Result<DiffReport, MemoryError>;
}
