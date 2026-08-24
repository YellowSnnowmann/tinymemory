//! Optional families that put content *into* memory and navigate it:
//! [`MemoryIngest`], [`MemoryDocuments`], and [`MemoryTree`].
//!
//! All three are optional. A driver that advertises none of them is still a
//! memory backend — it just accepts entries only through
//! [`crate::provider::MemoryCore::store`] and has no document tier and no
//! summary tree. The kernel unregisters the matching RPC methods and omits the
//! matching agent tools rather than registering handlers that fail.
//!
//! ## No configuration crosses this boundary
//!
//! Chunk sizes, embedding models, summariser prompts, seal thresholds, and
//! cascade policy are all *driver* concerns. None of them appear in these
//! signatures: the embedded driver reads them from the `MemoryConfig` it
//! already holds, and an external driver has its own. This was the sharpest
//! test of whether the M0 crate carve-out drew the line in the right place —
//! the families that looked most config-dependent turned out not to need any.

use async_trait::async_trait;

use crate::capabilities::Capability;
use crate::chunks::Chunk;
use crate::error::MemoryError;
use crate::provider::types::{IngestItem, IngestOutcome, SourceScope};
use crate::tree::{IngestRequest, QueryResult, SummaryForest, TreeLeaf, TreeStatus};
use crate::types::{NamespaceDocumentInput, NamespaceRetrievalContext, StoredMemoryDocument};

/// Bulk content ingestion — the driver owns chunking and embedding.
///
/// The distinction from [`crate::provider::MemoryCore::store`] is ownership of
/// the pipeline: `store` persists exactly one entry the caller has already
/// shaped, whereas ingest hands over raw source material and lets the driver
/// decide how to split, embed, and index it.
#[async_trait]
pub trait MemoryIngest: Send + Sync {
    /// Ingest one standalone document.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Invalid`] for content the driver refuses (empty body,
    /// unsupported MIME), otherwise backend failures.
    async fn ingest_document(&self, item: IngestItem) -> Result<IngestOutcome, MemoryError>;

    /// Ingest a run of chat messages that share a conversation.
    ///
    /// Taken as a batch rather than one call per message because chat chunking
    /// is inherently cross-message: a driver needs neighbouring turns to decide
    /// where a chunk boundary belongs. Ordering within `messages` is
    /// significant and must be preserved by the caller.
    ///
    /// # Errors
    ///
    /// As [`Self::ingest_document`]. Partial success is reported through the
    /// counts in [`IngestOutcome`], not as an error.
    async fn ingest_chat(&self, messages: Vec<IngestItem>) -> Result<IngestOutcome, MemoryError>;

    /// Ingest one email thread as its ordered run of messages.
    ///
    /// The argument shape is [`Self::ingest_chat`]'s, and the method is
    /// separate anyway, because the difference is on the way *in*: a driver
    /// splits an email thread at message boundaries and renders per-message
    /// headers, where a chat batch is chunked across turns. Routing mail
    /// through the chat method stores it as a conversation and loses the split,
    /// which is what a citation back to a single message stands on. Flattening
    /// it into [`Self::ingest_document`] loses the per-message structure
    /// entirely.
    ///
    /// Every item must carry the same `source_id` — the thread is the
    /// ingestion group, exactly as the conversation is for chat — and
    /// `timestamp` is what orders the messages, so a caller that omits it gets
    /// ingest time and a thread ordered by arrival.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Unsupported`] from a driver that predates this operation
    /// or has no mail path — it is *not* implied by the rest of the family,
    /// and a caller must be prepared for a driver that ingests documents and
    /// chat but not mail. Otherwise as [`Self::ingest_document`].
    async fn ingest_email(&self, _messages: Vec<IngestItem>) -> Result<IngestOutcome, MemoryError> {
        Err(MemoryError::unsupported(Capability::Ingest))
    }
}

/// The namespace-document tier: whole documents addressed by `(namespace, key)`.
///
/// Distinct from [`crate::provider::MemoryCore`] in granularity and in what is
/// stored: entries are short facts, documents are bodies with titles, tags,
/// source types, and structured metadata, and they carry their own ranked query
/// surface.
#[async_trait]
pub trait MemoryDocuments: Send + Sync {
    /// Upsert a document, returning its driver-assigned id.
    ///
    /// Keyed by `(namespace, key)` from the input: reusing a key replaces the
    /// existing document rather than creating a second one.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Invalid`] for a rejected input, otherwise backend
    /// failures.
    async fn put_document(&self, input: NamespaceDocumentInput) -> Result<String, MemoryError>;

