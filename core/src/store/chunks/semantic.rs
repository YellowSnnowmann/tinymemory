//! Compatibility exports for tinycortex's semantic Markdown chunker.

pub use crate::engine::backend::chunks::SemanticChunk as Chunk;

pub fn chunk_markdown(text: &str, max_tokens: usize) -> Vec<Chunk> {
    crate::engine::backend::chunks::chunk_semantic(text, max_tokens)
}
