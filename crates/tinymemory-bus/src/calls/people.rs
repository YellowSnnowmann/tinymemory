//! The people store: ranking, handles, scores and interactions.
//!
//! One [`BusCall`] per member; see [`crate::calls`] for how they are used.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use tinymemory_api::provider::people::{AddressBookSeedOutcome, PersonHandle, PersonInteraction, PersonRecord, PersonScore, RankedPerson, ResolvedPerson};

use crate::calls::BusCall;
use crate::error::Error;
use crate::names::methods;

/// Arguments for `ListPeople`.
///
/// Known people, ranked by closeness.
///
/// Size-checked like the other list-returning methods. `limit` bounds the
/// *count* but not the bytes — a store of people each carrying many handles
/// can still overflow a frame — so the ceiling is enforced on the encoded
/// response rather than trusted to the caller's limit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPeople {
    /// The `limit` argument — wire position 0.
    pub limit: Option<usize>,
}

impl BusCall for ListPeople {
    const METHOD: &'static str = methods::LIST_PEOPLE;

    type Response = Vec<RankedPerson>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.limit,)).map_err(Error::Encode)
    }
}

/// Arguments for `GetPerson`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPerson {
    /// The `person_id` argument — wire position 0.
    pub person_id: String,
}

impl BusCall for GetPerson {
    const METHOD: &'static str = methods::GET_PERSON;

    type Response = Option<PersonRecord>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.person_id,)).map_err(Error::Encode)
    }
}

/// Arguments for `ResolveHandle`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveHandle {
    /// The `handle` argument — wire position 0.
    pub handle: PersonHandle,
    /// The `create_if_missing` argument — wire position 1.
    pub create_if_missing: bool,
}

impl BusCall for ResolveHandle {
    const METHOD: &'static str = methods::RESOLVE_HANDLE;

    type Response = Option<ResolvedPerson>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.handle, self.create_if_missing)).map_err(Error::Encode)
    }
}

/// Arguments for `AddHandleAlias`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddHandleAlias {
    /// The `person_id` argument — wire position 0.
    pub person_id: String,
    /// The `handle` argument — wire position 1.
    pub handle: PersonHandle,
}

impl BusCall for AddHandleAlias {
    const METHOD: &'static str = methods::ADD_HANDLE_ALIAS;

    type Response = ();

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.person_id, self.handle)).map_err(Error::Encode)
    }
}

/// Arguments for `ScorePerson`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScorePerson {
    /// The `person_id` argument — wire position 0.
    pub person_id: String,
}

impl BusCall for ScorePerson {
    const METHOD: &'static str = methods::SCORE_PERSON;

    type Response = Option<PersonScore>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.person_id,)).map_err(Error::Encode)
    }
}

/// Arguments for `RecordInteraction`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordInteraction {
    /// The `interaction` argument — wire position 0.
    pub interaction: PersonInteraction,
}

impl BusCall for RecordInteraction {
    const METHOD: &'static str = methods::RECORD_INTERACTION;

    type Response = ();

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.interaction,)).map_err(Error::Encode)
    }
}

/// Arguments for `SeedFromAddressBook`.
///
/// Takes no arguments, so it encodes as an empty positional array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedFromAddressBook;

impl BusCall for SeedFromAddressBook {
    const METHOD: &'static str = methods::SEED_FROM_ADDRESS_BOOK;

    type Response = AddressBookSeedOutcome;

    fn into_args(self) -> crate::Result<Value> {
        Ok(Value::Array(Vec::new()))
    }
}
