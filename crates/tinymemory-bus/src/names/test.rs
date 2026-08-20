//! Tests for the member-name table.
//!
//! These are pinning tests, not behavioural ones. A member name is a string
//! that only fails at runtime, in a host, as an `UnknownMethod` — so the value
//! here is in catching a typo or a duplicate at `cargo test` time in this
//! crate, before the module or a host ever sees it.

use super::{methods, BUS_NAME, METHODS, OBJECT_PATH};

#[test]
fn the_object_identity_is_pinned() {
    // Changing either of these breaks every deployed host at once, so they are
    // spelled out here rather than derived from anything.
    assert_eq!(BUS_NAME, "ai.tinyhumans.tinymemory.Memory");
    assert_eq!(OBJECT_PATH, "/ai/tinyhumans/tinymemory/Memory");
}

#[test]
fn no_member_name_appears_twice() {
    let mut sorted = METHODS;
    sorted.sort_unstable();
    let mut unique = sorted.to_vec();
    unique.dedup();
    assert_eq!(
        unique.len(),
        METHODS.len(),
        "a member name is listed more than once"
    );
}

#[test]
fn every_member_name_is_pascal_case() {
    // `#[tinybus::interface]` derives a member from its method identifier with
    // `pascal_case`, so anything else in this table is a hand-written name that
    // will not match what the module actually serves.
    for member in METHODS {
        let mut chars = member.chars();
        let first = chars.next().unwrap_or('_');
        assert!(
            first.is_ascii_uppercase(),
            "{member} does not start with an uppercase letter"
        );
        assert!(
            member.chars().all(|c| c.is_ascii_alphanumeric()),
            "{member} is not alphanumeric"
        );
    }
}

#[test]
fn the_constants_and_the_table_are_the_same_set() {
    // A spot check in both directions: a constant that is not in the table
    // would be invisible to the module's drift assertion, and a table entry
    // with no constant is a name a caller has to spell by hand.
    assert!(METHODS.contains(&methods::STORE));
    assert!(METHODS.contains(&methods::OPEN_STORE));
    assert!(METHODS.contains(&methods::WORKFLOW_IDENTITY_MATCHES));
    assert_eq!(methods::STORE, "Store");
    assert_eq!(methods::OPEN_STORE, "OpenStore");
    assert_eq!(
        methods::WORKFLOW_IDENTITY_MATCHES,
        "WorkflowIdentityMatches"
    );
}
