//! `Config` adapter for tinycortex-owned bounded graph traversal.

use anyhow::Result;

use crate::Config;

pub use crate::engine::backend::graph::PairDistance;

pub fn pair_distances(
    config: &Config,
    entity_ids: &[String],
    max_h: u32,
) -> Result<Vec<PairDistance>> {
    crate::engine::backend::graph::pair_distances(
        &crate::engine::memory_config_from(config, config.workspace_dir().clone()),
        entity_ids,
        max_h,
    )
}
