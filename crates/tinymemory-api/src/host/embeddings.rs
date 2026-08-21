//! [`EmbeddingProvider`] — text → vector, supplied by the host.
//!
//! The memory subsystem embeds chunks, summaries and queries, but it does not
//! decide *how*: which provider, which credentials, which rate limit and which
//! fallback are host policy. So the core takes an `Arc<dyn EmbeddingProvider>`
//! and never constructs one.
//!
//! This trait deliberately lives in the contract crate rather than in
//! `tinymemory-core`, so that a host implementing it does not have to depend on
//! the engine. It carries nothing heavier than `async-trait` and `anyhow`.

use async_trait::async_trait;

/// Formats the canonical embedding-space signature string.
///
/// This is the **single source of truth** for the signature format. Both the
/// live-provider [`EmbeddingProvider::signature`] and any config-derived
/// signature must route through here, so a signature computed from
/// configuration is byte-identical to one computed from an instantiated
/// provider. Drift between the two silently splits one embedding space into
/// two, and every vector written on the wrong side of the split becomes
/// unsearchable without a re-embed.
/// # Delimiters in a component
///
/// A component containing `;`, `=` or `%` is percent-encoded, because without
/// that the format is ambiguous: `("a;model=b", "c")` and `("a", "b;model=c")`
/// are different embedding spaces that would otherwise produce one identical
/// key, and vectors from both would then be compared as though they came from
/// the same model.
///
/// Encoding only those three characters is what keeps this from being a
/// migration. Every provider and model identifier actually in use is
/// alphanumeric plus `-`, `_`, `.`, `/` or `:`, and each of those passes
/// through untouched — so every signature already on disk still formats to the
/// same bytes. Only a name that could have collided changes, and such a name
/// has never been written.
#[must_use]
pub fn format_embedding_signature(name: &str, model_id: &str, dims: usize) -> String {
    let name = escape_component(name);
    let model_id = escape_component(model_id);
    format!("provider={name};model={model_id};dims={dims}")
}

/// Percent-encode the three characters that carry structure in a signature.
///
/// `%` goes first and must: encoding it afterwards would re-encode the `%` this
/// function just introduced, and `a;b` would arrive as `a%3Bb` from one path
/// and `a%253Bb` from another.
fn escape_component(value: &str) -> String {
    if !value.contains(['%', ';', '=']) {
        // The overwhelmingly common path, and the one that guarantees existing
        // keys are untouched: no allocation beyond the copy, no rewriting.
        return value.to_string();
    }
    value
        .replace('%', "%25")
        .replace(';', "%3B")
        .replace('=', "%3D")
}

#[cfg(test)]
mod embedding_signature_tests {
    use async_trait::async_trait;

    use super::{format_embedding_signature, EmbeddingProvider, NoopEmbedding};

    /// The signature format is a **persisted key**, pinned to literal values.
    ///
    /// Written against golden strings rather than against another copy of the
    /// function on purpose: the host used to hold a byte-identical duplicate of
    /// this file and the two silently diverged once already. A guard that
    /// compares two implementations stops protecting anything the moment one of
    /// them goes away — which is exactly what happened when the duplicate was
    /// removed. Literals outlive that.
    ///
    /// Every vector on disk is keyed by one of these strings, so a change here
    /// is a migration, never an edit.
    #[test]
    fn signature_format_is_pinned_to_its_persisted_form() {
        assert_eq!(
            format_embedding_signature("ollama", "nomic-embed-text", 768),
            "provider=ollama;model=nomic-embed-text;dims=768"
        );
        assert_eq!(
            format_embedding_signature("none", "none", 0),
            "provider=none;model=none;dims=0"
        );
    }

    /// Two distinct embedding spaces must never share one signature.
    ///
    /// Without escaping these two collide exactly: both format to
    /// `provider=a;model=b;model=c;dims=3`. A collision here is not a cosmetic
    /// problem — the signature is what decides which vectors are comparable, so
    /// two models' vectors would be scored against each other as though they
    /// came from one space.
    #[test]
    fn delimiter_characters_cannot_make_distinct_spaces_collide() {
        let first = format_embedding_signature("a;model=b", "c", 3);
        let second = format_embedding_signature("a", "b;model=c", 3);
        assert_ne!(first, second);
    }

