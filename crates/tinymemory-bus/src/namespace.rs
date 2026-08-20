//! The namespace naming convention: `<section>:<scope>`.
//!
//! Namespaces are the only partitioning primitive this contract has, and every
//! family takes them as a bare `&str`. That is deliberate — a driver's
//! container vocabulary is its own, and a typed namespace threaded through
//! eighteen trait families would force every engine to agree on a shape none of
//! them share. What was missing was not a type in the signatures but a *shared
//! convention* for what goes in the string, so that "conversational memory",
//! "document memory", and "learnings" mean the same thing to every caller and
//! every engine instead of being three ad-hoc prefixes per host.
//!
//! ## The convention
//!
//! ```text
//! conversation:thread-8f21     a single conversation
//! document:handbook            a document collection
//! learning:rust-async          a topic the agent has learned about
//! entity:people                an entity index slice
//! profile:default              user-state facets
//! tool:github                  tool-scoped rules and outcomes
//! research-notes               unsectioned — legacy, still valid
//! ```
//!
//! The section is a closed vocabulary ([`MemorySection`]) plus an escape hatch
//! ([`MemorySection::Custom`]); the scope is free-form within the character
//! rules below. Splitting happens at the **first** colon, so a scope may
//! contain colons of its own and still round-trip.
//!
//! ## Unsectioned names stay valid
//!
//! A name with no recognised prefix parses as an unsectioned namespace and
//! renders back byte-for-byte. Every namespace written before this convention
//! existed keeps working, and nothing here silently rewrites a caller's string
//! — [`Namespace::parse`] is the only thing that interprets it, and it is a
//! caller's choice to run it.
//!
//! ## What this is not
//!
//! Not a permission boundary, and not a mapping table. A driver that cannot
//! store a colon should render a namespace with [`Namespace::flatten`] at its
//! own boundary; the contract does not decide that for it, because only the
//! driver knows what its store accepts.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::MemoryError;

/// Longest namespace string this convention accepts, in bytes.
///
/// Chosen to sit under the shortest limit among the engines this workspace
/// adapts rather than at any one engine's ceiling: a name that validates here
/// must be storable everywhere, otherwise validation would pass and the write
/// would fail, which is the worst of both.
pub const MAX_NAMESPACE_LEN: usize = 200;

/// What kind of memory a namespace holds.
///
/// A closed vocabulary so that two hosts, two engines, and an agent tool
/// description all name the same thing the same way — plus
/// [`MemorySection::Custom`], because a closed vocabulary with no escape hatch
/// gets worked around with prefixes nobody agrees on, which is the problem this
/// type exists to solve.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemorySection {
    /// Turn-by-turn conversational memory. Wire prefix `conversation`.
    Conversation,
    /// Whole documents and the chunks they were split into. Wire prefix
    /// `document`.
    Document,
    /// Durable conclusions the agent drew and expects to reuse. Wire prefix
    /// `learning`.
    Learning,
    /// The entity index — who and what the memory is about. Wire prefix
    /// `entity`.
    Entity,
    /// User-state facets and preferences. Wire prefix `profile`.
    Profile,
    /// Tool-scoped rules and remembered outcomes. Wire prefix `tool`.
    Tool,
    /// Content pulled in from an external source. Wire prefix `source`.
    Source,
    /// A section this vocabulary does not name, carried verbatim.
    Custom(String),
}

impl MemorySection {
    /// The wire prefix for this section.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Conversation => "conversation",
            Self::Document => "document",
            Self::Learning => "learning",
            Self::Entity => "entity",
            Self::Profile => "profile",
            Self::Tool => "tool",
            Self::Source => "source",
            Self::Custom(name) => name,
        }
    }

    /// Every section in the closed vocabulary, in declaration order.
    ///
    /// [`MemorySection::Custom`] is absent by construction: it has no fixed
    /// spelling to list.
    pub fn known() -> [MemorySection; 7] {
        [
            Self::Conversation,
            Self::Document,
            Self::Learning,
            Self::Entity,
            Self::Profile,
            Self::Tool,
            Self::Source,
        ]
    }

    /// Whether this section is one the vocabulary names.
    pub fn is_known(&self) -> bool {
        !matches!(self, Self::Custom(_))
    }

    /// Map a prefix onto a section, falling back to
    /// [`MemorySection::Custom`].
    ///
    /// Never fails: an unrecognised prefix is a custom section, not an error,
    /// because a host that invents one is doing exactly what the escape hatch
    /// is for.
    pub fn from_prefix(prefix: &str) -> Self {
        match prefix {
            "conversation" => Self::Conversation,
            "document" => Self::Document,
            "learning" => Self::Learning,
            "entity" => Self::Entity,
            "profile" => Self::Profile,
            "tool" => Self::Tool,
            "source" => Self::Source,
            other => Self::Custom(other.to_string()),
        }
    }
}

