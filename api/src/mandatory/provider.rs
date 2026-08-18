//! [`MemoryTraitProvider`](crate::mandatory::MemoryTraitProvider) — a complete, mandatory-only
//! [`MemoryProvider`](crate::provider::MemoryProvider) over any
//! [`Memory`](crate::traits::Memory) backend.
//!
//! ## What this is for
//!
//! Two things, and it is worth being clear which is which.
//!
//! **A real driver for a simple backend.** A store that implements [`Memory`](crate::traits::Memory)
//! becomes a bindable memory driver by wrapping it here — no capability
//! plumbing, no export format to invent. It advertises exactly the three
//! mandatory families, so a host binding it gets a memory subsystem whose
//! optional surface is *absent* rather than present-and-failing.
//!
//! **A conformance baseline.** Because it composes the same functions a richer
//! driver delegates to, testing a backend through this type tests the shared
//! layer directly, without a host in the picture.
//!
//! ## What it deliberately does not do
//!
//! No optional families. Every `as_*` accessor keeps the contract's `None`
//! default, and [`capabilities`](MemoryTraitProvider::capabilities) reports the
//! mandatory three — so the two halves agree and
//! [`audit_provider`](crate::provider::audit_provider) passes. A driver
//! that wants documents, trees, or a diff ledger implements those families over
//! its own engine and delegates only the mandatory three here.
//!
//! No policy. Tier checks, scope predicates, taint stamping, redaction and
//! audit belong in a decorator the *host* owns — see the crate docs.

use std::sync::Arc;

use crate::capabilities::{Capabilities, Capability};
use crate::error::MemoryError;
use crate::health::MemoryHealth;
use crate::provider::types::{ExportPage, ExportRecord, ImportOutcome, SourceScope};
use crate::provider::{MemoryCore, MemoryPortability, MemoryProvider, MemoryRecall};
use crate::recall::OwnedRecallOpts;
use crate::traits::Memory;
use crate::types::{MemoryCategory, MemoryEntry, MemoryTaint, NamespaceSummary};
use async_trait::async_trait;

use super::{engine_error, export_page, import_records, list_everything, recall};

/// A mandatory-only memory driver over an [`Memory`](crate::traits::Memory) backend.
#[derive(Clone)]
pub struct MemoryTraitProvider {
    memory: Arc<dyn Memory>,
    driver_id: String,
}

impl std::fmt::Debug for MemoryTraitProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `dyn Memory` is not `Debug`, and a backend handle is not something to
        // render anyway — it may hold a connection string.
        f.debug_struct("MemoryTraitProvider")
            .field("driver_id", &self.driver_id)
            .finish_non_exhaustive()
    }
}

impl MemoryTraitProvider {
    /// Wrap `memory` as a driver reporting `driver_id`.
    ///
    /// `driver_id` must be stable across restarts and must not embed a URL, a
    /// token, or anything else deployment-specific: it appears in status
    /// output, log lines, tracing spans, and audit events.
    #[must_use]
    pub fn new(memory: Arc<dyn Memory>, driver_id: impl Into<String>) -> Self {
        Self {
            memory,
            driver_id: driver_id.into(),
        }
    }

    /// The wrapped backend.
    #[must_use]
    pub fn memory(&self) -> &Arc<dyn Memory> {
        &self.memory
    }

    /// The families this type implements: the mandatory three, and nothing
    /// else.
    #[must_use]
    pub fn advertised_capabilities() -> Capabilities {
        Capabilities::from_iter([
            Capability::Core,
            Capability::Recall,
            Capability::Portability,
        ])
    }
}

#[async_trait]
impl MemoryCore for MemoryTraitProvider {
    async fn store(
        &self,
        namespace: &str,
        key: &str,
        content: &str,
        category: MemoryCategory,
        session_id: Option<&str>,
        taint: MemoryTaint,
    ) -> Result<(), MemoryError> {
        // `store_with_taint`, never `store` — see the module docs on `super`.
        self.memory
            .store_with_taint(namespace, key, content, category, session_id, taint)
            .await
            .map_err(engine_error)
    }

    async fn get(&self, namespace: &str, key: &str) -> Result<Option<MemoryEntry>, MemoryError> {
        self.memory.get(namespace, key).await.map_err(engine_error)
    }

    async fn forget(&self, namespace: &str, key: &str) -> Result<bool, MemoryError> {
        self.memory
            .forget(namespace, key)
            .await
            .map_err(engine_error)
    }

    async fn list(
        &self,
        namespace: Option<&str>,
        category: Option<&MemoryCategory>,
        session_id: Option<&str>,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        match namespace {
            Some(namespace) => self
                .memory
                .list(Some(namespace), category, session_id)
                .await
                .map_err(engine_error),
            None => list_everything(self.memory.as_ref(), category, session_id).await,
        }
    }

    async fn namespaces(&self) -> Result<Vec<NamespaceSummary>, MemoryError> {
        // The contract's `namespaces` is the backend's `namespace_summaries`;
        // the return type is identical, only the name differs.
        self.memory
            .namespace_summaries()
            .await
            .map_err(engine_error)
    }
}

#[async_trait]
impl MemoryRecall for MemoryTraitProvider {
    async fn recall(
        &self,
        query: &str,
        limit: usize,
        opts: &OwnedRecallOpts,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        recall(self.memory.as_ref(), query, limit, opts, scope).await
    }
}

#[async_trait]
impl MemoryPortability for MemoryTraitProvider {
    async fn export_page(
        &self,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<ExportPage, MemoryError> {
        export_page(self.memory.as_ref(), cursor, limit).await
    }

    async fn import_records(
        &self,
        records: Vec<ExportRecord>,
    ) -> Result<ImportOutcome, MemoryError> {
        import_records(self.memory.as_ref(), records).await
    }
}

#[async_trait]
impl MemoryProvider for MemoryTraitProvider {
    fn driver_id(&self) -> &str {
        &self.driver_id
    }

    fn capabilities(&self) -> Capabilities {
        Self::advertised_capabilities()
    }

    async fn health(&self) -> MemoryHealth {
        if self.memory.health_check().await {
            MemoryHealth::Ready
        } else {
            // No path and no connection detail: this string is logged and
            // rendered in operator-facing status.
            MemoryHealth::down("memory backend reported unhealthy")
        }
    }

    // `shutdown` keeps the contract's no-op default. The backend handle is an
    // `Arc` this type does not own exclusively; a driver must not tear down a
    // handle its host may still hold.
}
