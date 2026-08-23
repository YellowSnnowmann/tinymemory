# Graph View, Document Intake, and the Namespace Convention

**Status:** Implemented
**Owner:** TinyMemory maintainers

Three additions to the contract that share one motivation: a host should be
able to put content into the memory layer and read structure back out of it
without knowing which engine is bound.

## Problem

The contract had three gaps that every host was closing for itself, and closing
differently.

1. **There was no way to ask for a graph.** `MemoryGraph::relations` answers
   "which edges match this filter" and returns a flat list. A caller that wants
   to render a graph, or hand one to an agent, needs the node set as well, needs
   to know how far each node sits from where it started, and needs the answer
   bounded so an over-connected hub cannot return the whole store. The summary
   tree already had this shape — `MemoryTree::drill_down` returns a node
   *together with* its children — and the graph tier did not.

2. **There was no way to hand the memory layer a file.** `MemoryIngest` takes
   `IngestItem::content`, "already decoded to text". Every host therefore owned
   format detection, PDF and DOCX extraction, HTML cleanup, and the choice of
   which capability family to write into. Two hosts uploading the same file to
   two engines got two different results.

3. **Namespaces meant whatever a host decided.** They are the only partitioning
   primitive the contract has, and they cross it as a bare `&str`. "Conversational
   memory", "document memory" and "learnings" were three ad-hoc prefixes per
   host, so nothing downstream could act on the distinction.

## Goals

- One call that returns a bounded, renderable slice of the graph, available on
  every driver that has a relation tier — including drivers that write no new
  code.
- One path that takes a file or a URL, converts it to markdown, and stores it
  in whichever engine is bound, reporting what it actually did.
- One shared convention for what goes in a namespace string, without changing
  any trait signature.

## Non-goals

- **Typed namespaces in trait signatures.** A driver's container vocabulary is
  its own; threading a namespace type through eighteen families would force
  every engine to agree on a shape none of them share. The gap was a convention,
  not a type in the signatures.
- **PDF and DOCX extraction in this workspace.** Which extractor a deployment
  uses is its own decision. The contract is the seam, not the implementation.
- **Scheduling, credentials, retries, or robots.txt on the URL path.** Host
  policy, by the same rule that keeps them out of a driver.
- **A permission boundary on namespaces.** Validation is not authorisation.

## Proposed behavior

### 1. `MemoryGraph::graph_view`

```rust
async fn graph_view(&self, query: &GraphViewQuery) -> Result<GraphView, MemoryError>;
```

A **provided** method, not a required one. The default implementation
breadth-first expands `GraphViewQuery::seeds` using `relations` alone, so every
existing driver gains a graph view without writing one — and a driver with no
graph family surfaces the same `Unsupported` its `relations` already returns,
rather than a misleading empty view.

`GraphView` carries `nodes`, `edges`, `seeds`, `truncated` and `stats`. Bounds
(`depth`, `max_nodes`, `max_edges`, `predicates`, `direction`) live on the
query, because the caller is the only party that knows how big an answer it can
render.

The default traversal costs one `relations` call per node visited, including one
final round at the outermost hop that adds no nodes and exists only to close
edges *between* nodes already in the view. Inbound expansion has no indexed form
in this contract — `relations` cannot filter by object — so `In` and `Both` fall
back to a scan capped at `INBOUND_SCAN_LIMIT` per predicate. A driver with a
native multi-hop traversal should override the method.

### 2. Document and URL intake

A new crate, `tinymemory-documents`, in three parts:

- `DocumentFormat::sniff(bytes, filename, mime)` — magic bytes, then declared
  MIME, then filename, then the bytes themselves.
- `DocumentConverter` — an object-safe async trait. `NativeConverter` covers
  markdown, plain text and HTML with no dependencies; `ConverterChain` composes
  it with whatever a host binds for PDF and DOCX.
- `DocumentIntake` — converts, then writes through the best family the driver
  implements: `MemoryIngest` (chunked), else `MemoryDocuments`, else
  `MemoryCore::store`. `IntakeReceipt::route` reports which.

`fetch::fetch_url` (feature `network`) fetches one URL into a `RawDocument`,
reusing the SSRF guard `tinymemory-sources` already has.

`DataSource` gains `Upload` and `WebPage`, both feeding `SourceKind::Document`.
Distinct from the connector variants because there is no upstream provider to
re-read from: the bytes arrived once, and a re-sync path must not assume it can
refetch them.

### 3. The namespace convention

`<section>:<scope>`, split at the first colon.

```text
conversation:thread-8f21     document:handbook       learning:rust-async
entity:people                profile:default         tool:github
source:acme-wiki             research-notes          (unsectioned — legacy)
```

`tinymemory_api::namespace::{Namespace, MemorySection}` parses, builds and
validates. `MemorySection` is a closed vocabulary plus `Custom`, because a
closed vocabulary with no escape hatch gets worked around with prefixes nobody
agrees on — which is the problem it exists to solve.

## Invariants and constraints

**Graph view**

- Every id named by an edge in `GraphView::edges` is present in
  `GraphView::nodes`. A driver that cannot honour that drops the edge.
- `truncated` means *a bound was hit*. Reaching the requested `depth` is not
  truncation: conflating them would set the flag on every finite traversal of a
  connected graph and leave it saying nothing. Nodes reached but not expanded —
  for either reason — are counted in `stats.frontier_remaining`.
- Bounds are never an error. A traversal that hits one returns a partial view.
- Traversal terminates on cyclic graphs and visits each node once.

**Intake**

- Taint is passed through untouched. Intake never assigns provenance; the
  default is the closed one (`ExternalSync`).
- Size is checked on the raw bytes, before conversion.
- A conversion that produces no text is an error, not an empty success.
- Derived keys are deterministic: re-ingesting the same document upserts.
- The namespace is validated before any write.

**Namespaces**

- Parse and render are inverses, including for unsectioned and custom names.
- Every namespace written before the convention existed still parses and renders
  back byte-for-byte.
- A `..` path segment is rejected, never sanitised: sanitising would silently
  change which container a write lands in.

## Acceptance criteria

- `graph_view` returns seeds, neighbours and the edges between them against a
  driver that implements only `relations`; reports `Unsupported` against one
  that implements none of the graph family; terminates on a cycle; and emits no
  dangling edge at any bound.
- A markdown, plain-text or HTML upload lands in the chunked family when the
  driver has one, the document tier when it has that, and the mandatory family
  otherwise — with the receipt naming which.
- A PDF upload against a build with no PDF converter fails with an error naming
  the format, and nothing reaches the driver.
- A URL fetch to a loopback, private or link-local target is refused.
- Every section helper round-trips through `parse`; a legacy bare name is
  unsectioned and unchanged.

## Open questions

- **PDF and DOCX conversion is unimplemented in this workspace.** TinyDocs was
  the intended provider, but as of this writing it *generates* DOCX
  (`GenerateDocx(DocumentSpec) -> Vec<u8>`) and exposes no extraction surface.
  A TinyDocs-backed `DocumentConverter` needs an `ExtractText`-shaped method on
  the TinyDocs bus service first; until then the chain refuses both formats with
  an error that names them.
- Whether the TinyCortex and Cognee adapters should override `graph_view` with
  their native traversals. The default is correct everywhere; it is not the
  fastest anywhere.
