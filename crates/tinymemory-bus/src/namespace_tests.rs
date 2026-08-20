//! Tests for the `<section>:<scope>` namespace convention and its validator.

// A failed assertion in a test is a panic either way; `unwrap`/`expect` here say
// what the invariant was. Same allowance every other test module in this crate
// takes.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use super::*;

#[test]
fn a_section_helper_renders_the_canonical_form() {
    let ns = Namespace::conversation("thread-8f21").unwrap();
    assert_eq!(ns.as_str(), "conversation:thread-8f21");
    assert_eq!(ns.section(), Some(&MemorySection::Conversation));
    assert_eq!(ns.scope(), "thread-8f21");
    assert!(ns.is_sectioned());
}

#[test]
fn every_section_helper_uses_its_own_prefix() {
    for (rendered, section) in [
        (
            Namespace::conversation("x").unwrap(),
            MemorySection::Conversation,
        ),
        (Namespace::document("x").unwrap(), MemorySection::Document),
        (Namespace::learning("x").unwrap(), MemorySection::Learning),
        (Namespace::entity("x").unwrap(), MemorySection::Entity),
        (Namespace::profile("x").unwrap(), MemorySection::Profile),
        (Namespace::tool("x").unwrap(), MemorySection::Tool),
        (Namespace::source("x").unwrap(), MemorySection::Source),
    ] {
        assert_eq!(rendered.as_str(), format!("{}:x", section.as_str()));
        assert!(rendered.is_in(&section));
    }
}

#[test]
fn parsing_recovers_the_section_and_scope() {
    let ns = Namespace::parse("learning:rust-async").unwrap();
    assert_eq!(ns.section(), Some(&MemorySection::Learning));
    assert_eq!(ns.scope(), "rust-async");
}

#[test]
fn a_scope_may_contain_colons_and_still_round_trips() {
    let ns = Namespace::parse("document:acme:handbook:v2").unwrap();
    assert_eq!(ns.section(), Some(&MemorySection::Document));
    assert_eq!(ns.scope(), "acme:handbook:v2");
    assert_eq!(
        Namespace::parse(ns.as_str()).unwrap().scope(),
        "acme:handbook:v2"
    );
}

#[test]
fn an_unrecognised_prefix_becomes_a_custom_section() {
    let ns = Namespace::parse("audit:2026-q1").unwrap();
    assert_eq!(
        ns.section(),
        Some(&MemorySection::Custom("audit".to_string()))
    );
    assert_eq!(ns.scope(), "2026-q1");
    assert!(!ns.section().unwrap().is_known());
}

#[test]
fn a_bare_name_parses_as_unsectioned_and_renders_verbatim() {
    let ns = Namespace::parse("research-notes").unwrap();
    assert!(ns.section().is_none());
    assert!(!ns.is_sectioned());
    assert_eq!(ns.scope(), "research-notes");
    assert_eq!(ns.as_str(), "research-notes");
}

#[test]
fn a_path_shaped_legacy_name_stays_unsectioned() {
    // The prefix rules exclude '/' and '.', so the first segment of a
    // path-shaped name is not mistaken for a section.
    let ns = Namespace::parse("projects/acme/notes").unwrap();
    assert!(ns.section().is_none());
    assert_eq!(ns.as_str(), "projects/acme/notes");
}

#[test]
fn an_uppercase_prefix_is_not_a_section() {
    let ns = Namespace::parse("Document:handbook").unwrap();
    assert!(
        ns.section().is_none(),
        "a section vocabulary with two spellings is not a vocabulary"
    );
    assert_eq!(ns.as_str(), "Document:handbook");
}

#[test]
fn a_trailing_colon_with_no_scope_is_not_a_section() {
    let ns = Namespace::parse("document:").unwrap();
    assert!(ns.section().is_none());
    assert_eq!(ns.as_str(), "document:");
}

#[test]
fn unsectioned_rejects_a_name_that_carries_a_prefix() {
    let error = Namespace::unsectioned("document:handbook").unwrap_err();
    assert!(matches!(error, MemoryError::Invalid(_)), "got {error:?}");
}

#[test]
fn unsectioned_accepts_a_bare_name() {
    assert_eq!(
        Namespace::unsectioned("research-notes").unwrap().as_str(),
        "research-notes"
    );
}

#[test]
fn an_empty_namespace_is_rejected() {
    assert!(matches!(
        Namespace::parse("").unwrap_err(),
        MemoryError::Invalid(_)
    ));
}

#[test]
fn an_empty_scope_is_rejected_by_the_builders() {
    assert!(matches!(
        Namespace::conversation("").unwrap_err(),
        MemoryError::Invalid(_)
    ));
}

