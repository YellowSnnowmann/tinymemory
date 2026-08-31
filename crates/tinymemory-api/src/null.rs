//! [`NullMemoryProvider`] — the reference driver that stores nothing.
//!
//! ## What it is for
//!
//! A memory subsystem that is compiled out, disabled by configuration, or
//! explicitly bound to `driver = "null"` still has to bind *something*: the
//! kernel's registry holds exactly one driver per slot, and code that reaches
//! the slot must find a value rather than an `Option` it has to unwrap at every
//! call site. This is that value. It replaces the hand-written per-domain
//! `stub.rs` files with one generic answer.
//!
//! It is also the fixture the capability-degradation tests bind: with it in the
//! slot, the optional families are unadvertised, so their RPC methods are
//! unregistered and their agent tools are absent — and the core still boots.
//!
//! And it is the existence proof for the mandatory set: if
//! [`crate::provider::MemoryCore`], [`crate::provider::MemoryRecall`], and
//! [`crate::provider::MemoryPortability`] could not be implemented without a
//! storage engine, they would be the wrong three to have made mandatory.
//!
//! ## `/dev/null` semantics, and what that costs
//!
//! Writes are **accepted and discarded**; reads return empty. This mirrors the
//! Unix device the driver is named after, and it is the only behaviour that
//! lets the mandatory three be advertised honestly: a `store` that returned
//! [`crate::error::MemoryError::Unsupported`] would contradict advertising
//! [`crate::capabilities::Capability::Core`], and one that returned a hard
//! error would turn every optional auto-capture into a user-visible failure.
//!
//! The cost is real: content written here is gone. That is acceptable for a
//! subsystem the operator turned off, and unacceptable as a fallback for a
//! driver that failed to bind — **that** case falls back to the embedded
//! default, never to this. Do not wire it as a general-purpose failure mode.
//!
//! ## Why it implements the optional families but advertises three
//!
//! The optional families are implemented and every method returns
//! [`crate::error::MemoryError::Unsupported`] naming its family, but the
//! `as_*` accessors return `None` and
//! [`crate::provider::MemoryProvider::capabilities`] lists only the mandatory
//! three. So:
//!
//! - through `&dyn MemoryProvider` — the only way product code sees a driver —
//!   an unadvertised family is simply **unreachable**, which is the intended
//!   degradation;
//! - through the concrete type, a direct call yields a typed, *named*
//!   `Unsupported` error, which is what makes the contract's error mapping
//!   testable without writing a second mock.
//!
//! [`crate::provider::audit_provider`] confirms the two views agree.

use async_trait::async_trait;

use crate::capabilities::{Capabilities, Capability};
use crate::error::MemoryError;
use crate::goals::GoalsDoc;
use crate::health::MemoryHealth;
use crate::learning::LearningCandidate;
use crate::operations::{AnswerRequest, AnswerResponse, RawMemoryEvent};
use crate::provider::types::{
    DiffReport, EntityHit, ExportPage, ExportRecord, FlushOutcome, ImportOutcome, IngestItem,
    IngestOutcome, MaintenanceReport, ResetOutcome, SnapshotRef, SourceItem, SourceScope,
};
use crate::provider::{
    AddressBookSeedOutcome, ChunkDetail, ChunkEmbedding, ChunkQuery, CodingSessionIngestReport,
    CodingSessionIngestRequest, CodingSessionSource, CoverWindowQuery, EntityMatch, FacetType,
    FastRetrieveQuery, MemoryAnswer, MemoryChunks, MemoryCodingSessions, MemoryConversationIngest,
    MemoryCore, MemoryDiff, MemoryDocumentIngest, MemoryDocuments, MemoryEntities,
    MemoryEventIngest, MemoryGoals, MemoryGraph, MemoryIngest, MemoryLearningIngest,
    MemoryMaintenance, MemoryPeople, MemoryPortability, MemoryProfile, MemoryProvider,
    MemoryRecall, MemoryRetrieval, MemoryScoring, MemorySourceSink, MemorySourceSync,
    MemoryToolMemory, MemoryTree, PersonHandle, PersonInteraction, PersonRecord, PersonScore,
    ProfileFacet, RankedPerson, RawArchiveCoverage, RawRebuildOutcome, ResolvedPerson,
    RetrievalHit, RetrievalResponse, SourceRetrievalQuery, SourceSyncState, SourceSyncStatus,
    SyncAuditEntry, SyncRunOutcome, UserState,
};
use crate::recall::OwnedRecallOpts;
use crate::tool_memory::ToolMemoryRule;
use crate::tree::{IngestRequest, QueryResult, TreeStatus};
use crate::types::{
    GraphRelationRecord, MemoryCategory, MemoryEntry, MemoryKvRecord, MemoryTaint,
    NamespaceDocumentInput, NamespaceMemoryHit, NamespaceRetrievalContext, NamespaceSummary,
    StoredMemoryDocument,
};