    /// Escaping `%` last would make the encoding itself ambiguous.
    #[test]
    fn an_already_percent_encoded_name_does_not_collide_with_a_literal_one() {
        assert_ne!(
            format_embedding_signature("a%3Bb", "m", 3),
            format_embedding_signature("a;b", "m", 3)
        );
    }

    /// The escaping is not a migration: every identifier shaped like the ones
    /// actually in use formats to the same bytes it always did.
    #[test]
    fn identifiers_in_real_use_are_untouched_by_the_escaping() {
        for (provider, model) in [
            ("ollama", "nomic-embed-text"),
            ("openai", "text-embedding-3-small"),
            ("huggingface", "sentence-transformers/all-MiniLM-L6-v2"),
            ("local", "bge_base.en-v1.5"),
            ("backend", "tinyhumans:default"),
        ] {
            assert_eq!(
                format_embedding_signature(provider, model, 768),
                format!("provider={provider};model={model};dims=768"),
                "{provider}/{model} must not be rewritten — it is a persisted key"
            );
        }
    }

    #[tokio::test]
    async fn noop_returns_one_empty_vector_per_input() {
        let provider = NoopEmbedding;
        assert_eq!(provider.signature(), "provider=none;model=none;dims=0");
        assert_eq!(
            provider.embed(&["a", "b"]).await.unwrap(),
            vec![Vec::<f32>::new(), Vec::<f32>::new()]
        );
        assert!(provider.embed_one("a").await.unwrap().is_empty());
    }

    struct EmptyProvider;

    #[async_trait]
    impl EmbeddingProvider for EmptyProvider {
        fn name(&self) -> &str {
            "empty"
        }
        fn model_id(&self) -> &str {
            "empty"
        }
        fn dimensions(&self) -> usize {
            0
        }
        async fn embed(&self, _: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn embed_one_rejects_a_provider_that_returns_no_vectors() {
        let error = EmptyProvider.embed_one("text").await.unwrap_err();
        assert!(error.to_string().contains("Empty embedding result"));
    }
}

/// Converts text into numerical vectors.
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Provider name, e.g. `"ollama"`, `"openai"`.
    fn name(&self) -> &str;

    /// Stable model identifier used to generate embeddings.
    fn model_id(&self) -> &str;

    /// Number of dimensions in the generated embeddings.
    fn dimensions(&self) -> usize;

    /// Stable signature for the embedding space.
    ///
    /// Changing any component means existing vectors are no longer comparable
    /// with newly generated ones and must be stored and queried separately
    /// until a migration re-embeds them.
    fn signature(&self) -> String {
        format_embedding_signature(self.name(), self.model_id(), self.dimensions())
    }

    /// Generates embeddings for a batch of strings.
    ///
    /// # Errors
    /// Propagates transport, authentication and quota failures from the
    /// underlying provider.
    async fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>>;

    /// Generates an embedding for a single string.
    ///
    /// # Errors
    /// As [`Self::embed`], plus an error when the provider returns no vector.
    async fn embed_one(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let mut results = self.embed(&[text]).await?;
        results
            .pop()
            .ok_or_else(|| anyhow::anyhow!("Empty embedding result"))
    }
}

/// The inert provider bound when semantic search is switched off or no
/// embedding backend is configured. Reports zero dimensions and returns one
/// empty vector per input, so keyword-only retrieval keeps working while
/// vector rerank degrades to a no-op rather than an error.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopEmbedding;

#[async_trait]
impl EmbeddingProvider for NoopEmbedding {
    fn name(&self) -> &str {
        "none"
    }

    fn model_id(&self) -> &str {
        "none"
    }

    fn dimensions(&self) -> usize {
        0
    }

    async fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        Ok(vec![Vec::new(); texts.len()])
    }
}
