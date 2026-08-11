//! Consolidated memory search & retrieval module.
//!
//! All agent-facing retrieval tools, vector search infrastructure, and
//! scoring algorithms are accessible from here. Lower layers (`memory_store`,
//! `memory_tree`) provide persistence and tree traversal; this module composes
//! them into tools the agent can invoke.

// ── Public re-exports ───────────────────────────────────────────────────────
