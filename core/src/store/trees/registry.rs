//! `Config` adapters for tinycortex's tree registry.

use anyhow::Result;

use crate::openhuman::config::Config;
use crate::store::trees::types::{Tree, TreeKind};
use crate::tinycortex::engine_config;

pub fn list_trees_by_kind(config: &Config, kind: TreeKind) -> Result<Vec<Tree>> {
    tinycortex::memory::tree::store::list_trees_by_kind(&engine_config(config), kind)
}

pub fn archive_tree(config: &Config, tree_id: &str) -> Result<()> {
    log::debug!("[memory:trees] archive tree_id={tree_id}");
    tinycortex::memory::tree::store::archive_tree(&engine_config(config), tree_id)
}