/// The [`driver_id`](MemoryProvider::driver_id) this driver reports.
pub const NULL_DRIVER_ID: &str = "null";

/// Shorthand for the `Unsupported` error every unadvertised family returns.
fn unsupported<T>(capability: Capability) -> Result<T, MemoryError> {
    Err(MemoryError::unsupported(capability))
}

/// A driver that accepts every write, discards it, and returns nothing.
///
/// See the module documentation for what it is for, why writes are silently
/// dropped, and why it implements ten families it does not advertise.
#[derive(Debug, Clone, Copy, Default)]
pub struct NullMemoryProvider;

impl NullMemoryProvider {
    /// Construct the null driver. It holds no state, so every instance is
    /// interchangeable.
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl MemoryProvider for NullMemoryProvider {
    fn driver_id(&self) -> &str {
        NULL_DRIVER_ID
    }

    /// Exactly the mandatory three. The optional families are implemented
    /// below but deliberately not advertised, so they stay unreachable through
    /// the trait object.
    fn capabilities(&self) -> Capabilities {
        Capabilities::mandatory()
    }

    /// Always [`MemoryHealth::Ready`]: a driver with no backing store has
    /// nothing that can be unreachable, and reporting `Degraded` would make
    /// every status view of a deliberately-disabled subsystem look broken.
    async fn health(&self) -> MemoryHealth {
        MemoryHealth::Ready
    }

    // The `as_*` accessors are all left at their `None` defaults: nothing
    // optional is reachable through the trait object. That absence is the whole
    // point of this driver, so overriding any of them would be the bug.
}

#[async_trait]
impl MemoryCore for NullMemoryProvider {
    /// Accepts and discards. See the module docs on `/dev/null` semantics.
    async fn store(
        &self,
        _namespace: &str,
        _key: &str,
        _content: &str,
        _category: MemoryCategory,
        _session_id: Option<&str>,
        _taint: MemoryTaint,
    ) -> Result<(), MemoryError> {
        Ok(())
    }

    async fn get(&self, _namespace: &str, _key: &str) -> Result<Option<MemoryEntry>, MemoryError> {
        Ok(None)
    }

    /// Always `Ok(false)`: nothing was ever stored, so nothing existed to
    /// forget. Consistent with the idempotence the family requires.
    async fn forget(&self, _namespace: &str, _key: &str) -> Result<bool, MemoryError> {
        Ok(false)
    }

    async fn list(
        &self,
        _namespace: Option<&str>,
        _category: Option<&MemoryCategory>,
        _session_id: Option<&str>,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        Ok(Vec::new())
    }