#[test]
fn an_overlong_namespace_is_rejected() {
    let long = "a".repeat(MAX_NAMESPACE_LEN + 1);
    let error = Namespace::parse(&long).unwrap_err();
    assert!(error.to_string().contains("limit"), "got {error}");
}

#[test]
fn a_namespace_of_exactly_the_limit_is_accepted() {
    let at_limit = "a".repeat(MAX_NAMESPACE_LEN);
    assert!(Namespace::parse(&at_limit).is_ok());
}

#[test]
fn whitespace_and_control_characters_are_rejected() {
    for bad in ["has space", "tab\there", "new\nline", "nul\0byte"] {
        assert!(
            matches!(Namespace::parse(bad), Err(MemoryError::Invalid(_))),
            "{bad:?} should be rejected"
        );
    }
}

#[test]
fn a_traversal_segment_is_rejected_rather_than_sanitised() {
    for bad in ["../etc", "a/../b", "document:a/../../b"] {
        let error = Namespace::parse(bad).unwrap_err();
        assert!(error.to_string().contains(".."), "{bad:?} gave {error}");
    }
}

#[test]
fn a_dotted_segment_that_is_not_traversal_is_allowed() {
    assert!(Namespace::parse("document:v1.2.3").is_ok());
    assert!(Namespace::parse("a/..b/c").is_ok());
}

#[test]
fn a_leading_or_trailing_slash_is_rejected() {
    assert!(Namespace::parse("/absolute").is_err());
    assert!(Namespace::parse("trailing/").is_err());
}

#[test]
fn a_custom_section_can_be_built_and_round_trips() {
    let ns = Namespace::new(MemorySection::Custom("audit".into()), "2026-q1").unwrap();
    assert_eq!(ns.as_str(), "audit:2026-q1");
    assert_eq!(Namespace::parse(ns.as_str()).unwrap(), ns);
}

#[test]
fn a_custom_section_with_an_invalid_prefix_is_rejected() {
    let error = Namespace::new(MemorySection::Custom("Audit Log".into()), "x").unwrap_err();
    assert!(matches!(error, MemoryError::Invalid(_)), "got {error:?}");
}

#[test]
fn flatten_swaps_the_separator_for_a_store_that_cannot_hold_a_colon() {
    let ns = Namespace::document("handbook").unwrap();
    assert_eq!(ns.flatten("__"), "document__handbook");
    assert_eq!(ns.flatten("/"), "document/handbook");
    assert_eq!(ns.flatten(":"), ns.as_str());
}

#[test]
fn flatten_leaves_an_unsectioned_name_alone() {
    let ns = Namespace::unsectioned("research-notes").unwrap();
    assert_eq!(ns.flatten("__"), "research-notes");
}

#[test]
fn known_lists_the_closed_vocabulary_and_excludes_custom() {
    let known = MemorySection::known();
    assert_eq!(known.len(), 7);
    assert!(known.iter().all(MemorySection::is_known));
    assert!(known.contains(&MemorySection::Conversation));
    assert!(known.contains(&MemorySection::Learning));
}

#[test]
fn every_known_prefix_maps_back_to_its_own_section() {
    for section in MemorySection::known() {
        assert_eq!(MemorySection::from_prefix(section.as_str()), section);
    }
}

#[test]
fn a_namespace_serializes_as_its_canonical_string() {
    let ns = Namespace::learning("rust-async").unwrap();
    assert_eq!(
        serde_json::to_string(&ns).unwrap(),
        "\"learning:rust-async\""
    );
    let decoded: Namespace = serde_json::from_str("\"learning:rust-async\"").unwrap();
    assert_eq!(decoded, ns);
}

#[test]
fn deserializing_an_invalid_namespace_fails() {
    assert!(serde_json::from_str::<Namespace>("\"has space\"").is_err());
}

#[test]
fn a_namespace_parses_through_from_str_and_displays_back() {
    let ns: Namespace = "document:handbook".parse().unwrap();
    assert_eq!(ns.to_string(), "document:handbook");
    assert_eq!(ns.as_ref(), "document:handbook");
}

#[test]
fn is_in_distinguishes_sections() {
    let ns = Namespace::document("handbook").unwrap();
    assert!(ns.is_in(&MemorySection::Document));
    assert!(!ns.is_in(&MemorySection::Conversation));
}

#[test]
fn validate_name_accepts_the_characters_engines_actually_need() {
    for good in [
        "conversation:thread-8f21",
        "user@example.com",
        "a+b",
        "projects/acme/notes",
        "v1.2.3",
    ] {
        assert!(validate_name(good).is_ok(), "{good:?} should be allowed");
    }
}