    /// Fetch a document by `(namespace, key)`.
    ///
    /// # Errors
    ///
    /// A missing document is `Ok(None)`; `Err` is reserved for backend
    /// failures.
    async fn get_document(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<StoredMemoryDocument>, MemoryError>;

    /// List document summaries, optionally restricted to one namespace.
    ///
    /// # Errors
    ///
    /// Backend failures only.
    async fn list_documents(
        &self,
        namespace: Option<&str>,
    ) -> Result<serde_json::Value, MemoryError>;

    /// List every namespace containing documents.
    ///
    /// # Errors
    ///
    /// Backend failures only.
    async fn list_namespaces(&self) -> Result<Vec<String>, MemoryError>;

    /// Delete a document by its driver-assigned id.
    ///
    /// # Errors
    ///
    /// Backend failures only; a missing document is reported in the returned
    /// outcome rather than as an error.
    async fn delete_document(
        &self,
        namespace: &str,
        document_id: &str,
    ) -> Result<serde_json::Value, MemoryError>;

    /// Delete all data belonging to one namespace.
    ///
    /// # Errors
    ///
    /// Backend failures only.
    async fn clear_namespace(&self, namespace: &str) -> Result<(), MemoryError>;

    /// Run a ranked query over one namespace's documents.
    ///
    /// Returns both the ranked hits and the driver's rendered context text, so
    /// a caller that only wants something injectable does not have to
    /// re-assemble it (and re-assemble it differently from every other caller).
    ///
    /// # Errors
    ///
    /// Backend failures only; a query that matches nothing returns an empty
    /// hit list.
    async fn query_documents(
        &self,
        namespace: &str,
        query: &str,
        limit: usize,
    ) -> Result<NamespaceRetrievalContext, MemoryError>;

    /// Recall the highest-ranked context from a namespace without a query.
    ///
    /// This is a distinct engine operation rather than a query with an empty
    /// string: query-less recall applies the namespace's freshness and
    /// priority ranking without introducing a synthetic search term.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Unsupported`] when a provider predating this optional
    /// operation does not implement it, otherwise backend failures. An empty
    /// namespace returns empty context.
    async fn recall_documents(
        &self,
        _namespace: &str,
        _limit: usize,
    ) -> Result<NamespaceRetrievalContext, MemoryError> {
        Err(MemoryError::unsupported(Capability::Documents))
    }
}

/// The time-ordered summary tree: buffered leaves rolled up into hour → day →
/// month → year → root summaries.
///
/// Sealing and cascading are exposed as explicit calls rather than happening
/// implicitly on ingest because the **host** owns scheduling. A driver runs one
/// step when asked; it does not get to install its own background loop. This is
/// the same rule as the engine's `queue::run_once`.
///
/// # Navigating one node, and walking the whole forest
///
/// [`Self::drill_down`] addresses a node by id and returns it with its direct
/// children — enough to descend a tree a caller is already inside.
/// [`Self::summary_forest`] and [`Self::recent_leaves`] answer the question
/// that has no starting id: what trees exist, how they nest, and what content
/// hangs off them. Both are here rather than in
/// [`MemoryRetrieval`](crate::provider::MemoryRetrieval) because neither ranks
/// and neither takes a query; they are structure, not results.
///
/// The embedded driver happens to serve the two from different storage — the
/// markdown time tree on disk, the sealed summary forest in tables — and the
/// contract deliberately does not encode that split. See
/// [`crate::tree`] for the shapes and why they are described separately there.
#[async_trait]
pub trait MemoryTree: Send + Sync {
    /// Append raw content to the ingestion buffer for later sealing.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Invalid`] for a rejected request, otherwise backend
    /// failures.
    async fn append(&self, request: IngestRequest) -> Result<(), MemoryError>;

