//! The process-global [`MemoryEventSink`], and the `publish` the extracted code
//! calls in place of the host's bus.
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
//! build, during early startup — [`publish`] drops the event. An event bus that
//! panicked or errored when unwired would turn every emit site into an error
//! path, and none of the call sites have anything useful to do with that error:
//! the work they are reporting on has already happened.

use std::sync::{Arc, RwLock};

pub use crate::host::{
    EmbeddingHealthReason, MemoryEvent, MemoryEventSink, NoopEventSink,
};

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

/// A [`MemoryEventSink`] that records what it was given, for tests.
///
/// Several tests used to assert on the host's web-channel broadcast, because
/// before the extraction the publish went straight onto that channel. The
/// decision to publish is core behaviour; the wire format is the host's. So the
/// tests kept the half they are actually about — *did the transition publish,
/// and did it publish exactly once* — and assert it here instead.
#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct RecordingSink {
    events: std::sync::Mutex<Vec<MemoryEvent>>,
}

#[cfg(test)]
impl RecordingSink {
    /// Install a fresh recorder and return it. Replaces any existing sink.
    pub(crate) fn install() -> Arc<Self> {
        let sink = Arc::new(Self::default());
        set_event_sink(Arc::clone(&sink) as Arc<dyn MemoryEventSink>);
        sink
    }

    /// Take everything recorded so far, leaving the recorder empty.
    pub(crate) fn drain(&self) -> Vec<MemoryEvent> {
        std::mem::take(&mut *self.events.lock().expect("recording sink lock"))
    }
}

#[cfg(test)]
impl MemoryEventSink for RecordingSink {
    fn publish(&self, event: MemoryEvent) {
        self.events.lock().expect("recording sink lock").push(event);
    }
}
