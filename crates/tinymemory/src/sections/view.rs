//! [`SectionView`] — one section's slice of a provider, addressed by scope.

use std::fmt;

use tinymemory_api::error::MemoryError;
use tinymemory_api::namespace::{MemorySection, Namespace};
use tinymemory_api::provider::MemoryProvider;
use tinymemory_api::types::{MemoryCategory, MemoryEntry, MemoryTaint};

use super::types::SectionScope;

/// A borrowing handle onto one [`MemorySection`] of a provider.
///
/// Every method takes the **scope** — `"thread-8f21"`, not
/// `"conversation:thread-8f21"` — and builds the namespace itself, so a caller
/// never spells the convention out and a scope that cannot form a valid
/// namespace is refused before anything is written.
///
/// The handle borrows rather than owning an `Arc`, matching `DocumentIntake`:
/// it is cheap to make, cheap to drop, and cannot outlive the provider it reads.
#[derive(Clone)]
pub struct SectionView<'a> {
    provider: &'a dyn MemoryProvider,
    section: MemorySection,
}

impl fmt::Debug for SectionView<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SectionView")
            .field("section", &self.section.as_str())
            .field("driver_id", &self.provider.driver_id())
            .finish()
    }
}

impl<'a> SectionView<'a> {
    /// Bind `section` of `provider`.
    ///
    /// The section is taken by value because a [`MemorySection::Custom`] owns
    /// its name, and borrowing one would make [`Sections::section`] unable to
    /// hand back a view over a section the caller built inline.
    ///
    /// [`Sections::section`]: super::Sections::section
    #[must_use]
    pub fn new(provider: &'a dyn MemoryProvider, section: MemorySection) -> Self {
        Self { provider, section }
    }

    /// The section this view is bound to.
    #[must_use]
    pub fn section(&self) -> &MemorySection {
        &self.section
    }

    /// The namespace `scope` names within this section.
    ///
    /// Useful on its own for a caller that needs the string a write *would*
    /// touch — an audit line, a log field — without performing the write.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Invalid`] when the section and scope cannot form a valid
    /// namespace: an empty scope, a disallowed character, or a rendered name
    /// over the convention's length limit.
    pub fn namespace(&self, scope: &str) -> Result<Namespace, MemoryError> {
        Namespace::new(self.section.clone(), scope)
    }

    /// Store one entry under `scope`, returning the namespace it landed in.
    ///
    /// The parameters after `key` mirror `MemoryCore::store` exactly, so what
    /// this adds over the raw call is visible: the namespace, and nothing else.
    ///
    /// Returns the [`Namespace`] rather than `()` so a caller that wants to log
    /// or audit where the entry went does not have to re-derive it.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Invalid`] for a scope [`Self::namespace`] rejects — in
    /// which case **nothing is stored** — otherwise whatever the backend returns.
    pub async fn put(
        &self,
        scope: &str,
        key: &str,
        content: &str,
        category: MemoryCategory,
        session_id: Option<&str>,
        taint: MemoryTaint,
    ) -> Result<Namespace, MemoryError> {
        let namespace = self.namespace(scope)?;
        self.provider
            .store(namespace.as_str(), key, content, category, session_id, taint)
            .await?;
        Ok(namespace)
    }

    /// Read one entry back by `(scope, key)`.
    ///
    /// # Errors
    ///
    /// As [`Self::put`]. A missing entry is `Ok(None)`, not an error.
    pub async fn get(&self, scope: &str, key: &str) -> Result<Option<MemoryEntry>, MemoryError> {
        let namespace = self.namespace(scope)?;
        self.provider.get(namespace.as_str(), key).await
    }

    /// Delete one entry, reporting whether it existed.
    ///
    /// # Errors
    ///
    /// As [`Self::put`]. Forgetting an absent entry is `Ok(false)`.
    pub async fn forget(&self, scope: &str, key: &str) -> Result<bool, MemoryError> {
        let namespace = self.namespace(scope)?;
        self.provider.forget(namespace.as_str(), key).await
    }

    /// List the entries in one scope.
    ///
    /// # Errors
    ///
    /// As [`Self::put`].
    pub async fn list(
        &self,
        scope: &str,
        category: Option<&MemoryCategory>,
        session_id: Option<&str>,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        let namespace = self.namespace(scope)?;
        self.provider
            .list(Some(namespace.as_str()), category, session_id)
            .await
    }

    /// Every scope this section currently holds.
    ///
    /// Ordered by entry count descending, ties by namespace ascending — the same
    /// order [`across_section`](super::SectionRecall::across_section) visits them in, so the
    /// first [`MAX_SECTION_NAMESPACES`](super::MAX_SECTION_NAMESPACES) rows here are exactly the ones a section-wide
    /// recall would search.
    ///
    /// Namespaces belonging to another section, and unsectioned namespaces left
    /// over from before the convention existed, are excluded. So are namespaces
    /// the convention cannot parse at all: a driver may hold names this
    /// vocabulary does not admit, and refusing to list a section because one
    /// unrelated name is malformed would be the wrong failure.
    ///
    /// # Errors
    ///
    /// Whatever the backend returns from its namespace enumeration.
    pub async fn scopes(&self) -> Result<Vec<SectionScope>, MemoryError> {
        let mut scopes: Vec<SectionScope> = self
            .provider
            .namespaces()
            .await?
            .into_iter()
            .filter_map(|summary| {
                let namespace = Namespace::parse(&summary.namespace).ok()?;
                if namespace.section() != Some(&self.section) {
                    return None;
                }
                Some(SectionScope {
                    namespace,
                    entries: summary.count,
                    last_updated: summary.last_updated,
                })
            })
            .collect();
        scopes.sort_by(|a, b| {
            b.entries
                .cmp(&a.entries)
                .then_with(|| a.namespace.cmp(&b.namespace))
        });
        Ok(scopes)
    }

    /// List every entry in the section, across all of its scopes.
    ///
    /// Costs one namespace enumeration plus one `list` per scope, and unlike
    /// [`across_section`](super::SectionRecall::across_section) it is **not capped** — a caller asking
    /// to list a section gets all of it. Use [`Self::scopes`] and [`Self::list`]
    /// to page through a large section under your own control.
    ///
    /// Ordered by namespace then key, so the result is deterministic even though
    /// the per-scope order the driver returns is not specified.
    ///
    /// # Errors
    ///
    /// As [`Self::scopes`], plus whatever any per-scope `list` returns.
    pub async fn list_section(
        &self,
        category: Option<&MemoryCategory>,
        session_id: Option<&str>,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        let mut entries = Vec::new();
        for scope in self.scopes().await? {
            entries.extend(
                self.provider
                    .list(Some(scope.namespace.as_str()), category, session_id)
                    .await?,
            );
        }
        entries.sort_by(|a, b| a.namespace.cmp(&b.namespace).then_with(|| a.key.cmp(&b.key)));
        Ok(entries)
    }
}
