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
use serde::{Deserialize, Serialize};

use crate::error::MemoryError;

/// Opaque identity of one person, as the driver issued it.
///
/// Treat as a token: round-trip it, compare it for equality, never parse it.
pub type PersonRef = String;

/// One way a person is addressed.
///
/// The driver is responsible for canonicalising these before storing or
/// looking up — case folding an email, trimming a handle, collapsing whitespace
/// in a display name. Two handles that canonicalise alike must resolve to the
/// same person, which is why callers pass the raw form and never a
/// pre-normalised one: normalisation that differed between caller and driver
/// would silently mint duplicate people.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum PersonHandle {
    /// An iMessage handle — a phone number or an Apple ID.
    IMessage(String),
    /// An email address.
    Email(String),
    /// A human-readable display name.
    DisplayName(String),
}

/// One person as the driver holds them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonRecord {
    /// Driver-issued identity.
    pub id: PersonRef,
    /// Best-known display name, when one is known.
    #[serde(default)]
    pub display_name: Option<String>,
    /// Primary email, when one is known.
    #[serde(default)]
    pub primary_email: Option<String>,
    /// Primary phone number, when one is known.
    #[serde(default)]
    pub primary_phone: Option<String>,
    /// Every handle this person is known by, canonicalised.
    #[serde(default)]
    pub handles: Vec<PersonHandle>,
    /// Creation time, RFC 3339.
    pub created_at: String,
    /// Last-update time, RFC 3339.
    pub updated_at: String,
}

/// Per-component breakdown of a closeness score, each in `[0, 1]`.
///
/// Exposed rather than collapsed to one number so a caller can explain a
/// ranking. The components are **not** comparable across drivers: each engine
/// picks its own half-life and depth proxy, so compare within one driver's
/// results only.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PersonScore {
    /// How recently the person was interacted with.
    pub recency: f32,
    /// How often.
    pub frequency: f32,
    /// How two-sided the exchange is — one-sided contact scores zero.
    pub reciprocity: f32,
    /// How substantial each interaction is.
    pub depth: f32,
    /// The composite, clamped to `[0, 1]`.
    pub score: f32,
}

/// A person together with their score, as returned by a ranked list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RankedPerson {
    /// The person.
    pub person: PersonRecord,
    /// Their closeness score.
    pub score: PersonScore,
}

/// The outcome of resolving a handle.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedPerson {
    /// Who the handle resolved to.
    pub id: PersonRef,
    /// Whether this call minted the person rather than finding them.
    ///
    /// Distinguished so a caller can tell "I now know who this is" from "I have
    /// just invented someone", which read identically from the id alone.
    pub created: bool,
}

/// One observed interaction, as reported by the host.
///
/// The host owns the channels, so it observes these; the driver only stores and
/// aggregates them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonInteraction {
    /// Who the interaction was with.
    pub person_id: PersonRef,
    /// When it happened, RFC 3339.
    pub at: String,
    /// `true` when the user sent it. This is what drives reciprocity, so an
    /// importer that cannot tell direction should not guess.
    pub is_outbound: bool,
    /// A proxy for substance — token or character count. Clamped during
    /// scoring, so an outlier cannot dominate a ranking.
    pub length: u32,
}

/// What an address-book seed actually did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddressBookSeedOutcome {
    /// People created or updated from the address book.
    pub seeded: usize,
    /// Contacts skipped — no usable handle, or a write that failed.
    pub skipped: usize,
}

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
    async fn record_interaction(
        &self,
        interaction: &PersonInteraction,
    ) -> Result<(), MemoryError>;

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
