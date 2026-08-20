//! Driver identity, capability negotiation, health and store opening.
//!
//! One [`BusCall`] per member; see [`crate::calls`] for how they are used.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use tinymemory_api::capabilities::Capabilities;
use tinymemory_api::health::MemoryHealth;

use crate::calls::BusCall;
use crate::error::Error;
use crate::names::methods;

/// Arguments for `DriverId`.
///
/// Takes no arguments, so it encodes as an empty positional array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverId;

impl BusCall for DriverId {
    const METHOD: &'static str = methods::DRIVER_ID;

    type Response = String;

    fn into_args(self) -> crate::Result<Value> {
        Ok(Value::Array(Vec::new()))
    }
}

/// Arguments for `Capabilities`.
///
/// Takes no arguments, so it encodes as an empty positional array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capabilities;

impl BusCall for Capabilities {
    const METHOD: &'static str = methods::CAPABILITIES;

    type Response = Capabilities;

    fn into_args(self) -> crate::Result<Value> {
        Ok(Value::Array(Vec::new()))
    }
}

/// Arguments for `Health`.
///
/// Current liveness, as the driver reports it.
///
/// Takes no arguments, so it encodes as an empty positional array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Health;

impl BusCall for Health {
    const METHOD: &'static str = methods::HEALTH;

    type Response = MemoryHealth;

    fn into_args(self) -> crate::Result<Value> {
        Ok(Value::Array(Vec::new()))
    }
}

/// Arguments for `Shutdown`.
///
/// Release backend resources.
///
/// Idempotent, as the trait requires. Note that this does **not** unload the
/// module: `TinyBus` never unloads a library, so a host that shuts the
/// driver down and rebinds gets a fresh engine inside the same mapped image.
///
/// Takes no arguments, so it encodes as an empty positional array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shutdown;

impl BusCall for Shutdown {
    const METHOD: &'static str = methods::SHUTDOWN;

    type Response = ();

    fn into_args(self) -> crate::Result<Value> {
        Ok(Value::Array(Vec::new()))
    }
}

/// Arguments for `OpenStore`.
///
/// Bring up a store rooted at `<workspace>/<memory_subdir>` and return the
/// object path serving it.
///
/// # Why the module opens stores rather than the host selecting one per call
///
/// A host with per-profile memory needs more than one store in a process.
/// The alternative was a store selector threaded through every method on
/// every capability family — a change to the shape of the whole contract,
/// to express something that is not a property of a memory operation at
/// all. Which store you are talking to is settled when you are handed a
/// driver, exactly like which workspace you are bound to.
///
/// So the root object opens stores and hands back object paths. Each is an
/// ordinary `MemoryService` exporting the identical interface, and the
/// contract does not change at all: `MemoryProvider` still describes one
/// store, and a proxy still talks to one store.
///
/// Idempotent per subtree — see `StoreOpener::served` for why opening the
/// same database twice is worth going out of the way to avoid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenStore {
    /// The `memory_subdir` argument — wire position 0.
    pub memory_subdir: String,
}

impl BusCall for OpenStore {
    const METHOD: &'static str = methods::OPEN_STORE;

    type Response = String;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.memory_subdir,)).map_err(Error::Encode)
    }
}
