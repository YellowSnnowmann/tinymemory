//! The process-global [`MemoryEventSink`], and
//! the [`publish()`] the engine calls in place of the host's own bus.
//!
//! # Why this is in the contract crate
//!
//! The event bus is **host policy, not engine substance** — `tinymemory-core`'s
//! own ownership note (`engine/mod.rs`) lists it on the *Product (host)* side of
//! the split. It lives here so a host that reaches memory only over the TinyBus
//! module can install a sink and read sync stages without linking the engine.
//! `tinymemory_core::events` re-exports it, so existing paths still resolve.
//!
//! # Why a global
//!
//! The publish sites are scattered across ingestion, the summary tree, the sync
//! pipelines and the store — deep inside call stacks that already thread a
//! config, a store handle and a cancellation token. Threading a fourth
//! parameter through all of them to reach a sink would be a large, mechanical,
//! reviewer-hostile diff for no gain, and it is exactly the shape the host's own
//! `BUS` static had before the extraction. This mirrors that shape rather than
//! inventing a new one.
//!
//! # Default is silence, not a panic
//!
//! Before a host installs a sink — in unit tests, in the standalone engine
//! build, during early startup — [`publish()`] drops the event. An event bus that
//! panicked or errored when unwired would turn every emit site into an error
//! path, and none of the call sites have anything useful to do with that error:
//! the work they are reporting on has already happened.

use std::sync::{Arc, RwLock};

pub use crate::host::{EmbeddingHealthReason, MemoryEvent, MemoryEventSink, NoopEventSink};

/// `std::sync::RwLock`, not the engine's `parking_lot` one. The lock is held
/// for a clone and nothing else, so the two behave identically here — and using
/// the `std` lock is what let the event bus move onto the host side of the
/// split without this crate taking on a new dependency, which its module docs
/// promise callers it will not do.
static SINK: RwLock<Option<Arc<dyn MemoryEventSink>>> = RwLock::new(None);

/// Read the sink, treating a poisoned lock as the sink it holds.
///
/// Poisoning means another thread panicked while holding the lock. The only
/// thing ever done under it is a `clone`, so there is no torn state to recover
/// from, and the module docs above are explicit that an unwired bus **drops**
/// the event rather than turning every emit site into an error path.
/// Propagating a panic out of `publish` would do exactly what they rule out.
fn sink_snapshot() -> Option<Arc<dyn MemoryEventSink>> {
    match SINK.read() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

/// Install the host's event sink. Called once during startup wiring, before any
/// memory work begins. Calling it again replaces the sink, which is what test
/// harnesses want between cases.
pub fn set_event_sink(sink: Arc<dyn MemoryEventSink>) {
    match SINK.write() {
        Ok(mut guard) => *guard = Some(sink),
        Err(poisoned) => *poisoned.into_inner() = Some(sink),
    }
}

/// Remove any installed sink, returning to silent-drop behaviour. For tests.
pub fn clear_event_sink() {
    match SINK.write() {
        Ok(mut guard) => *guard = None,
        Err(poisoned) => *poisoned.into_inner() = None,
    }
}

/// The installed sink, or `None` when no host has wired one up.
#[must_use]
pub fn event_sink() -> Option<Arc<dyn MemoryEventSink>> {
    sink_snapshot()
}

/// Announce a memory-domain event to the host. A no-op when no sink is
/// installed.
pub fn publish(event: MemoryEvent) {
    if let Some(sink) = sink_snapshot() {
        sink.publish(event);
    } else {
        log::trace!("[memory:events] dropped event with no sink installed: {event:?}");
    }
}

#[cfg(feature = "test-support")]
#[path = "events_test_support.rs"]
mod test_support;
#[cfg(feature = "test-support")]
pub use test_support::{RecordingSink, RecordingSinkGuard};
