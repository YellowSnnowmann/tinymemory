//! Product Config adapters over tinycortex content readers.
use crate::engine::engine_config;

pub use crate::engine::backend::store::content::{
    read_chunk_file, read_summary_file, verify_chunk_file, verify_summary_file, ChunkFileContents,
    VerifyResult,
};

pub fn read_chunk_body(config: &crate::Config, chunk_id: &str) -> anyhow::Result<String> {
    crate::engine::backend::store::content::read_chunk_body(&engine_config(config), chunk_id)
}

pub fn read_summary_body(config: &crate::Config, summary_id: &str) -> anyhow::Result<String> {
    crate::engine::backend::store::content::read_summary_body(&engine_config(config), summary_id)
}
