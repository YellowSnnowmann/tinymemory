//! One-shot installation of stub host seams for this crate's own tests.
//!
//! The seams fail loudly when unwired — see [`crate::embedding_host`] for why
//! that is deliberate — so a test that reaches any of them needs a host
//! installed. These stubs are the smallest thing that makes the *core's*
//! behaviour observable: a noop embedder, known cloud defaults.
//!
//! # What is NOT stubbed, on purpose
//!
//! There is no `ChatHost` stub. Which provider answers a role, and what model
//! id that resolves to, is host routing policy — a stub could only assert
//! itself. The tests that covered that behaviour moved to the host, where the
//! real implementation is.

use std::sync::Once;

static INIT: Once = Once::new();

/// Install the stub seams. Idempotent; safe to call from every test.
pub(crate) fn init() {
    INIT.call_once(|| {
        crate::embedding_host::TestEmbeddingHost::install();
    });
}