impl fmt::Display for MemorySection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A validated namespace name, optionally carrying a [`MemorySection`].
///
/// Construct one with a section helper ([`Namespace::conversation`], …) or by
/// parsing an existing string, then hand [`Namespace::as_str`] to any contract
/// method that takes a namespace.
///
/// # Examples
///
/// ```
/// use tinymemory_bus::namespace::{MemorySection, Namespace};
///
/// let ns = Namespace::conversation("thread-8f21")?;
/// assert_eq!(ns.as_str(), "conversation:thread-8f21");
/// assert_eq!(ns.section(), Some(&MemorySection::Conversation));
/// assert_eq!(ns.scope(), "thread-8f21");
///
/// // A name written before the convention existed still parses, and renders
/// // back byte-for-byte.
/// let legacy = Namespace::parse("research-notes")?;
/// assert!(legacy.section().is_none());
/// assert_eq!(legacy.as_str(), "research-notes");
/// # Ok::<(), tinymemory_bus::error::MemoryError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Namespace {
    section: Option<MemorySection>,
    scope: String,
    rendered: String,
}

impl Namespace {
    /// Parse and validate a namespace string.
    ///
    /// Splits at the first colon. A name with no colon, or whose prefix fails
    /// the section character rules, is an unsectioned namespace rather than an
    /// error.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Invalid`] when the name is empty, longer than
    /// [`MAX_NAMESPACE_LEN`], contains a character outside the allowed set, or
    /// contains a `..` path-traversal segment.
    pub fn parse(raw: &str) -> Result<Self, MemoryError> {
        validate_name(raw)?;
        match raw.split_once(':') {
            Some((prefix, scope)) if is_valid_section(prefix) && !scope.is_empty() => Ok(Self {
                section: Some(MemorySection::from_prefix(prefix)),
                scope: scope.to_string(),
                rendered: raw.to_string(),
            }),
            _ => Ok(Self {
                section: None,
                scope: raw.to_string(),
                rendered: raw.to_string(),
            }),
        }
    }

    /// Build a namespace in `section` with `scope`.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Invalid`] when the section prefix or the resulting name
    /// fails validation.
    pub fn new(section: MemorySection, scope: impl Into<String>) -> Result<Self, MemoryError> {
        let scope = scope.into();
        if !is_valid_section(section.as_str()) {
            return Err(MemoryError::Invalid(format!(
                "namespace section {:?} must be lowercase letters, digits, '-' or '_'",
                section.as_str()
            )));
        }
        if scope.is_empty() {
            return Err(MemoryError::Invalid(
                "namespace scope must not be empty".to_string(),
            ));
        }
        let rendered = format!("{}:{scope}", section.as_str());
        validate_name(&rendered)?;
        Ok(Self {
            section: Some(section),
            scope,
            rendered,
        })
    }

    /// A namespace with no section prefix.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Invalid`] as [`Namespace::parse`]; additionally rejects a
    /// name that *would* parse as sectioned, because silently accepting one
    /// here would make `unsectioned("document:x").section()` return `Some`.
    pub fn unsectioned(raw: impl Into<String>) -> Result<Self, MemoryError> {
        let raw = raw.into();
        let parsed = Self::parse(&raw)?;
        if parsed.section.is_some() {
            return Err(MemoryError::Invalid(format!(
                "namespace {raw:?} carries a section prefix; use Namespace::parse"
            )));
        }
        Ok(parsed)
    }

    /// Turn-by-turn conversational memory for one conversation.
    ///
    /// # Errors
    ///
    /// As [`Namespace::new`].
    pub fn conversation(scope: impl Into<String>) -> Result<Self, MemoryError> {
        Self::new(MemorySection::Conversation, scope)
    }

    /// A document collection.
    ///
    /// # Errors
    ///
    /// As [`Namespace::new`].
    pub fn document(scope: impl Into<String>) -> Result<Self, MemoryError> {
        Self::new(MemorySection::Document, scope)
    }

    /// Durable conclusions about one topic.
    ///
    /// # Errors
    ///
    /// As [`Namespace::new`].
    pub fn learning(scope: impl Into<String>) -> Result<Self, MemoryError> {
        Self::new(MemorySection::Learning, scope)
    }

    /// A slice of the entity index.
    ///
    /// # Errors
    ///
    /// As [`Namespace::new`].
    pub fn entity(scope: impl Into<String>) -> Result<Self, MemoryError> {
        Self::new(MemorySection::Entity, scope)
    }

    /// User-state facets.
    ///
    /// # Errors
    ///
    /// As [`Namespace::new`].
    pub fn profile(scope: impl Into<String>) -> Result<Self, MemoryError> {
        Self::new(MemorySection::Profile, scope)
    }

