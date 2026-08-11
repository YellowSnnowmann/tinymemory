//! Generic summary-tree mechanics shared by all tree flavors.
//!
//! Covers storage, buffer management, bucket-seal cascade, time-based
//! flush, the get-or-create registry primitive, and the kind/profile
//! factory.
//!
//! Flavor-specific policy (global digest, topic hotness, source file
//! mirror) lives in `crate::tree_global`,
//! `crate::tree_topic`, and
//! [`crate::tree_source`] respectively.
//!
//! Persistence (store + types) has moved to `memory_store::trees`.

pub mod bucket_seal;
pub mod factory;
pub mod flush;
pub mod registry;

// Re-export persistence from memory_store so callers using tree::store / tree::types still work.
pub use crate::store::trees::store;
pub use crate::store::trees::types;

pub use crate::store::trees::{get_summary_embedding, set_summary_embedding};
pub use crate::store::trees::{
    Buffer, SummaryNode, Tree, TreeKind, TreeStatus, INPUT_TOKEN_BUDGET, OUTPUT_TOKEN_BUDGET,
    SUMMARY_FANOUT,
};
pub use bucket_seal::{
    append_leaf, append_leaf_deferred, seal_document_subtree, LabelStrategy, LeafRef,
    MERGE_LEVEL_BASE,
};
pub use factory::{TreeFactory, TreeProfile, GLOBAL_SCOPE};
pub use registry::{get_or_create_tree, new_summary_id, new_tree_id};
