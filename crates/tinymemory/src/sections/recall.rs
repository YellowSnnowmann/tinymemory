//! [`SectionRecall`] — ranked retrieval within one scope, or across a section.

use std::fmt;

use tinymemory_api::error::MemoryError;
use tinymemory_api::namespace::MemorySection;
use tinymemory_api::provider::types::SourceScope;
use tinymemory_api::provider::MemoryProvider;
use tinymemory_api::recall::OwnedRecallOpts;

use super::types::{
    merge_hits, SectionHits, CROSS_SESSION_SECTION_CONFLICT, MAX_SECTION_NAMESPACES,
    NAMESPACE_FILTER_CONFLICT,
};
use super::view::SectionView;

/// A borrowing handle for section-aware recall.
///
/// Two questions, deliberately separate because they cost different amounts:
/// [`Self::in_scope`] is one provider call, and [`Self::across_section`] is one
/// per namespace in the section.
#[derive(Clone, Copy)]
pub struct SectionRecall<'a> {
    provider: &'a dyn MemoryProvider,
}

impl fmt::Debug for SectionRecall<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SectionRecall")
            .field("driver_id", &self.provider.driver_id())
            .finish()
    }
}

/// Refuse options that already pin a namespace.
fn reject_namespace_filter(opts: &OwnedRecallOpts) -> Result<(), MemoryError> {
    if opts.namespace.is_some() {
        return Err(MemoryError::Invalid(NAMESPACE_FILTER_CONFLICT.to_string()));
    }
    Ok(())
}

/// Refuse `cross_session` recall on any section other than
/// [`MemorySection::Conversation`].
///
/// See [`CROSS_SESSION_SECTION_CONFLICT`] for why: the bundled driver's
/// `cross_session` option only ever surfaces episodic conversational rows, and
/// relabels them with whatever namespace the call was pinned to — so honouring
/// it on a document or learning section would return conversational content
/// mislabeled as that section's own hits.
fn reject_cross_session_outside_conversation(
    section: &MemorySection,
    opts: &OwnedRecallOpts,
) -> Result<(), MemoryError> {
    if opts.cross_session && !matches!(section, MemorySection::Conversation) {
        return Err(MemoryError::Invalid(
            CROSS_SESSION_SECTION_CONFLICT.to_string(),
        ));
    }
    Ok(())
}

/// `opts` with `namespace` pinned to `namespace`.
fn pinned_to(opts: &OwnedRecallOpts, namespace: &str) -> OwnedRecallOpts {
    let mut pinned = opts.clone();
    pinned.namespace = Some(namespace.to_string());
    pinned
}

impl<'a> SectionRecall<'a> {
    /// Bind `provider`.
    #[must_use]
    pub fn new(provider: &'a dyn MemoryProvider) -> Self {
        Self { provider }
    }

    /// Recall within one scope of one section — a single provider call.
    ///
    /// Hits keep the order the driver returned them in: for one namespace that
    /// order *is* the driver's ranking, and re-sorting it here would discard
    /// whatever the engine knows and this façade does not.
    ///
    /// `sources` is the contract's per-turn source allowlist, passed straight
    /// through. It is named `sources` rather than `scope` because `scope` here
    /// means the namespace scope, and the two are unrelated.
    ///
    /// Note that a driver composed from the mandatory families **refuses** a
    /// `Some(sources)` recall outright — it cannot apply the predicate
    /// internally, and applying it afterwards would be wrong — so on those
    /// drivers only `None` succeeds. That refusal is the driver's, passed
    /// through unchanged rather than pre-empted here, so a driver that does
    /// implement source scoping is not held back by this façade.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Invalid`] in two cases: carrying
    /// [`NAMESPACE_FILTER_CONFLICT`] when `opts` already pins a namespace, and
    /// carrying the namespace validator's own message when the section and
    /// scope cannot form a valid namespace. Otherwise whatever the backend
    /// returns.
    pub async fn in_scope(
        &self,
        section: &MemorySection,
        scope: &str,
        query: &str,
        limit: usize,
        opts: &OwnedRecallOpts,
        sources: Option<&SourceScope>,
    ) -> Result<SectionHits, MemoryError> {
        reject_namespace_filter(opts)?;
        let namespace = SectionView::new(self.provider, section).namespace(scope)?;
        let hits = self
            .provider
            .recall(query, limit, &pinned_to(opts, namespace.as_str()), sources)
            .await?;
        Ok(SectionHits {
            hits,
            namespaces_searched: 1,
            truncated: false,
        })
    }

    /// Recall across every scope in a section.
    ///
    /// **Costs one namespace enumeration plus one recall per scope in the
    /// section**, up to [`MAX_SECTION_NAMESPACES`]. A caller who cannot afford
    /// that should use [`Self::in_scope`].
    ///
    /// The fan-out is not an optimisation to be replaced later by a single call
    /// with a section filter — the contract has no cross-namespace recall to
    /// build one on. `OwnedRecallOpts::namespace` is an exact match, and leaving
    /// it `None` means the `global` namespace on the embedded engine while
    /// meaning *every* namespace on the reference driver, so filtering the
    /// results of one unfiltered call would be correct in tests and empty in
    /// production. Asking each namespace by name is the only honest way to do
    /// this, and it is what the contract's own `list_everything` does for the
    /// same reason.
    ///
    /// Guarantees:
    ///
    /// - Namespaces are visited in [`SectionView::scopes`] order — entry count
    ///   descending, ties by namespace — so which ones the cap drops is
    ///   deterministic.
    /// - Each namespace is asked for the full `limit`, never a share of it: a
    ///   share would let one scope's best hit lose to another's worst.
    /// - Hits merge by score descending, absent scores last, ties by namespace
    ///   then key, and are then truncated to `limit`.
    /// - [`SectionHits::truncated`] means *namespaces were skipped*. Hits
    ///   reaching `limit` is ordinary and is not reported as truncation.
    /// - A section with no namespaces yields `Ok` with no hits and
    ///   `namespaces_searched: 0`, never an error.
    ///
    /// One caveat, stated rather than papered over: the scores being ranked come
    /// from separate calls. They are comparable in practice on every bundled
    /// driver, since it is one engine answering one query, but the contract does
    /// not guarantee that a score means the same thing across two calls.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Invalid`] carrying [`NAMESPACE_FILTER_CONFLICT`] when
    /// `opts` already pins a namespace. Otherwise whatever the backend returns
    /// from the enumeration or from any one recall — a section recall fails as a
    /// whole rather than reporting a partial answer as a success.
    pub async fn across_section(
        &self,
        section: &MemorySection,
        query: &str,
        limit: usize,
        opts: &OwnedRecallOpts,
        sources: Option<&SourceScope>,
    ) -> Result<SectionHits, MemoryError> {
        reject_namespace_filter(opts)?;
        let scopes = SectionView::new(self.provider, section).scopes().await?;
        let truncated = scopes.len() > MAX_SECTION_NAMESPACES;

        let mut gathered = Vec::new();
        let mut namespaces_searched = 0;
        for scope in scopes.into_iter().take(MAX_SECTION_NAMESPACES) {
            let pinned = pinned_to(opts, scope.namespace.as_str());
            gathered.extend(self.provider.recall(query, limit, &pinned, sources).await?);
            namespaces_searched += 1;
        }

        Ok(SectionHits {
            hits: merge_hits(gathered, limit),
            namespaces_searched,
            truncated,
        })
    }
}
