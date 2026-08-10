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

use std::sync::{Arc, Once};

use async_trait::async_trait;

use crate::config_loader::ConfigLoader;
use crate::Config;

/// A [`ConfigLoader`] that hands back a default test config.
///
/// The background loops reload config on every tick by design; without a loader
/// they fail with the unwired-seam error before reaching the behaviour under
/// test.
#[derive(Debug)]
struct TestConfigLoader;

#[async_trait]
impl ConfigLoader for TestConfigLoader {
    async fn load(&self) -> Result<Box<Config>, String> {
        Ok(Box::new(
            tinymemory_api::host::test_support::TestHostConfig::default(),
        ))
    }

    async fn reload_snapshot(&self, _snapshot: &Config) -> Result<Arc<Config>, String> {
        Ok(Arc::new(
            tinymemory_api::host::test_support::TestHostConfig::default(),
        ))
    }
}

static INIT: Once = Once::new();

/// Install the stub seams. Idempotent; safe to call from every test.
pub(crate) fn init() {
    INIT.call_once(|| {
        crate::embedding_host::TestEmbeddingHost::install();
        crate::config_loader::set_config_loader(Arc::new(TestConfigLoader));
    });
}
