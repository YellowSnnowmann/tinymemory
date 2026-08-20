//! The re-exported error table still round-trips from this crate's paths.
//!
//! `tinymemory_api::wire_tests` pins the table itself. What is checked here is
//! that the re-export surfaces the whole of it — a name constant that failed to
//! come across would leave a host unable to recognise that error class, and a
//! missing `pub use` is invisible until someone reaches for it.

use super::{from_wire, wire_name};
use crate::types::MemoryError;

#[test]
fn every_name_constant_is_reachable_from_this_crate() {
    let names = [
        super::NOT_FOUND,
        super::INVALID,
        super::BUDGET_EXCEEDED,
        super::PATH_ESCAPE,
        super::IO,
        super::SERDE,
        super::UNSUPPORTED,
        super::OTHER,
        super::UNAUTHORIZED,
        super::UNREACHABLE,
        super::TIMEOUT,
        super::UNAVAILABLE,
        super::BACKEND,
    ];
    for name in names {
        assert!(
            name.starts_with("ai.tinyhumans.tinymemory.Error."),
            "{name} is not under the contract's error namespace"
        );
    }
}

#[test]
fn a_named_error_round_trips_through_the_re_exports() {
    let recovered = from_wire(super::PATH_ESCAPE, "symlink leaves workspace");
    assert!(matches!(recovered, MemoryError::PathEscape(_)));
    // Back out again under the same name: the two directions are the same
    // table, which is the property that keeps the ends from drifting.
    assert_eq!(wire_name(&recovered), super::PATH_ESCAPE);
}

#[test]
fn an_unknown_name_is_a_backend_failure_not_a_caller_mistake() {
    // A module newer than this build may name an error this table has no
    // variant for. Reporting that as `Invalid` would tell a caller its input
    // was wrong when it was not.
    let recovered = from_wire("ai.tinyhumans.tinymemory.Error.FromTheFuture", "…");
    assert!(matches!(recovered, MemoryError::Other(_)));
}
