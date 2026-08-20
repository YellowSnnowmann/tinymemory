//! Every value type that crosses the bus, re-exported from the contract crate.
//!
//! # These are re-exports, deliberately, and not definitions
//!
//! The obvious reading of "a crate that holds the bus types" is a crate that
//! *defines* them. That would be wrong here, and the repository already
//! documents why in the root manifest: when `tinymemory-api` was resolved
//! twice, `MemoryCategory` from one copy was not the same type as
//! `MemoryCategory` from the other, and the mismatch only showed up at the
//! seam. Defining a second set of structurally identical types here would
//! reproduce that on purpose: the module would serve `tinymemory_api::`
//! types, the host would hold `tinymemory_bus::` ones, and every call site
//! would need a conversion whose correctness nothing checks.
//!
//! So one definition, in `tinymemory-api`, surfaced here. A host that depends
//! on this crate gets exactly the types the module serves — the same types,
//! not equivalents.
//!
//! # Why the host does not just depend on `tinymemory-api`
//!
//! It could, and it would compile. But `tinymemory-api` is the *driver*
//! contract: it also carries `MemoryProvider` and its capability traits, the
//! mandatory-family composition, the null driver, and the `host::` config
//! sections. A host that loads the module implements none of those — it makes
//! calls. This crate is the subset that crosses a frame, so what a host
//! compiles against is what it can actually send and receive.
//!
//! The grouping below mirrors the capability families in [`crate::calls`].

pub use tinymemory_api::capabilities::{Capabilities, Capability};
pub use tinymemory_api::chunks::{Chunk, Metadata, SourceRef};
pub use tinymemory_api::error::{MemoryError};
pub use tinymemory_api::goals::{GoalItem, GoalsDoc};
pub use tinymemory_api::health::{MemoryHealth};
pub use tinymemory_api::provider::chunks::{ChunkDetail, ChunkEmbedding, ChunkQuery};
pub use tinymemory_api::provider::episodic::{ConversationSegment, EpisodicTurn};
pub use tinymemory_api::provider::people::{AddressBookSeedOutcome, PersonHandle, PersonInteraction, PersonRecord, PersonScore, RankedPerson, ResolvedPerson};
pub use tinymemory_api::provider::profile::{FacetType, ProfileFacet, UserState};
pub use tinymemory_api::provider::retrieval::{CoverWindowQuery, EntityMatch, FastRetrieveQuery, RetrievalHit, RetrievalResponse, SourceRetrievalQuery};
pub use tinymemory_api::provider::types::{DiffReport, EntityHit, ExportPage, ExportRecord, ImportOutcome, IngestItem, IngestOutcome, MaintenanceReport, SnapshotRef, SourceItem, SourceScope};
pub use tinymemory_api::recall::{OwnedRecallOpts};
pub use tinymemory_api::tool_memory::{ToolMemoryRule};
pub use tinymemory_api::tree::{IngestRequest, QueryResult, TreeStatus};
pub use tinymemory_api::types::{GraphRelationRecord, MemoryCategory, MemoryEntry, MemoryKvRecord, MemoryTaint, NamespaceDocumentInput, NamespaceMemoryHit, NamespaceRetrievalContext, NamespaceSummary, StoredMemoryDocument};

/// `serde_json::Value`, which three document methods return verbatim.
///
/// `ListDocuments` and `DeleteDocument` answer with a driver-shaped JSON
/// document rather than a typed record, so a host has to hold the untyped
/// value. Re-exported here so it arrives from the same place as everything
/// else on the wire and a host does not have to match `serde_json` versions
/// by hand.
pub use serde_json::Value as JsonValue;