    /// Retrieve the chunks a single logical source contributed, newest first.
    ///
    /// `scope` is the per-turn allowlist and must be applied **inside** the
    /// driver's query, for the reasons in [`SourceScope`]. `None` means
    /// unrestricted.
    ///
    /// # Errors
    ///
    /// Backend failures only; an unknown `source_id` yields an empty vector.
    async fn query_source(
        &self,
        namespace: &str,
        source_id: &str,
        limit: usize,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<Chunk>, MemoryError>;

    /// Fetch one node together with its direct children, for navigation.
    ///
    /// # Errors
    ///
    /// [`MemoryError::NotFound`] when `node_id` does not exist in `namespace`.
    async fn drill_down(&self, namespace: &str, node_id: &str) -> Result<QueryResult, MemoryError>;

    /// Convert buffered content into leaf nodes, returning the resulting tree
    /// state.
    ///
    /// Idempotent when the buffer is empty: sealing nothing is a successful
    /// no-op, not an error, so a scheduler may call it unconditionally.
    ///
    /// # Errors
    ///
    /// Backend failures only.
    async fn seal(&self, namespace: &str) -> Result<TreeStatus, MemoryError>;

    /// Roll sealed leaves up through the parent levels, returning the resulting
    /// tree state.
    ///
    /// Idempotent for the same reason as [`Self::seal`].
    ///
    /// # Errors
    ///
    /// Backend failures only.
    async fn cascade(&self, namespace: &str) -> Result<TreeStatus, MemoryError>;

    /// Walk every sealed summary the store holds, across every tree.
    ///
    /// # Why [`Self::drill_down`] cannot answer this
    ///
    /// `drill_down` starts from a node id and returns that node with its
    /// direct children. A caller that wants the whole forest has no id to
    /// start from — that is what it is asking for — and no way to discover
    /// one, because nothing else in the contract enumerates trees. Walking it
    /// by repeated `drill_down` would also be one round trip per node, over a
    /// bus, to rebuild a shape the driver already has in one table.
    ///
    /// [`crate::provider::MemoryRetrieval::retrieve_children`] does not answer
    /// it either, for a different reason: it *ranks*. It needs a seed node and
    /// returns scored hits without a parent link, which is a reading list
    /// rather than a graph.
    ///
    /// # `scope` is a predicate, not a post-filter
    ///
    /// The allowlist must be applied **inside** the driver's query for the
    /// reasons in [`SourceScope`], and this member is the one where getting it
    /// wrong is least visible: an unscoped forest walk hands back every source
    /// in the store at once, which is precisely the shape a per-turn source
    /// gate exists to prevent. `None` means unrestricted and must be a
    /// decision, not a default the caller drifted into.
    ///
    /// A driver returns nodes whose tree the scope allows. It may therefore
    /// return a node whose `parent_id` names one it withheld; see
    /// [`crate::tree::TreeSummary::parent_id`] for what a caller does with
    /// that.
    ///
    /// # Bounds
    ///
    /// `limit` caps the nodes returned and the driver clamps it to its own
    /// cap — a caller cannot raise the ceiling by asking for more, the same
    /// rule [`crate::provider::ChunkQuery::limit`] carries. Hitting either
    /// bound sets [`SummaryForest::truncated`] rather than erroring.
    ///
    /// Tombstoned summaries are never returned. A driver that keeps them
    /// filters them out here; "deleted" is not a state a caller has to know
    /// about to draw a graph.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Unsupported`] from a driver that has a tree family but
    /// cannot enumerate it — deliberately not an empty forest, because a
    /// driver with trees reporting none is a lie a caller would render as an
    /// empty store. Backend failures otherwise; a store that has sealed
    /// nothing returns an empty, untruncated forest, which is true of it.
    async fn summary_forest(
        &self,
        _limit: usize,
        _scope: Option<&SourceScope>,
    ) -> Result<SummaryForest, MemoryError> {
        Err(MemoryError::unsupported(Capability::Tree))
    }

    /// The most recent leaves, each with the summary that sealed it, newest
    /// first.
    ///
    /// The forest's bottom edge. [`Self::summary_forest`] returns the summary
    /// nodes and the child ids they sealed over; this returns the leaves
    /// themselves with the back-pointer that says which summary claimed them,
    /// so a caller can attach content to the structure without one lookup per
    /// leaf.
    ///
    /// # Why not [`crate::provider::MemoryChunks::list_chunks`]
    ///
    /// That returns the same rows and drops the link: a [`Chunk`] does not say
    /// which summary sealed it, and the link is what makes a leaf part of a
    /// tree rather than a loose row. It is also the half that changes without
    /// the chunk changing — a leaf gains a parent when the scheduler seals it,
    /// long after ingest.
    ///
    /// Both halves are separate calls rather than one combined read because
    /// the two bounds are separate: a caller may want the whole forest
    /// skeleton and only the newest few hundred leaves, and folding them into
    /// one response would make the smaller bound pay for the larger.
    ///
    /// # Bounds and scope
    ///
    /// As [`Self::summary_forest`]: `limit` is clamped by the driver, and
    /// `scope` is applied inside the query, before the limit, so a disallowed
    /// source cannot starve permitted ones out of the page.
    ///
    /// [`TreeLeaf::preview`] is a label, capped at
    /// [`crate::tree::LEAF_PREVIEW_CHARS`] characters. Bodies are
    /// [`crate::provider::MemoryChunks::chunk_detail`]'s job, one row at a
    /// time; a forest-sized read carrying whole bodies would not fit a frame.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Unsupported`] on the same terms as
    /// [`Self::summary_forest`]. Backend failures otherwise; a store with no
    /// leaves returns an empty vector.
    async fn recent_leaves(
        &self,
        _limit: usize,
        _scope: Option<&SourceScope>,
    ) -> Result<Vec<TreeLeaf>, MemoryError> {
        Err(MemoryError::unsupported(Capability::Tree))
    }
}
