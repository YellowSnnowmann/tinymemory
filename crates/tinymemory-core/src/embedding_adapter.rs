//! [`TinyInferenceEmbeddingProvider`] — the adapter from TinyInference's embedding
//! model trait onto the seam's [`EmbeddingProvider`].
//!
//! It lives in this crate rather than in `tinymemory-api` because the contract
//! crate must stay dependency-light and cannot name `tinyinference`; and rather
//! than in the host because the tree's embedder factory — which is core code —
//! builds Ollama models directly and needs to wrap them. The host re-exports it
//! from `inference::embeddings`, so every existing path there keeps resolving
//! and keeps naming this one type.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tinyinference::embeddings::EmbeddingModel;

pub use tinymemory_api::host::{format_embedding_signature, EmbeddingProvider};

/// Compatibility adapter from the canonical TinyInference embedding model.
pub struct TinyInferenceEmbeddingProvider {
    model: Box<dyn EmbeddingModel>,
}

impl TinyInferenceEmbeddingProvider {
    pub fn new(model: impl EmbeddingModel + 'static) -> Self {
        Self {
            model: Box::new(model),
        }
    }

    pub fn boxed(model: impl EmbeddingModel + 'static) -> Box<dyn EmbeddingProvider> {
        Box::new(Self::new(model))
    }
}

#[async_trait]
impl EmbeddingProvider for TinyInferenceEmbeddingProvider {
    fn name(&self) -> &str {
        self.model.name()
    }

    fn model_id(&self) -> &str {
        self.model.model_id()
    }

    fn dimensions(&self) -> usize {
        self.model.dimensions()
    }

    fn signature(&self) -> String {
        self.model.signature()
    }

    async fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        let owned = texts
            .iter()
            .map(|text| (*text).to_owned())
            .collect::<Vec<_>>();
        self.model
            .embed(&owned)
            .await
            .map_err(|error| anyhow::anyhow!(error))
    }
}

/// Context and batch window used for long-document Ollama embeddings.
pub(crate) const RECOMMENDED_OLLAMA_CONTEXT_TOKENS: u32 = 8192;

/// TinyInference-compatible Ollama model carrying TinyMemory's long-context
/// request policy.
///
/// TinyInference deliberately owns the provider-neutral [`EmbeddingModel`]
/// contract. TinyMemory owns this one request policy because its persisted
/// chunks are sized for `bge-m3`'s full 8K context and silently accepting a
/// provider default with a smaller window would truncate those chunks.
pub(crate) struct LongContextOllamaEmbeddingModel {
    client: reqwest::Client,
    base_url: String,
    model: String,
    dimensions: usize,
}

impl LongContextOllamaEmbeddingModel {
    /// Build a validated Ollama model with the supplied HTTP client.
    pub(crate) fn try_new(
        base_url: &str,
        model: &str,
        dimensions: usize,
        client: reqwest::Client,
    ) -> tinyinference::Result<Self> {
        let raw_url = if base_url.trim().is_empty() {
            tinyinference::embeddings::DEFAULT_OLLAMA_URL
        } else {
            base_url.trim()
        };
        let parsed = reqwest::Url::parse(raw_url).map_err(|error| {
            tinyinference::Error::Validation(format!(
                "invalid Ollama base_url `{raw_url}`: {error}"
            ))
        })?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(tinyinference::Error::Validation(format!(
                "invalid Ollama base_url `{raw_url}`: expected an http:// or https:// URL"
            )));
        }
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err(tinyinference::Error::Validation(format!(
                "invalid Ollama base_url `{raw_url}`: configure the server root without credentials"
            )));
        }
        if parsed.query().is_some() || parsed.fragment().is_some() {
            return Err(tinyinference::Error::Validation(format!(
                "invalid Ollama base_url `{raw_url}`: query strings and fragments are not supported"
            )));
        }

        let model = if model.trim().is_empty() {
            tinyinference::embeddings::DEFAULT_OLLAMA_MODEL.to_owned()
        } else {
            model.trim().to_owned()
        };
        if model.to_ascii_lowercase().starts_with("local-") {
            return Err(tinyinference::Error::Validation(format!(
                "invalid Ollama embedding model `{model}`: `local-*` IDs are virtual routing aliases"
            )));
        }

        Ok(Self {
            client,
            base_url: parsed.as_str().trim_end_matches('/').to_owned(),
            model,
            dimensions,
        })
    }

    async fn request(&self, input: Vec<String>) -> tinyinference::Result<reqwest::Response> {
        self.client
            .post(format!("{}/api/embed", self.base_url))
            .json(&OllamaRequest {
                model: self.model.clone(),
                input,
                options: OllamaOptions {
                    num_ctx: RECOMMENDED_OLLAMA_CONTEXT_TOKENS,
                    num_batch: RECOMMENDED_OLLAMA_CONTEXT_TOKENS,
                },
            })
            .send()
            .await
            .map_err(|error| {
                tinyinference::Error::Embedding(format!(
                    "ollama embed request failed (is Ollama running at {}?): {error}",
                    self.base_url
                ))
            })
    }

    fn validate_vectors(&self, expected: usize, vectors: &[Vec<f32>]) -> tinyinference::Result<()> {
        if vectors.len() != expected {
            return Err(tinyinference::Error::Embedding(format!(
                "ollama embed count mismatch: sent {expected} texts, got {} embeddings",
                vectors.len()
            )));
        }
        for (index, vector) in vectors.iter().enumerate() {
            if vector.len() != self.dimensions {
                return Err(tinyinference::Error::Embedding(format!(
                    "ollama embed dimension mismatch at index {index}: expected {}, got {}",
                    self.dimensions,
                    vector.len()
                )));
            }
        }
        Ok(())
    }
}

#[async_trait]
impl EmbeddingModel for LongContextOllamaEmbeddingModel {
    fn name(&self) -> &str {
        "ollama"
    }

    fn model_id(&self) -> &str {
        &self.model
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    async fn embed(&self, texts: &[String]) -> tinyinference::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        if texts.iter().any(|text| text.trim().is_empty()) {
            return Err(tinyinference::Error::Validation(
                "Ollama embedding batches must not contain blank inputs".to_owned(),
            ));
        }

        let response = self.request(texts.to_vec()).await?;
        if !response.status().is_success() {
            let status = response.status();
            let detail = response.text().await.unwrap_or_default();
            return Err(tinyinference::Error::Embedding(format!(
                "ollama embed failed with status {status}{}",
                if detail.trim().is_empty() {
                    String::new()
                } else {
                    format!(": {}", detail.trim())
                }
            )));
        }
        let payload: OllamaResponse = response.json().await.map_err(|error| {
            tinyinference::Error::Embedding(format!(
                "ollama embed response parse failed: {error}"
            ))
        })?;
        self.validate_vectors(texts.len(), &payload.embeddings)?;
        Ok(payload.embeddings)
    }
}

#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    input: Vec<String>,
    options: OllamaOptions,
}

#[derive(Clone, Copy, Serialize)]
struct OllamaOptions {
    num_ctx: u32,
    num_batch: u32,
}

#[derive(Deserialize)]
struct OllamaResponse {
    #[serde(default)]
    embeddings: Vec<Vec<f32>>,
}

#[cfg(test)]
#[path = "embedding_adapter_test.rs"]
mod test;
