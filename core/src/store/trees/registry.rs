//! `Config` adapters for tinycortex's tree registry.

use anyhow::Result;

use crate::engine::engine_config;
use crate::store::trees::types::{Tree, TreeKind};
use crate::Config;

pub fn list_trees_by_kind(config: &Config, kind: TreeKind) -> Result<Vec<Tree>> {
    crate::engine::backend::tree::store::list_trees_by_kind(&engine_config(config), kind)
}

pub fn archive_tree(config: &Config, tree_id: &str) -> Result<()> {
    log::debug!("[memory:trees] archive tree_id={tree_id}");
    crate::engine::backend::tree::store::archive_tree(&engine_config(config), tree_id)
}