    /// Tool-scoped rules and outcomes.
    ///
    /// # Errors
    ///
    /// As [`Namespace::new`].
    pub fn tool(scope: impl Into<String>) -> Result<Self, MemoryError> {
        Self::new(MemorySection::Tool, scope)
    }

    /// Content pulled in from one external source.
    ///
    /// # Errors
    ///
    /// As [`Namespace::new`].
    pub fn source(scope: impl Into<String>) -> Result<Self, MemoryError> {
        Self::new(MemorySection::Source, scope)
    }

    /// The section this namespace belongs to, or `None` when unsectioned.
    pub fn section(&self) -> Option<&MemorySection> {
        self.section.as_ref()
    }

    /// The part after the section prefix — or the whole name when unsectioned.
    pub fn scope(&self) -> &str {
        &self.scope
    }

    /// The canonical `<section>:<scope>` string to pass to a contract method.
    pub fn as_str(&self) -> &str {
        &self.rendered
    }

    /// Whether this namespace carries a section prefix.
    pub fn is_sectioned(&self) -> bool {
        self.section.is_some()
    }

    /// Whether this namespace is in `section`.
    pub fn is_in(&self, section: &MemorySection) -> bool {
        self.section.as_ref() == Some(section)
    }

    /// Render with `separator` in place of the colon.
    ///
    /// For a store that cannot hold a colon in a container name. The result is
    /// **not** parseable back into a [`Namespace`] unless `separator` is `":"`
    /// — it is an output format for a driver boundary, not a second canonical
    /// form.
    ///
    /// # Examples
    ///
    /// ```
    /// use tinymemory_bus::namespace::Namespace;
    ///
    /// let ns = Namespace::document("handbook")?;
    /// assert_eq!(ns.flatten("__"), "document__handbook");
    /// # Ok::<(), tinymemory_bus::error::MemoryError>(())
    /// ```
    pub fn flatten(&self, separator: &str) -> String {
        match &self.section {
            Some(section) => format!("{}{separator}{}", section.as_str(), self.scope),
            None => self.scope.clone(),
        }
    }
}

impl fmt::Display for Namespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.rendered)
    }
}

impl AsRef<str> for Namespace {
    fn as_ref(&self) -> &str {
        &self.rendered
    }
}

impl std::str::FromStr for Namespace {
    type Err = MemoryError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        Self::parse(raw)
    }
}

impl TryFrom<String> for Namespace {
    type Error = MemoryError;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(&raw)
    }
}

impl From<Namespace> for String {
    fn from(namespace: Namespace) -> Self {
        namespace.rendered
    }
}

/// Whether `prefix` may act as a section label.
///
/// Deliberately narrower than the scope rules: a section is a vocabulary word,
/// so `Document` and `document` must not be two sections, and a prefix
/// containing a slash or a dot is far more likely to be the first segment of an
/// unsectioned path-shaped name than a section anyone meant.
fn is_valid_section(prefix: &str) -> bool {
    !prefix.is_empty()
        && prefix
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// Validate a whole namespace string against the character and length rules.
///
/// # Errors
///
/// [`MemoryError::Invalid`] naming the specific rule that failed, so a caller
/// can show the user which one rather than "invalid namespace".
pub fn validate_name(raw: &str) -> Result<(), MemoryError> {
    if raw.is_empty() {
        return Err(MemoryError::Invalid(
            "namespace must not be empty".to_string(),
        ));
    }
    if raw.len() > MAX_NAMESPACE_LEN {
        return Err(MemoryError::Invalid(format!(
            "namespace is {} bytes, over the {MAX_NAMESPACE_LEN}-byte limit",
            raw.len()
        )));
    }
    if let Some(bad) = raw.chars().find(|c| !is_allowed_char(*c)) {
        return Err(MemoryError::Invalid(format!(
            "namespace contains disallowed character {bad:?}"
        )));
    }
    // Namespaces reach engines that map them onto directories. A traversal
    // segment is rejected here rather than sanitised, because sanitising would
    // silently change which container a write lands in.
    if raw.split('/').any(|segment| segment == "..") {
        return Err(MemoryError::Invalid(
            "namespace must not contain a '..' segment".to_string(),
        ));
    }
    if raw.starts_with('/') || raw.ends_with('/') {
        return Err(MemoryError::Invalid(
            "namespace must not start or end with '/'".to_string(),
        ));
    }
    Ok(())
}

/// Whether `c` may appear anywhere in a namespace.
///
/// ASCII only, and no whitespace: a namespace is a key that ends up in URLs,
/// file paths, and SQL parameters across several engines, and every character
/// outside this set is one of those engines' escaping problem.
fn is_allowed_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '@' | '+')
}

#[cfg(test)]
#[path = "namespace_tests.rs"]
mod tests;
