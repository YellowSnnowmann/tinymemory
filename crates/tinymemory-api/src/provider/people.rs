//! The people family: contacts, handle resolution, and closeness scoring.
//!
//! A driver advertising [`Capability::People`](crate::capabilities::Capability::People)
//! owns a store of people, the aliases each is known by, and the interactions
//! observed with them — and can rank them by how close the user is to each.
//!
//! # Why this is a family and not a widening of an existing one
//!
//! People is storage the engine owns, and it does not fit any family already
//! defined: a person is not a memory entry, not a document, and not a graph
//! entity. Adding these methods to, say, [`MemoryEntities`] would also have
//! been a **major** contract bump — the version rule treats a new method on a
//! family a driver may already advertise as breaking, because negotiation
//! cannot save a caller from a method an older driver does not implement. A new
//! family is a minor bump instead, and an older driver simply does not
//! advertise it.
//!
//! [`MemoryEntities`]: crate::provider::MemoryEntities
//!
//! # The types here are the contract's own
//!
//! None of these name an engine type. TinyCortex has its own `Person`,
//! `Handle` and `Interaction`; a second engine will have others. The adapter at
//! each engine's edge converts, which is what keeps this contract
//! engine-neutral — see the module rules in
//! [`super`].
//!
//! # Identity crosses as a string
//!
//! [`PersonRef`] is an opaque string rather than a `Uuid`. The contract does
//! not promise that every engine identifies people by UUID, and a caller must
//! not parse one out — it round-trips an id it was given and nothing more.

use async_trait::async_trait;

use crate::error::MemoryError;

// The value types this family exchanges. They are defined in `tinymemory-bus`
// — they cross the module boundary, and a host that only makes calls must be
// able to name them without compiling this trait — and re-exported here so
// every historical path keeps resolving and the types stay the same types.
pub use tinymemory_bus::provider::people::{AddressBookSeedOutcome, PersonHandle, PersonInteraction, PersonRecord, PersonRef, PersonScore, RankedPerson, ResolvedPerson};

/// Contacts, handle resolution, and closeness scoring.
///
/// Reached through
/// [`MemoryProvider::as_people`](super::MemoryProvider::as_people); a driver
/// that does not advertise [`Capability::People`](crate::capabilities::Capability::People)
/// returns `None` there and none of this is callable.
#[async_trait]
pub trait MemoryPeople: Send + Sync {
    /// Known people, ranked by closeness, highest first.
    ///
    /// `limit` caps the result; `None` means the driver's own default. A driver
    /// must bound this even when asked for everything — an unbounded people
    /// list crosses the same 16 MiB frame as everything else.
    ///
    /// # Errors
    ///
    /// Backend failures only. An empty store yields an empty vector.
    async fn list_people(&self, limit: Option<usize>) -> Result<Vec<RankedPerson>, MemoryError>;

    /// One person by id.
    ///
    /// # Errors
    ///
    /// Backend failures only. An unknown id yields `Ok(None)` rather than
    /// [`MemoryError::NotFound`] — asking about someone who is not in the store
    /// is a normal question with a negative answer, not a failure.
    async fn get_person(&self, person_id: &str) -> Result<Option<PersonRecord>, MemoryError>;

    /// Resolve a handle to a person, optionally minting one.
    ///
    /// With `create_if_missing` false an unknown handle yields `Ok(None)`. With
    /// it true the driver mints a person and reports
    /// [`ResolvedPerson::created`].
    ///
    /// # Errors
    ///
    /// Backend failures only.
    async fn resolve_handle(
        &self,
        handle: &PersonHandle,
        create_if_missing: bool,
    ) -> Result<Option<ResolvedPerson>, MemoryError>;

    /// Record that a person is also known by `handle`.
    ///
    /// Idempotent: adding an alias a person already has is a no-op, not an
    /// error, because an importer replaying the same source must converge.
    ///
    /// # Errors
    ///
    /// [`MemoryError::NotFound`] when `person_id` is unknown — unlike a lookup,
    /// this is a write against an identity the caller claimed exists. Backend
    /// failures otherwise.
    async fn add_handle_alias(
        &self,
        person_id: &str,
        handle: &PersonHandle,
    ) -> Result<(), MemoryError>;

    /// The closeness score for one person.
    ///
    /// # Errors
    ///
    /// Backend failures only. An unknown id yields `Ok(None)`.
    async fn score_person(&self, person_id: &str) -> Result<Option<PersonScore>, MemoryError>;

    /// Record one observed interaction.
    ///
    /// # Errors
    ///
    /// [`MemoryError::NotFound`] when the person is unknown; backend failures
    /// otherwise.
    async fn record_interaction(&self, interaction: &PersonInteraction) -> Result<(), MemoryError>;

    /// Seed people from the host platform's address book, when it has one.
    ///
    /// A host with no address book — or without the permission to read it —
    /// reports `seeded: 0` rather than failing, so a caller cannot distinguish
    /// "nothing to import" from "not available here". That is deliberate: both
    /// mean the same thing to the caller, and the alternative leaks a platform
    /// detail into the contract.
    ///
    /// # Errors
    ///
    /// Backend failures only.
    async fn seed_from_address_book(&self) -> Result<AddressBookSeedOutcome, MemoryError>;
}
