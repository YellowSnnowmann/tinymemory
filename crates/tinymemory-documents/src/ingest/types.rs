//! The request intake takes, and the receipt it gives back.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use tinymemory_api::chunks::{DataSource, SourceRef};
use tinymemory_api::namespace::Namespace;
use tinymemory_api::types::{MemoryCategory, MemoryTaint};

use crate::convert::{ConvertedDocument, RawDocument};
use crate::error::Result;
use crate::format::DocumentFormat;

/// Where [`super::DocumentIntake`] put a document.
///
/// Reported rather than hidden because the three routes have genuinely
/// different consequences — only [`IntakeRoute::Ingest`] chunks and embeds —
/// and a host that cannot see which one it got cannot explain to a user why
/// their upload is not searchable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntakeRoute {
    /// Through [`tinymemory_api::provider::MemoryIngest`]: the driver chunked
    /// and embedded the document.
    Ingest,
    /// Through [`tinymemory_api::provider::MemoryDocuments`]: stored whole,
    /// queryable by the document tier's own ranking.
    Documents,
    /// Through [`tinymemory_api::provider::MemoryCore::store`]: one entry, no
    /// chunking. Always available, least capable.
    Core,
}

impl IntakeRoute {
    /// The wire spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ingest => "ingest",
            Self::Documents => "documents",
            Self::Core => "core",
        }
    }

    /// Whether this route chunks and embeds the document.
    pub fn is_chunked(self) -> bool {
        matches!(self, Self::Ingest)
    }
}

impl std::fmt::Display for IntakeRoute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Everything about *where* a document should land, as opposed to what it is.
///
/// [`Self::key`] is derived rather than required: most callers have no
/// meaningful key for an upload, and a caller forced to invent one invents a
/// random id, which makes re-uploading the same document produce a second copy.
#[derive(Debug, Clone)]
pub struct IntakeRequest {
    /// Target namespace. Validated against the
    /// [`tinymemory_api::namespace`] convention before any write.
    pub namespace: String,
    /// Explicit upsert key. Derived from the document when absent.
    pub key: Option<String>,
    /// Where the content came from.
    pub source: DataSource,
    /// Account or user the content belongs to; empty for anonymous.
    pub owner: String,
    /// Labels carried through to the driver.
    pub tags: Vec<String>,
    /// Category for the document and core routes.
    pub category: MemoryCategory,
    /// Priority label for the document route.
    pub priority: String,
    /// Optional session scope.
    pub session_id: Option<String>,
    /// Event time for tree placement; the driver substitutes ingest time when
    /// absent.
    pub timestamp: Option<DateTime<Utc>>,
    /// Provenance taint. Passed straight through — intake never assigns it.
    pub taint: MemoryTaint,
}