    async fn namespaces(&self) -> Result<Vec<NamespaceSummary>, MemoryError> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl MemoryRecall for NullMemoryProvider {
    async fn recall(
        &self,
        _query: &str,
        _limit: usize,
        _opts: &OwnedRecallOpts,
        _scope: Option<&SourceScope>,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl MemoryPortability for NullMemoryProvider {
    /// One empty, terminal page: no records and no continuation cursor, so a
    /// caller's export loop terminates on the first iteration.
    ///
    /// This driver never issues a cursor (every page is the first and only
    /// page), so any `Some(_)` cursor a caller passes back is necessarily one
    /// this driver did not hand out — reject it rather than silently treating
    /// it as a valid terminal page.
    async fn export_page(
        &self,
        cursor: Option<&str>,
        _limit: usize,
    ) -> Result<ExportPage, MemoryError> {
        if cursor.is_some() {
            return Err(MemoryError::Invalid(
                "null provider does not issue export cursors".into(),
            ));
        }

        Ok(ExportPage::default())
    }

    /// Counts every record as skipped rather than imported. Reporting them as
    /// imported would tell a migration its data landed somewhere it did not.
    async fn import_records(
        &self,
        records: Vec<ExportRecord>,
    ) -> Result<ImportOutcome, MemoryError> {
        Ok(ImportOutcome {
            imported: 0,
            skipped: u32::try_from(records.len()).unwrap_or(u32::MAX),
            failed: 0,
            errors: Vec::new(),
        })
    }
}

#[async_trait]
impl MemoryIngest for NullMemoryProvider {
    async fn ingest_document(&self, _item: IngestItem) -> Result<IngestOutcome, MemoryError> {
        unsupported(Capability::Ingest)
    }

    async fn ingest_chat(&self, _messages: Vec<IngestItem>) -> Result<IngestOutcome, MemoryError> {
        unsupported(Capability::Ingest)
    }

    // Spelled out rather than left to the trait's default. The default exists
    // for drivers that predate the method; this one refuses everything on
    // purpose, and a family here that answered by inheritance would stop
    // refusing the day the default changes.
    async fn ingest_email(&self, _messages: Vec<IngestItem>) -> Result<IngestOutcome, MemoryError> {
        unsupported(Capability::Ingest)
    }
}

#[async_trait]
impl MemoryDocumentIngest for NullMemoryProvider {
    async fn ingest_document(&self, _document: IngestItem) -> Result<IngestOutcome, MemoryError> {
        unsupported(Capability::DocumentIngest)
    }
}

#[async_trait]
impl MemoryConversationIngest for NullMemoryProvider {
    async fn ingest_conversation(
        &self,
        _messages: Vec<IngestItem>,
    ) -> Result<IngestOutcome, MemoryError> {
        unsupported(Capability::ConversationIngest)
    }
}

#[async_trait]
impl MemoryLearningIngest for NullMemoryProvider {
    async fn ingest_learning(
        &self,
        _learning: LearningCandidate,
    ) -> Result<IngestOutcome, MemoryError> {
        unsupported(Capability::LearningIngest)
    }
}

#[async_trait]
impl MemoryEventIngest for NullMemoryProvider {
    async fn ingest_event(&self, _event: RawMemoryEvent) -> Result<IngestOutcome, MemoryError> {
        unsupported(Capability::EventIngest)
    }
}

#[async_trait]
impl MemoryAnswer for NullMemoryProvider {
    async fn answer(&self, _request: AnswerRequest) -> Result<AnswerResponse, MemoryError> {
        unsupported(Capability::Answer)
    }
}

#[async_trait]
impl MemoryDocuments for NullMemoryProvider {
    async fn put_document(&self, _input: NamespaceDocumentInput) -> Result<String, MemoryError> {
        unsupported(Capability::Documents)
    }

    async fn get_document(
        &self,
        _namespace: &str,
        _key: &str,
    ) -> Result<Option<StoredMemoryDocument>, MemoryError> {
        unsupported(Capability::Documents)
    }

    async fn list_documents(
        &self,
        _namespace: Option<&str>,
    ) -> Result<serde_json::Value, MemoryError> {
        unsupported(Capability::Documents)
    }

    async fn list_namespaces(&self) -> Result<Vec<String>, MemoryError> {
        unsupported(Capability::Documents)
    }

    async fn delete_document(
        &self,
        _namespace: &str,
        _document_id: &str,
    ) -> Result<serde_json::Value, MemoryError> {
        unsupported(Capability::Documents)
    }

    async fn clear_namespace(&self, _namespace: &str) -> Result<(), MemoryError> {
        unsupported(Capability::Documents)
    }

    async fn query_documents(
        &self,
        _namespace: &str,
        _query: &str,
        _limit: usize,
    ) -> Result<NamespaceRetrievalContext, MemoryError> {
        unsupported(Capability::Documents)
    }

    async fn recall_documents(
        &self,
        _namespace: &str,
        _limit: usize,
    ) -> Result<NamespaceRetrievalContext, MemoryError> {
        unsupported(Capability::Documents)
    }
}

#[async_trait]
impl MemoryTree for NullMemoryProvider {
    async fn append(&self, _request: IngestRequest) -> Result<(), MemoryError> {
        unsupported(Capability::Tree)
    }

    async fn query_source(
        &self,
        _namespace: &str,
        _source_id: &str,
        _limit: usize,
        _scope: Option<&SourceScope>,
    ) -> Result<Vec<crate::chunks::Chunk>, MemoryError> {
        unsupported(Capability::Tree)
    }

    async fn drill_down(
        &self,
        _namespace: &str,
        _node_id: &str,
    ) -> Result<QueryResult, MemoryError> {
        unsupported(Capability::Tree)
    }

    async fn seal(&self, _namespace: &str) -> Result<TreeStatus, MemoryError> {
        unsupported(Capability::Tree)
    }

    async fn cascade(&self, _namespace: &str) -> Result<TreeStatus, MemoryError> {
        unsupported(Capability::Tree)
    }
}

#[async_trait]
impl MemoryEntities for NullMemoryProvider {
    async fn entities(
        &self,
        _namespace: &str,
        _query: Option<&str>,
        _limit: usize,
    ) -> Result<Vec<EntityHit>, MemoryError> {
        unsupported(Capability::Entities)
    }

    async fn entity_edges(
        &self,
        _namespace: &str,
        _entity_id: &str,
        _limit: usize,
    ) -> Result<Vec<GraphRelationRecord>, MemoryError> {
        unsupported(Capability::Entities)
    }

    async fn touch_entities(
        &self,
        _namespace: &str,
        _entity_ids: &[String],
    ) -> Result<(), MemoryError> {
        unsupported(Capability::Entities)
    }
}

#[async_trait]
impl MemoryGraph for NullMemoryProvider {
    async fn kv_get(
        &self,
        _namespace: Option<&str>,
        _key: &str,
    ) -> Result<Option<MemoryKvRecord>, MemoryError> {
        unsupported(Capability::Graph)
    }

    async fn kv_put(
        &self,
        _namespace: Option<&str>,
        _key: &str,
        _value: serde_json::Value,
    ) -> Result<(), MemoryError> {
        unsupported(Capability::Graph)
    }

    async fn kv_delete(&self, _namespace: Option<&str>, _key: &str) -> Result<bool, MemoryError> {
        unsupported(Capability::Graph)
    }

    async fn kv_list(
        &self,
        _namespace: Option<&str>,
        _prefix: Option<&str>,
        _limit: usize,
    ) -> Result<Vec<MemoryKvRecord>, MemoryError> {
        unsupported(Capability::Graph)
    }

    async fn relations(
        &self,
        _namespace: Option<&str>,
        _subject: Option<&str>,
        _predicate: Option<&str>,
        _limit: usize,
    ) -> Result<Vec<GraphRelationRecord>, MemoryError> {
        unsupported(Capability::Graph)
    }

    async fn put_relation(&self, _relation: GraphRelationRecord) -> Result<(), MemoryError> {
        unsupported(Capability::Graph)
    }
}

#[async_trait]
impl MemoryDiff for NullMemoryProvider {
    async fn capture_snapshot(&self, _source_id: &str) -> Result<SnapshotRef, MemoryError> {
        unsupported(Capability::Diff)
    }

    async fn snapshots(
        &self,
        _source_id: &str,
        _limit: usize,
    ) -> Result<Vec<SnapshotRef>, MemoryError> {
        unsupported(Capability::Diff)
    }

    async fn diff(
        &self,
        _source_id: &str,
        _from: Option<&str>,
        _to: &str,
    ) -> Result<DiffReport, MemoryError> {
        unsupported(Capability::Diff)
    }
}

#[async_trait]
impl MemoryGoals for NullMemoryProvider {
    async fn goals(&self) -> Result<GoalsDoc, MemoryError> {
        unsupported(Capability::Goals)
    }

    async fn set_goals(&self, _goals: GoalsDoc) -> Result<(), MemoryError> {
        unsupported(Capability::Goals)
    }
}

#[async_trait]
impl MemoryToolMemory for NullMemoryProvider {
    async fn tool_rules(&self, _tool_name: &str) -> Result<Vec<ToolMemoryRule>, MemoryError> {
        unsupported(Capability::ToolMemory)
    }

    async fn put_tool_rule(&self, _rule: ToolMemoryRule) -> Result<(), MemoryError> {
        unsupported(Capability::ToolMemory)
    }

    async fn delete_tool_rule(
        &self,
        _tool_name: &str,
        _rule_id: &str,
    ) -> Result<bool, MemoryError> {
        unsupported(Capability::ToolMemory)
    }
}

#[async_trait]
impl MemorySourceSink for NullMemoryProvider {
    async fn accept_source_items(
        &self,
        _source_id: &str,
        _source_kind: &str,
        _items: Vec<SourceItem>,
        _taint: MemoryTaint,
    ) -> Result<IngestOutcome, MemoryError> {
        unsupported(Capability::Sources)
    }

    async fn forget_source(&self, _source_id: &str) -> Result<u64, MemoryError> {
        unsupported(Capability::Sources)
    }
}

#[async_trait]
impl MemoryMaintenance for NullMemoryProvider {
    async fn reembed(&self) -> Result<MaintenanceReport, MemoryError> {
        unsupported(Capability::Maintenance)
    }

    async fn compact(&self) -> Result<MaintenanceReport, MemoryError> {
        unsupported(Capability::Maintenance)
    }

    async fn consolidate(&self) -> Result<MaintenanceReport, MemoryError> {
        unsupported(Capability::Maintenance)
    }

    async fn doctor(&self) -> Result<MaintenanceReport, MemoryError> {
        unsupported(Capability::Maintenance)
    }

    // The trait defaults these to an empty outcome, which is the right answer
    // for a real driver that simply has nothing buffered or nothing derived.
    // It is the wrong answer here: both *mutate*, and this provider stores
    // nothing, so "flushed nothing" and "reset nothing" would read as work
    // done rather than as a driver that cannot do it.
    async fn flush_pending(&self) -> Result<FlushOutcome, MemoryError> {
        unsupported(Capability::Maintenance)
    }

    async fn reset_derived_index(&self) -> Result<ResetOutcome, MemoryError> {
        unsupported(Capability::Maintenance)
    }
}

#[async_trait]
impl MemoryPeople for NullMemoryProvider {
    async fn list_people(&self, _limit: Option<usize>) -> Result<Vec<RankedPerson>, MemoryError> {
        unsupported(Capability::People)
    }

    async fn get_person(&self, _person_id: &str) -> Result<Option<PersonRecord>, MemoryError> {
        unsupported(Capability::People)
    }

    async fn resolve_handle(
        &self,
        _handle: &PersonHandle,
        _create_if_missing: bool,
    ) -> Result<Option<ResolvedPerson>, MemoryError> {
        unsupported(Capability::People)
    }

    async fn add_handle_alias(
        &self,
        _person_id: &str,
        _handle: &PersonHandle,
    ) -> Result<(), MemoryError> {
        unsupported(Capability::People)
    }

    async fn score_person(&self, _person_id: &str) -> Result<Option<PersonScore>, MemoryError> {
        unsupported(Capability::People)
    }

    async fn record_interaction(
        &self,
        _interaction: &PersonInteraction,
    ) -> Result<(), MemoryError> {
        unsupported(Capability::People)
    }

    async fn seed_from_address_book(&self) -> Result<AddressBookSeedOutcome, MemoryError> {
        unsupported(Capability::People)
    }
}

#[async_trait]
impl MemoryChunks for NullMemoryProvider {
    async fn list_chunks(
        &self,
        _query: &ChunkQuery,
        _scope: Option<&SourceScope>,
    ) -> Result<Vec<crate::chunks::Chunk>, MemoryError> {
        unsupported(Capability::Chunks)
    }

    async fn get_chunk(
        &self,
        _chunk_id: &str,
    ) -> Result<Option<crate::chunks::Chunk>, MemoryError> {
        unsupported(Capability::Chunks)
    }

    async fn chunk_detail(&self, _chunk_id: &str) -> Result<Option<ChunkDetail>, MemoryError> {
        unsupported(Capability::Chunks)
    }

    async fn storage_kinds(&self) -> Result<Vec<String>, MemoryError> {
        unsupported(Capability::Chunks)
    }

    async fn chunk_embeddings(
        &self,
        _chunk_ids: &[String],
        _model_signature: &str,
    ) -> Result<Vec<ChunkEmbedding>, MemoryError> {
        unsupported(Capability::Chunks)
    }
}

#[async_trait]
impl MemoryRetrieval for NullMemoryProvider {
    async fn fast_retrieve(
        &self,
        _query: &str,
        _options: FastRetrieveQuery,
        _scope: Option<&SourceScope>,
    ) -> Result<RetrievalResponse, MemoryError> {
        unsupported(Capability::Retrieval)
    }

    async fn cover_window(
        &self,
        _window: &CoverWindowQuery,
        _scope: Option<&SourceScope>,
    ) -> Result<RetrievalResponse, MemoryError> {
        unsupported(Capability::Retrieval)
    }

    async fn retrieve_source(
        &self,
        _query: &SourceRetrievalQuery,
        _scope: Option<&SourceScope>,
    ) -> Result<RetrievalResponse, MemoryError> {
        unsupported(Capability::Retrieval)
    }

    async fn retrieve_children(
        &self,
        _node_id: &str,
        _max_depth: u32,
        _query: Option<&str>,
        _limit: Option<usize>,
        _scope: Option<&SourceScope>,
    ) -> Result<Vec<RetrievalHit>, MemoryError> {
        unsupported(Capability::Retrieval)
    }

    async fn retrieve_leaves(
        &self,
        _chunk_ids: &[String],
        _scope: Option<&SourceScope>,
    ) -> Result<Vec<RetrievalHit>, MemoryError> {
        unsupported(Capability::Retrieval)
    }

    async fn recall_namespace_scored(
        &self,
        _namespace: &str,
        _query: &str,
        _limit: usize,
        _exclude_session_id: Option<&str>,
    ) -> Result<Vec<NamespaceMemoryHit>, MemoryError> {
        unsupported(Capability::Retrieval)
    }

    async fn recall_namespace_recent(
        &self,
        _namespace: &str,
        _limit: usize,
    ) -> Result<Vec<NamespaceMemoryHit>, MemoryError> {
        unsupported(Capability::Retrieval)
    }

    async fn search_entities(
        &self,
        _query: &str,
        _kinds: Option<&[String]>,
        _limit: usize,
    ) -> Result<Vec<EntityMatch>, MemoryError> {
        unsupported(Capability::Retrieval)
    }
}

#[async_trait]
impl MemoryProfile for NullMemoryProvider {
    async fn list_active_facets(&self) -> Result<Vec<ProfileFacet>, MemoryError> {
        unsupported(Capability::Profile)
    }
    async fn list_all_facets(&self) -> Result<Vec<ProfileFacet>, MemoryError> {
        unsupported(Capability::Profile)
    }
    async fn get_facet(&self, _key: &str) -> Result<Option<ProfileFacet>, MemoryError> {
        unsupported(Capability::Profile)
    }
    async fn facets_by_type(
        &self,
        _facet_type: FacetType,
    ) -> Result<Vec<ProfileFacet>, MemoryError> {
        unsupported(Capability::Profile)
    }
    async fn upsert_facet(&self, _facet: &ProfileFacet) -> Result<(), MemoryError> {
        unsupported(Capability::Profile)
    }
    async fn upsert_provider_facet(
        &self,
        _facet_id: &str,
        _facet_type: FacetType,
        _key: &str,
        _value: &str,
        _confidence: f64,
        _segment_id: Option<&str>,
        _observed_at: f64,
    ) -> Result<(), MemoryError> {
        unsupported(Capability::Profile)
    }
    async fn set_facet_user_state(
        &self,
        _key: &str,
        _user_state: UserState,
    ) -> Result<bool, MemoryError> {
        unsupported(Capability::Profile)
    }
    async fn delete_facet(&self, _key: &str) -> Result<bool, MemoryError> {
        unsupported(Capability::Profile)
    }
    async fn delete_facet_by_id(&self, _facet_id: &str) -> Result<bool, MemoryError> {
        unsupported(Capability::Profile)
    }
    async fn drop_facets_below(&self, _threshold: f64) -> Result<usize, MemoryError> {
        unsupported(Capability::Profile)
    }
    /// `false`, matching the trait's documented "an error reads as no".
    async fn workflow_identity_matches(&self, _pattern: &str, _value: &str) -> bool {
        false
    }
}

#[async_trait]
impl MemorySourceSync for NullMemoryProvider {
    async fn run_connection_sync(
        &self,
        _toolkit: &str,
        _connection_id: &str,
    ) -> Result<SyncRunOutcome, MemoryError> {
        unsupported(Capability::SourceSync)
    }

    async fn source_sync_state(
        &self,
        _toolkit: &str,
        _connection_id: &str,
    ) -> Result<Option<SourceSyncState>, MemoryError> {
        // Not `Ok(None)`, which the trait defines as "this connection has never
        // synced". This driver cannot sync at all, and answering "never synced"
        // would put a connection with a plausible empty state in front of a
        // caller that would then offer to sync it.
        unsupported(Capability::SourceSync)
    }

    async fn sync_audit_log(
        &self,
        _limit: Option<usize>,
    ) -> Result<Vec<SyncAuditEntry>, MemoryError> {
        unsupported(Capability::SourceSync)
    }

    async fn estimate_sync_cost_usd(
        &self,
        _input_tokens: u64,
        _output_tokens: u64,
    ) -> Result<f64, MemoryError> {
        // The trait lets a driver whose sync is free answer `0.0`. This one has
        // no sync to price, and quoting a free one would be a price rather than
        // an absence — the same distinction the state read above draws.
        unsupported(Capability::SourceSync)
    }

    async fn sync_statuses(&self) -> Result<Vec<SourceSyncStatus>, MemoryError> {
        unsupported(Capability::SourceSync)
    }

    async fn raw_archive_coverage(
        &self,
        _tree_scope: &str,
        _archive_source_id: &str,
    ) -> Result<RawArchiveCoverage, MemoryError> {
        unsupported(Capability::SourceSync)
    }

    async fn rebuild_from_raw_archive(
        &self,
        _tree_scope: &str,
        _archive_source_id: &str,
    ) -> Result<RawRebuildOutcome, MemoryError> {
        unsupported(Capability::SourceSync)
    }
}

#[async_trait]
impl MemoryCodingSessions for NullMemoryProvider {
    async fn coding_session_status(&self) -> Result<Vec<CodingSessionSource>, MemoryError> {
        // Not an empty list. The trait defines one row per agent the driver
        // knows about, so an empty answer is "I looked and found no agents
        // installed" — which this driver did not do.
        unsupported(Capability::CodingSessions)
    }

    async fn ingest_coding_sessions(
        &self,
        _request: CodingSessionIngestRequest,
    ) -> Result<CodingSessionIngestReport, MemoryError> {
        unsupported(Capability::CodingSessions)
    }
}

#[async_trait]
impl MemoryScoring for NullMemoryProvider {
    async fn extract_entities(&self, _query: &str) -> Result<Vec<String>, MemoryError> {
        unsupported(Capability::Scoring)
    }

    async fn embed_text(&self, _text: &str) -> Result<Vec<f32>, MemoryError> {
        unsupported(Capability::Scoring)
    }

    async fn embedder_slug(&self) -> Result<String, MemoryError> {
        unsupported(Capability::Scoring)
    }
}

#[cfg(test)]
#[path = "null_tests.rs"]
mod tests;
