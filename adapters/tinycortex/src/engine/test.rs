//! Capability honesty for the full engine provider.
//!
//! The point of lifting the optional families here (issue #18 §C3) is that a
//! host filtering its surface from a negotiated capability set gets the whole
//! engine rather than the mandatory third of it. That is only safe if the set
//! is true.
//!
//! These assert the rule directly rather than through a constructed provider.
//! Construction needs a `MemoryClient`, which needs the host's process-global
//! seams (`set_embedding_host` and friends) installed — and a test that installs
//! a process global is order-dependent, which `AGENTS.md` rules out. The
//! provider-level check that `capabilities()` equals the reachable accessors is
//! `audit_provider`, and it runs against a real engine in the conformance suite
//! once a host has wired those seams.

#![allow(clippy::expect_used, clippy::panic)]

use tinymemory_api::capabilities::{Capabilities, Capability};

use super::advertised_capabilities;

#[test]
fn the_mandatory_families_are_always_advertised() {
    let caps = advertised_capabilities();
    for mandatory in Capability::MANDATORY {
        assert!(
            caps.contains(mandatory),
            "`{}` must be advertised in every build",
            mandatory.as_str()
        );
    }
}

#[cfg(feature = "memory-git")]
#[test]
fn the_full_engine_advertises_every_family_with_memory_git() {
    // The lift's headline: this adapter used to advertise three families.
    assert_eq!(advertised_capabilities(), Capabilities::all());
    assert!(advertised_capabilities().contains(Capability::Diff));
}

#[cfg(not(feature = "memory-git"))]
#[test]
fn diff_is_withheld_when_the_snapshot_store_is_compiled_out() {
    // The gate has to reach the advertisement, not just the accessor. A build
    // that advertised `Diff` here would fail `audit_provider` — which is how
    // that audit earns its place.
    let caps = advertised_capabilities();
    assert!(!caps.contains(Capability::Diff));
    // Everything else the engine serves is still advertised: withholding one
    // family must not quietly withhold the rest.
    assert_eq!(caps, Capabilities::all().without(Capability::Diff));
    assert_eq!(caps.len(), Capabilities::all().len() - 1);
}
