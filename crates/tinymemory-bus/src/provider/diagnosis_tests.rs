//! Tests for the pipeline diagnosis.
//!
//! The invariant worth pinning is that an older module's report still decodes:
//! every compound field defaults, so a diagnosis missing `degraded` or
//! `counters` reads as "nothing reported" rather than failing the call — which
//! is the difference between a status panel that degrades and one that goes
//! blank.

// A failed assertion in a test is a panic either way; `unwrap`/`expect` here say
// what the invariant was. Same allowance the crate's other test modules take.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use super::*;

#[test]
fn a_minimal_diagnosis_decodes_to_nothing_reported() {
    let diagnosis: Diagnosis =
        serde_json::from_value(serde_json::json!({ "healthy": true })).expect("decode a minimal");
    assert!(diagnosis.healthy);
    assert!(diagnosis.stages.is_empty());
    assert_eq!(diagnosis.first_blocking_cause, None);
    assert_eq!(diagnosis.degraded, DegradedCapabilities::default());
    assert_eq!(diagnosis.counters.total_chunks, 0);
    assert_eq!(diagnosis.counters.extraction_coverage, None);
}

#[test]
fn an_unmeasured_coverage_is_not_a_measured_zero() {
    // `None` is "the read failed"; `Some(0.0)` is "nothing has structure". A
    // caller escalates on the second and retries the first.
    let unmeasured = DiagnosisCounters::default();
    let measured = DiagnosisCounters {
        extraction_coverage: Some(0.0),
        ..DiagnosisCounters::default()
    };
    assert_ne!(unmeasured, measured);
    assert!(serde_json::to_value(&unmeasured)
        .expect("serialize counters")
        .get("extraction_coverage")
        .is_none());
}

#[test]
fn a_failure_carries_the_drivers_own_code_unparsed() {
    // A code this build has never heard of must survive the round trip: the
    // whole reason these are strings is that a newer driver classifies causes
    // this one cannot name.
    let failure = DiagnosisFailure {
        code: "a_cause_from_a_newer_driver".to_string(),
        class: None,
        remediation_key: "memory.doctor.unknown".to_string(),
        detail: Some("the driver's own words".to_string()),
    };
    let round_tripped: DiagnosisFailure =
        serde_json::from_value(serde_json::to_value(&failure).expect("serialize failure"))
            .expect("decode failure");
    assert_eq!(round_tripped, failure);
    assert_eq!(round_tripped.class, None);
}

#[test]
fn healthy_and_a_blocking_cause_are_kept_consistent_by_the_producer() {
    // The contract's rule, asserted on the shape a driver is expected to build:
    // `healthy` is `first_blocking_cause.is_none()`. The type cannot enforce
    // it, so the test states it where a reader will find it.
    let stages = vec![
        DiagnosisStage {
            stage: "routing".to_string(),
            ok: true,
            failure: None,
            note: "routed".to_string(),
        },
        DiagnosisStage {
            stage: "embeddings".to_string(),
            ok: false,
            failure: Some(DiagnosisFailure {
                code: "embeddings_unconfigured".to_string(),
                class: Some("unrecoverable".to_string()),
                remediation_key: "memory.embeddings.unconfigured".to_string(),
                detail: None,
            }),
            note: "no embedder resolved".to_string(),
        },
    ];
    let first = stages
        .iter()
        .find(|stage| !stage.ok)
        .and_then(|stage| stage.failure.clone());
    let diagnosis = Diagnosis {
        healthy: first.is_none(),
        stages,
        first_blocking_cause: first,
        degraded: DegradedCapabilities {
            semantic_recall: true,
            ..DegradedCapabilities::default()
        },
        counters: DiagnosisCounters::default(),
    };
    assert!(!diagnosis.healthy);
    assert_eq!(
        diagnosis
            .first_blocking_cause
            .as_ref()
            .map(|failure| failure.code.as_str()),
        Some("embeddings_unconfigured")
    );
}
