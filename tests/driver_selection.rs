//! Driver admission: which ids exist, what class each binds as, and what is
//! refused.
//!
//! Exercises only the public surface of the `tinymemory` facade.
//!
//! # Scope note
//!
//! Issue #18 §E3 describes this file as also asserting that "the bound
//! provider's `driver_id()` matches" the configured id. That step needs
//! `MemoryHostConfig::memory_provider()` to actually select an engine, which is
//! §A5 and does not exist yet — `create_memory_client_with_local_ai` still
//! constructs TinyCortex unconditionally. The issue's own sequencing says to
//! write these tests "against the *current* behaviour first", so this file
//! pins what admission does today. The binding half joins it when §A5 lands,
//! and this file is where it goes.

// A failing assertion in a test *is* a panic; the crate-wide `expect_used` /
// `unwrap_used` / `panic` lints exist to keep the library from panicking, not
// the tests. Same allowance, and same reasoning, as `src/registry/test.rs`.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use tinymemory::registry::{
    ConfigLabels, DriverClass, DriverEntry, DriverRegistry, COGNEE_DRIVER_ID, MEM0_DRIVER_ID,
    SUPERMEMORY_DRIVER_ID, TINYCORTEX_DRIVER_ID, TRUSTED,
};

fn labels() -> ConfigLabels<'static> {
    ConfigLabels {
        section: "[memory]",
        drivers: "[memory.drivers]",
        driver_entry: "[memory.drivers.<id>]",
    }
}

fn trusted_external() -> DriverEntry<'static> {
    DriverEntry {
        class: None,
        trust_state: TRUSTED,
    }
}

#[test]
fn a_reserved_embedded_id_is_admitted_without_any_config_entry() {
    // The embedded default's options live in the host's own config blocks, so
    // it must not require a `drivers` entry to be selectable at all.
    let admission = DriverRegistry::builtin()
        .admit(TINYCORTEX_DRIVER_ID, None, labels())
        .expect("the built-in embedded engine is admitted");
    assert_eq!(admission.id, TINYCORTEX_DRIVER_ID);
    assert_eq!(admission.class, DriverClass::Embedded);
}

#[test]
fn the_null_driver_is_admitted_and_is_class_null() {
    let admission = DriverRegistry::builtin()
        .admit(tinymemory::registry::NULL_DRIVER_ID, None, labels())
        .expect("the null driver is admitted");
    assert_eq!(admission.class, DriverClass::Null);
}

#[test]
fn every_reserved_external_id_resolves_to_the_external_class() {
    let registry = DriverRegistry::builtin();
    for id in [SUPERMEMORY_DRIVER_ID, MEM0_DRIVER_ID, COGNEE_DRIVER_ID] {
        let admission = registry
            .admit(id, Some(trusted_external()), labels())
            .unwrap_or_else(|reason| panic!("{id} was refused: {}", reason.reason));
        assert_eq!(admission.class, DriverClass::External, "{id}");
        assert_eq!(admission.id, id);
    }
}

#[test]
fn an_external_driver_without_an_entry_is_refused_fail_closed() {
    // The fail-closed half: an external engine needs endpoint, credential and
    // trust configuration, so admitting it implicitly would bind an
    // out-of-process backend nobody configured.
    let reason = DriverRegistry::builtin()
        .admit(SUPERMEMORY_DRIVER_ID, None, labels())
        .expect_err("an external driver with no entry must be refused");
    assert_eq!(reason.configured_driver, SUPERMEMORY_DRIVER_ID);
    assert!(
        reason.reason.contains("external"),
        "the refusal should say why: {}",
        reason.reason
    );
}

#[test]
fn an_untrusted_external_driver_is_refused_even_with_an_entry() {
    let entry = DriverEntry {
        class: None,
        trust_state: "untrusted",
    };
    let reason = DriverRegistry::builtin()
        .admit(SUPERMEMORY_DRIVER_ID, Some(entry), labels())
        .expect_err("trust must be raised explicitly before an external bind");
    assert!(
        reason.reason.contains(TRUSTED),
        "the refusal should name the value to set: {}",
        reason.reason
    );
}

#[test]
fn a_reserved_id_cannot_have_its_class_overridden_by_config() {
    // A reserved id names a fixed implementation. An explicit `class` line may
    // confirm it but never override it — otherwise config could run the
    // embedded engine under the checks meant for an external one.
    let entry = DriverEntry {
        class: Some("external"),
        trust_state: TRUSTED,
    };
    let reason = DriverRegistry::builtin()
        .admit(TINYCORTEX_DRIVER_ID, Some(entry), labels())
        .expect_err("a reserved id's class must not be overridable");
    assert!(
        reason.reason.contains("built in"),
        "the refusal should explain why: {}",
        reason.reason
    );
}

#[test]
fn an_unknown_driver_id_is_refused_rather_than_defaulted() {
    let reason = DriverRegistry::builtin()
        .admit("not-an-engine", None, labels())
        .expect_err("an unreserved id with no entry must be refused");
    assert_eq!(reason.configured_driver, "not-an-engine");
}

#[test]
fn an_empty_driver_id_is_refused() {
    let reason = DriverRegistry::builtin()
        .admit("   ", None, labels())
        .expect_err("a blank driver id must be refused");
    assert!(
        reason.reason.contains("empty"),
        "the refusal should name the problem: {}",
        reason.reason
    );
}

#[test]
fn a_config_class_typo_is_echoed_back_to_the_operator() {
    // The offending value comes from the host's own config file, not from a
    // driver or the network, so echoing it discloses nothing the reader did not
    // write — and without it the message cannot point at the line to fix.
    let entry = DriverEntry {
        class: Some("embeded"),
        trust_state: TRUSTED,
    };
    let reason = DriverRegistry::builtin()
        .admit("some-driver", Some(entry), labels())
        .expect_err("an unparseable class must be refused");
    assert!(
        reason.reason.contains("embeded"),
        "the refusal should quote the typo: {}",
        reason.reason
    );
}
