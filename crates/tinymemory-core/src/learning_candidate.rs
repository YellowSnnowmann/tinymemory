//! Learning candidate buffer — Phase 1 of issue #566.
//!
//! The taxonomy ([`FacetClass`], [`CueFamily`], [`EvidenceRef`]) and the
//! unit-of-work [`LearningCandidate`] are defined in the contract crate; this
//! module re-exports them and owns the thread-safe ring-buffer [`Buffer`] that
//! collects candidates emitted by producers (Phase 2) before the stability
//! detector consumes them (Phase 3).
//!
//! The buffer is bounded: when full it evicts the oldest entry (FIFO overflow).
//! A global singleton is exposed via [`global()`]; individual tests may
//! construct their own [`Buffer`] with `Buffer::new(capacity)`.
//!
//! # Why the types moved out and the buffer did not (#5560)
//!
//! The types moved to [`tinymemory_api::learning`] because a *host* names them:
//! the stability detector, the facet cache and the reflection hooks all live in
//! OpenHuman, and reaching them through this crate is one of the compile-time
//! links #5560 removes. They are inert serde data, so the contract crate is the
//! right floor for them — same argument, and the same destination, as
//! [`EvidenceRef`], which went there first.
//!
//! The buffer stayed because a **`static` is not a payload**. This crate is
//! compiled into the module `cdylib`; the contract crate is compiled into both
//! that and the host binary. Moving [`global()`] down would not give the two
//! sides one queue, it would give them two, and the producer would push into
//! the copy the consumer never drains.
//!
//! **That split is already live, and moving the types does not close it.** The
//! one producer in this workspace is
//! `crate::sync::composio::providers::profile`, which pushes an identity
//! candidate on every provider-profile sync — and that code runs inside the
//! module. The host's detector drains the host's buffer. Delivering a candidate
//! across that boundary needs a bus member (or an event), which is contract
//! work rather than a re-export, and is called out in the upstream gap notes
//! rather than papered over here.

use std::collections::VecDeque;
use std::sync::OnceLock;

use parking_lot::Mutex;

/// The learning-candidate taxonomy, defined in the contract crate.
///
/// Re-exported at this path because ~30 call sites in this crate and in
/// OpenHuman already spell it `learning_candidate::FacetClass`, and the move
/// delivers the decoupling without spending that churn.
pub use tinymemory_api::learning::{CueFamily, FacetClass, LearningCandidate};

// ── Evidence reference ──────────────────────────────────────────

/// Where a candidate's evidence points. Defined in the contract crate — the
/// memory store persists it, so both sides must name one type. See
/// [`tinymemory_api::host::EvidenceRef`].
pub use tinymemory_api::host::EvidenceRef;

// ── Buffer ───────────────────────────────────────────────────────────────────

/// Thread-safe, bounded ring-buffer of [`LearningCandidate`] items.
///
/// Backed by a `parking_lot::Mutex<VecDeque<LearningCandidate>>`. When full
/// the oldest entry is evicted to make room (FIFO overflow). This keeps
/// memory bounded and naturally prioritises recent evidence.
///
/// The global singleton has a default capacity of 1024. Tests should
/// construct their own buffer via [`Buffer::new`].
pub struct Buffer {
    inner: Mutex<VecDeque<LearningCandidate>>,
    capacity: usize,
}

impl Buffer {
    /// Create a new buffer with the given capacity.
    ///
    /// `capacity` must be ≥ 1. A capacity of zero would make every `push`
    /// a no-op; callers should use a non-zero value.
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.max(1);
        Self {
            inner: Mutex::new(VecDeque::with_capacity(cap)),
            capacity: cap,
        }
    }

    /// Push a candidate onto the buffer.
    ///
    /// If the buffer is already at capacity, the oldest entry is evicted first
    /// (FIFO overflow). This ensures the buffer always reflects the most recent
    /// evidence.
    pub fn push(&self, candidate: LearningCandidate) {
        let mut guard = self.inner.lock();
        if guard.len() >= self.capacity {
            guard.pop_front(); // evict oldest
        }
        guard.push_back(candidate);
    }

    /// Drain all candidates from the buffer and return them in FIFO order.
    ///
    /// After this call the buffer is empty.
    pub fn drain(&self) -> Vec<LearningCandidate> {
        let mut guard = self.inner.lock();
        guard.drain(..).collect()
    }

    /// Clone all candidates without removing them.
    ///
    /// Useful for inspection or debugging.
    pub fn peek(&self) -> Vec<LearningCandidate> {
        let guard = self.inner.lock();
        guard.iter().cloned().collect()
    }

    /// Current number of candidates in the buffer.
    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }

    /// Returns `true` when the buffer holds no candidates.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Maximum number of candidates the buffer will hold.
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

// ── Global singleton ─────────────────────────────────────────────────────────

static GLOBAL_BUFFER: OnceLock<Buffer> = OnceLock::new();

/// Return the global [`Buffer`] singleton.
///
/// Initialised on first call with a default capacity of 1024. All producers
/// push into this buffer; the stability detector drains it.
pub fn global() -> &'static Buffer {
    GLOBAL_BUFFER.get_or_init(|| Buffer::new(1024))
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "learning_candidate_tests.rs"]
mod tests;
