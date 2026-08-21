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

use std::sync::Arc;

use parking_lot::RwLock;

pub use tinymemory_api::host::{
    EmbeddingHealthReason, MemoryEvent, MemoryEventSink, NoopEventSink,
};

static SINK: RwLock<Option<Arc<dyn MemoryEventSink>>> = RwLock::new(None);

/// Install the host's event sink. Called once during startup wiring, before any
/// memory work begins. Calling it again replaces the sink, which is what test
/// harnesses want between cases.
pub fn set_event_sink(sink: Arc<dyn MemoryEventSink>) {
    *SINK.write() = Some(sink);
}

/// Remove any installed sink, returning to silent-drop behaviour. For tests.
pub fn clear_event_sink() {
    *SINK.write() = None;
}

/// The installed sink, or `None` when no host has wired one up.
#[must_use]
pub fn event_sink() -> Option<Arc<dyn MemoryEventSink>> {
    SINK.read().clone()
}

/// Announce a memory-domain event to the host. A no-op when no sink is
/// installed.
pub fn publish(event: MemoryEvent) {
    let sink = SINK.read().clone();
    if let Some(sink) = sink {
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
    events: parking_lot::Mutex<Vec<MemoryEvent>>,
}

#[cfg(test)]
impl RecordingSink {
    /// Install a fresh recorder for the lifetime of the returned guard.
    ///
    /// All unit tests that replace the process-global sink share one lock.
    /// Dropping the guard restores the previous sink only when this recorder
    /// still owns the slot, so cleanup cannot clobber a newer host install.
    pub(crate) fn install() -> RecordingSinkGuard {
        static TEST_SINK_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let lock = TEST_SINK_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = event_sink();
        let sink = Arc::new(Self::default());
        let installed = Arc::clone(&sink) as Arc<dyn MemoryEventSink>;
        set_event_sink(Arc::clone(&installed));
        RecordingSinkGuard {
            sink,
            installed,
            previous,
            _lock: lock,
        }
    }

    /// Take everything recorded so far, leaving the recorder empty.
    pub(crate) fn drain(&self) -> Vec<MemoryEvent> {
        std::mem::take(&mut *self.events.lock())
    }
}

/// Serialises unit tests that temporarily replace the global event sink.
#[cfg(test)]
pub(crate) struct RecordingSinkGuard {
    sink: Arc<RecordingSink>,
    installed: Arc<dyn MemoryEventSink>,
    previous: Option<Arc<dyn MemoryEventSink>>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl std::ops::Deref for RecordingSinkGuard {
    type Target = RecordingSink;

    fn deref(&self) -> &Self::Target {
        &self.sink
    }
}

#[cfg(test)]
impl Drop for RecordingSinkGuard {
    fn drop(&mut self) {
        let still_installed = event_sink()
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &self.installed));
        if !still_installed {
            return;
        }
        match self.previous.take() {
            Some(previous) => set_event_sink(previous),
            None => clear_event_sink(),
        }
    }
}

#[cfg(test)]
impl MemoryEventSink for RecordingSink {
    fn publish(&self, event: MemoryEvent) {
        self.events.lock().push(event);
    }
}