impl IntakeRequest {
    /// A request targeting `namespace`, with the defaults an upload wants.
    ///
    /// Taint defaults to [`MemoryTaint::ExternalSync`], the closed default: a
    /// document that arrived from outside is external until a host that knows
    /// better says otherwise.
    pub fn new(namespace: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            key: None,
            source: DataSource::Upload,
            owner: String::new(),
            tags: Vec::new(),
            category: MemoryCategory::Core,
            priority: "normal".to_string(),
            session_id: None,
            timestamp: None,
            taint: MemoryTaint::ExternalSync,
        }
    }

    /// A request for a document fetched from a URL.
    pub fn from_url(namespace: impl Into<String>) -> Self {
        Self {
            source: DataSource::WebPage,
            ..Self::new(namespace)
        }
    }

    /// Set an explicit upsert key.
    #[must_use]
    pub fn with_key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// Attach tags.
    #[must_use]
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// Set the provenance taint.
    #[must_use]
    pub fn with_taint(mut self, taint: MemoryTaint) -> Self {
        self.taint = taint;
        self
    }

    /// Set the owner.
    #[must_use]
    pub fn with_owner(mut self, owner: impl Into<String>) -> Self {
        self.owner = owner.into();
        self
    }

    /// Set the category.
    #[must_use]
    pub fn with_category(mut self, category: MemoryCategory) -> Self {
        self.category = category;
        self
    }

    /// Set the event time.
    #[must_use]
    pub fn with_timestamp(mut self, timestamp: DateTime<Utc>) -> Self {
        self.timestamp = Some(timestamp);
        self
    }

    /// Check the namespace against the naming convention.
    ///
    /// # Errors
    ///
    /// [`tinymemory_api::error::MemoryError::Invalid`] when the namespace
    /// fails [`Namespace::parse`].
    pub fn validate(&self) -> Result<()> {
        Namespace::parse(&self.namespace)?;
        Ok(())
    }

    /// The upsert key for this document: the explicit one, or a stable key
    /// derived from the document's origin, filename, or title.
    ///
    /// Derivation order matters. A URL identifies a document across re-fetches,
    /// a filename identifies it across re-uploads, and a title is the last
    /// resort — so re-fetching a page updates it rather than duplicating it.
    pub fn key(&self, document: &RawDocument, title: &str) -> String {
        if let Some(key) = &self.key {
            return key.clone();
        }
        let raw = document
            .origin
            .clone()
            .or_else(|| document.filename.clone())
            .unwrap_or_else(|| title.to_string());
        // `https://` and `http://` slugify into a `https-//` prefix that is on
        // every key and distinguishes nothing. Dropping the scheme also makes
        // the same page fetched over both schemes upsert rather than duplicate.
        let raw = raw.split_once("://").map_or(raw.as_str(), |(_, rest)| rest);
        slugify(raw)
    }

    /// The title to use when the document carries none.
    pub fn fallback_title(&self, document: &RawDocument) -> String {
        document.display_name()
    }

    /// A pointer back to where the document came from, for citation.
    pub fn source_ref(&self, document: &RawDocument) -> Option<SourceRef> {
        document
            .origin
            .clone()
            .or_else(|| document.filename.clone())
            .map(|value| SourceRef { value })
    }

    /// Metadata to attach on the document route.
    ///
    /// Records the original format and size alongside the converted body, so a
    /// stored document still says it used to be a PDF after conversion has
    /// erased every other trace of that.
    pub fn document_metadata(
        &self,
        document: &RawDocument,
        converted: &ConvertedDocument,
    ) -> serde_json::Value {
        serde_json::json!({
            "source_format": converted.format.to_string(),
            "source_bytes": converted.source_bytes,
            "source_mime": converted.format.mime(),
            "origin": document.origin,
            "filename": document.filename,
            "converter": converted.metadata,
        })
    }
}

/// What intake actually did.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntakeReceipt {
    /// Which family took the write.
    pub route: IntakeRoute,
    /// Namespace the document landed in.
    pub namespace: String,
    /// Upsert key it was stored under. Re-submitting the same document with
    /// the same request produces the same key.
    pub key: String,
    /// Title as stored.
    pub title: String,
    /// Format the source was detected as, before conversion.
    pub format: DocumentFormat,
    /// Size of the converted markdown, in bytes.
    pub markdown_bytes: usize,
    /// Size of the source document, in bytes.
    pub source_bytes: usize,
    /// Driver-assigned ids, when the driver surfaces them.
    #[serde(default)]
    pub ids: Vec<String>,
    /// Units the driver newly persisted.
    pub written: u32,
    /// Units the driver recognised as already present.
    pub skipped: u32,
}

/// Reduce arbitrary text to a namespace-safe key.
///
/// Deliberately lossy and deliberately deterministic: the same URL or filename
/// must always produce the same key, or re-ingesting a document would store a
/// second copy instead of replacing the first.
fn slugify(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_dash = false;
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if matches!(c, '.' | '_' | '-' | '/') && !out.is_empty() {
            out.push(c);
            last_dash = false;
        } else if !out.is_empty() && !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches(['-', '/', '.']);
    if trimmed.is_empty() {
        return "document".to_string();
    }
    // Keys share the namespace character rules and the same practical length
    // ceiling; a key longer than this is a URL with a session token in it. A
    // shortened key is disambiguated with a digest of the *full* input, so two
    // origins that only differ after the cut do not upsert over each other.
    if trimmed.chars().count() <= 120 {
        return trimmed.to_string();
    }
    let head: String = trimmed.chars().take(112).collect();
    let head = head.trim_matches(['-', '/', '.']);
    format!("{head}-{:07x}", fnv1a(trimmed) & 0xfff_ffff)
}

/// A stable, non-cryptographic digest used only to keep truncated slugify keys
/// distinct. FNV-1a rather than `DefaultHasher`, whose output is not
/// guaranteed stable across Rust releases and would silently reshuffle keys
/// that were already truncated.
fn fnv1a(raw: &str) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET_BASIS;
    for byte in raw.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}
