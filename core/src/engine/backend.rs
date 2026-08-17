//! The engine's own memory surface, reached through one door.
//!
//! Issue #18 §C1 asks that nothing outside this module name the `tinycortex`
//! crate. Eighty-three files did, in two hundred and ninety-six places, which
//! made the engine's shape an ambient fact of the whole crate rather than a
//! dependency anyone had chosen — and made "what would a second engine have to
//! provide?" a question no one could answer without reading all of them.
//!
//! Re-exporting rather than wrapping is deliberate. A wrapper layer over three
//! hundred call sites would be a second surface to keep in step with the first,
//! which is the failure §A1 had just finished deleting. What this buys is not
//! insulation from the engine's API — the call sites still use it verbatim —
//! but a single place that *names* it, so the coupling is enumerable: this file
//! is the list of everything `tinymemory-core` needs an engine to provide.
//!
//! # Why this is not `MemoryProvider`
//!
//! §A3 proposes routing these call sites through `&dyn MemoryProvider` instead.
//! That is not possible, and the reason is worth recording where the next
//! reader will find it. Core does not *consume* the engine's memory API; it
//! shares the engine's SQLite database. Thirty-four files here hold a
//! `rusqlite::Transaction` or a `Connection`, and the entry points they call
//! take them:
//!
//! ```text
//! upsert_buffer_tx(tx: &Transaction<'_>, buf: &Buffer) -> Result<()>
//! shared_connection(config: &MemoryConfig) -> Result<Arc<PMutex<Connection>>>
//! ```
//!
//! Serving those through the contract would put `rusqlite` in
//! `tinymemory-api`, which its own manifest forbids and CI now enforces. The
//! honest description is that core and the engine co-implement one store, and
//! separating them is a decomposition rather than a routing change.
//!
//! Nested under a module rather than re-exported flat because the seam already
//! has its own `ingest` and `sync` modules, which are host-side pieces and
//! not the engine's.

// The engine's submodules.
pub use tinycortex::memory::{
    archivist, chunks, conversations, diff, graph, health, ingest, people, queue, retrieval, score,
    sources, store, sync, tool_memory, tree, types,
};

// …and the items it re-exports at its own top level, which call sites reach for
// by the same short paths. Listed rather than globbed so this file stays the
// enumerable answer to "what does core need an engine to provide".
pub use tinycortex::memory::{
    GraphRelationRecord, InMemoryMemoryStore, MemoryCategory, MemoryConfig, MemoryEngineError,
    MemoryEngineResult, MemoryEntry, MemoryId, MemoryInput, MemoryItemKind, MemoryKvRecord,
    MemoryQuery, MemoryRecord, MemoryResult, MemoryStore, MemoryTaint, NamespaceDocumentInput,
    NamespaceMemoryHit, NamespaceQueryResult, NamespaceRetrievalContext, NamespaceSummary,
    RecallOpts, RetrievalScoreBreakdown, SearchHit, StoreError, StoredMemoryDocument,
    WeightProfile, GLOBAL_NAMESPACE,
};

// The storage trait itself. Since §A2 this is the contract's trait, not the
// engine's — the engine re-exports the same one.
pub use tinycortex::memory::Memory;
