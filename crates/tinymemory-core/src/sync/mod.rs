//! Memory sync pipelines.
//!
//! One top-level module hosting every "pull data from upstream → land it
//! in memory_store" pipeline, organised by the kind of upstream it talks
//! to. Two kinds today:
//!
//! - [`workspace`] — Local workspace connectors (filesystem vault sync,
//!   local-only ingest, agent-experience capture from the harness).
//! - [`mcp`] — Third-party MCP servers. Pulls via the MCP protocol over
//!   stdio/SSE.
//!
//! Both implement the `SyncPipeline` trait so the orchestrator
//! (`memory::jobs`) can drive them uniformly: `init` → `tick` → repeat.
//!
//! ## Layer rules
//!
//! - Sync writes into `memory_store` only — never directly into trees,
//!   never directly into unified. The ingest pipeline in
//!   `memory::ingest_pipeline` is the seam.
//! - One pipeline per upstream service.
//! - Pipeline modules own their own types, their own state, and their
//!   own retry/backoff policy. The trait gives the orchestrator a
//!   single shape to call; everything else stays local.

pub mod audit;
pub mod mcp;
pub mod sync_status;
pub mod usage;
pub mod workspace;
