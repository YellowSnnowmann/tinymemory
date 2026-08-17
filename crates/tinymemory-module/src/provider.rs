//! The module's own configuration, converted for the engine provider.
//!
//! The provider itself now lives in `tinymemory-tinycortex` (issue #18 §C3).
//! Everything that was here delegated to `tinymemory-core` on a blocking
//! thread and was never module-specific; what remains is the one thing that is
//! — turning a `ModuleConfig` into the engine's runtime configuration.

use std::sync::Arc;

use tinymemory_core::store::MemoryClient;
use tinymemory_tinycortex::engine::{EngineRuntimeConfig, TinycortexProvider};

use crate::ModuleConfig;

impl From<&ModuleConfig> for EngineRuntimeConfig {
    fn from(config: &ModuleConfig) -> Self {
        Self {
            workspace_dir: config.workspace_dir.clone(),
            config_path: config.workspace_dir.join("config.toml"),
            memory: config.memory.clone(),
            memory_tree: config.memory_tree.clone(),
            scheduler_gate: config.scheduler_gate.clone(),
            local_ai: config.local_ai.clone(),
            embeddings_provider: config.embeddings_provider.clone(),
            memory_provider: config.memory_provider.clone(),
            default_model: config.default_model.clone(),
            default_temperature: config.default_temperature,
            output_language: config.output_language.clone(),
            memory_sources: config.memory_sources.clone(),
        }
    }
}

/// Builds the engine provider this module serves over the bus.
pub(crate) fn provider(config: &ModuleConfig, client: Arc<MemoryClient>) -> TinycortexProvider {
    TinycortexProvider::new(
        config.driver_id.clone(),
        EngineRuntimeConfig::from(config),
        client,
    )
}
