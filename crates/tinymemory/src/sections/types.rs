//! The value types the section surface returns, and the two constants that
//! bound and explain its one non-obvious behaviour.
//!
//! Kept apart from the handles in [`view`](super::view) and
//! [`recall`](super::recall) because they are what a caller *stores* — a
//! [`SectionScope`] outlives the [`SectionView`](super::SectionView) that
//! produced it, whereas the handles borrow the provider and cannot.

use tinymemory_api::namespace::Namespace;
use tinymemory_api::types::MemoryEntry;

/// How many namespaces [`SectionRecall::across_section`] visits before it stops.
///
/// A section-wide recall costs one provider call per namespace in the section,
/// so an unbounded fan-out would let a store that has accumulated thousands of
/// conversations turn one call into thousands. The cap trades completeness for a
/// predictable ceiling and reports when it bit, through
/// [`SectionHits::truncated`] — rather than silently returning a partial answer
/// that looks complete.
///
/// [`SectionRecall::across_section`]: super::SectionRecall::across_section
pub const MAX_SECTION_NAMESPACES: usize = 64;

/// The message carried by the [`MemoryError::Invalid`] that
/// [`SectionRecall`](super::SectionRecall) returns when the caller's recall
/// options already pin a namespace.
///
/// A section recall derives the namespace itself — from the section and, for
/// [`in_scope`](super::SectionRecall::in_scope), from the scope. Honouring a
/// caller's `namespace` too would mean either ignoring one of the two filters or
/// intersecting them into an empty result, so the conflict is refused instead.
///
/// Exposed as a constant, following the precedent of the contract's own
/// `SCOPE_UNAPPLIED`, so a caller's test asserts the same string the caller sees.
///
/// [`MemoryError::Invalid`]: tinymemory_api::error::MemoryError::Invalid
pub const NAMESPACE_FILTER_CONFLICT: &str =
    "recall options must not set a namespace: the section surface derives it";

/// One namespace within a section, as [`SectionView::scopes`] reports it.
///
/// [`SectionView::scopes`]: super::SectionView::scopes
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionScope {
    /// The parsed namespace, section prefix included.
    pub namespace: Namespace,
    /// How many entries it holds.
    pub entries: usize,
    /// RFC 3339 timestamp of its most recent update, when the driver tracks one.
    pub last_updated: Option<String>,
}

impl SectionScope {
    /// The scope — the part after the section prefix.
    ///
    /// This is the string every [`SectionView`](super::SectionView) method
    /// takes, so a scope discovered here can be passed straight back in.
    #[must_use]
    pub fn scope(&self) -> &str {
        self.namespace.scope()
    }
}

/// What a section-wide recall found, and how much of the section it saw.
///
/// The two non-hit fields exist so a caller can tell "the section holds nothing
/// matching" from "the fan-out stopped early", which a bare `Vec` cannot express.
#[derive(Debug, Clone, Default)]
pub struct SectionHits {
    /// The merged hits, most relevant first.
    pub hits: Vec<MemoryEntry>,
    /// How many namespaces were actually searched.
    pub namespaces_searched: usize,
    /// Whether [`MAX_SECTION_NAMESPACES`] stopped the fan-out short.
    ///
    /// This says namespaces were *skipped*. It never means the hits themselves
    /// were truncated to the caller's limit, which is expected and ordinary.
    pub truncated: bool,
}

/// The score a hit sorts on, with an absent score ordering last.
///
/// Absent scores map to negative infinity rather than zero: a driver that scores
/// nothing would otherwise have its hits outrank genuinely poor matches.
fn sort_score(entry: &MemoryEntry) -> f64 {
    entry.score.unwrap_or(f64::NEG_INFINITY)
}

/// Merge hits gathered from several namespaces into one ranked, bounded list.
///
/// Ordering is score descending, absent scores last, ties broken by namespace
/// then key — total and deterministic, so a fixed store always yields the same
/// answer. `f64::total_cmp` is used rather than `partial_cmp` so a `NaN` score
/// from a misbehaving driver orders predictably instead of poisoning the sort.
pub(super) fn merge_hits(mut hits: Vec<MemoryEntry>, limit: usize) -> Vec<MemoryEntry> {
    hits.sort_by(|a, b| {
        sort_score(b)
            .total_cmp(&sort_score(a))
            .then_with(|| a.namespace.cmp(&b.namespace))
            .then_with(|| a.key.cmp(&b.key))
    });
    hits.truncate(limit);
    hits
}
