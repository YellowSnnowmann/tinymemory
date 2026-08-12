//! Tests for the service's error mapping.
//!
//! This is the module's half of the wire contract: every `MemoryError` the
//! engine can raise has to leave as a named bus error the host's client can map
//! back. `tinymemory_api::wire_tests` pins the table itself; what is tested here
//! is that the service actually goes through it.
//!
//! Covered here rather than in the loader E2E deliberately. An E2E can only
//! provoke the errors the engine happens to raise for a given input, which makes
//! it a test of engine internals this port does not own — an earlier revision
//! tried `ExportPage` with a zero limit, and since a driver accepting a zero
//! limit is equally legitimate, the test asserted nothing when it passed. Here
//! every variant is reachable by construction.

use tinybus::Error as BusError;
use tinymemory_api::error::MemoryError;
use tinymemory_api::wire;

use super::into_bus_error;

/// The name and message a mapped error carries on the wire.
fn mapped(error: &MemoryError) -> (String, String) {
    match into_bus_error(error) {
        BusError::MethodFailed { name, message } => (name, message),
        other => panic!("expected MethodFailed, got {other:?}"),
    }
}

#[test]
fn every_variant_leaves_under_its_contract_name() {
    // Exhaustive by construction: `wire::wire_name` is a total match over
    // `MemoryError`, so a new variant fails to compile there before it can
    // silently leave this list.
    let cases = [
        (MemoryError::NotFound("k".into()), wire::NOT_FOUND),
        (MemoryError::Invalid("bad".into()), wire::INVALID),
        (
            MemoryError::BudgetExceeded("too big".into()),
            wire::BUDGET_EXCEEDED,
        ),
        (
            MemoryError::PathEscape("../outside".into()),
            wire::PATH_ESCAPE,
        ),
        (MemoryError::unsupported_raw("tree"), wire::UNSUPPORTED),
        (
            MemoryError::Other(anyhow::anyhow!("engine fell over")),
            wire::OTHER,
        ),
    ];

    for (error, expected) in &cases {
        let (name, _) = mapped(error);
        assert_eq!(&name, expected, "{error:?} left under the wrong name");
    }
}

#[test]
fn a_path_escape_never_leaves_as_an_invalid() {
    // The security-relevant collapse. `Invalid` tells a caller its input was
    // malformed and invites a retry; a sandbox escape is not that, and the host
    // re-raises whatever it receives to its own callers.
    let (name, _) = mapped(&MemoryError::PathEscape("../../etc".into()));
    assert_eq!(name, wire::PATH_ESCAPE);
    assert_ne!(name, wire::INVALID);
}

#[test]
fn a_miss_never_leaves_as_an_invalid() {
    // `get`'s contract makes a miss `Ok(None)`, so a `NotFound` that arrived as
    // `Invalid` would turn an ordinary absence into a caller-visible failure.
    let (name, _) = mapped(&MemoryError::NotFound("absent".into()));
    assert_eq!(name, wire::NOT_FOUND);
    assert_ne!(name, wire::INVALID);
}

#[test]
fn the_names_the_service_emits_are_the_ones_the_host_decodes() {
    // The drift that matters is silent, so this closes the loop rather than
    // trusting the two tables to agree: map out through the service, back
    // through the client's decoder, and require the variant to survive.
    let originals = [
        MemoryError::NotFound("k".into()),
        MemoryError::Invalid("bad".into()),
        MemoryError::BudgetExceeded("too big".into()),
        MemoryError::PathEscape("../outside".into()),
        MemoryError::unsupported_raw("tree"),
    ];

    for original in &originals {
        let (name, message) = mapped(original);
        let decoded = wire::from_wire(&name, &message);
        assert_eq!(
            std::mem::discriminant(&decoded),
            std::mem::discriminant(original),
            "{original:?} did not survive the round trip, arrived as {decoded:?}"
        );
    }
}

#[test]
fn a_message_carries_no_user_content_beyond_what_the_engine_put_there() {
    // Not a redaction test — the engine owns its message. This pins that the
    // service adds nothing of its own, so the only thing that can leak is what
    // the engine already chose to say.
    let error = MemoryError::NotFound("some-key".into());
    let (_, message) = mapped(&error);
    assert_eq!(message, wire::wire_message(&error));
}
