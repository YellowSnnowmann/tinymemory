//! Tests for the learning-candidate taxonomy — the pure-data half.
//!
//! The buffer tests (FIFO order, bounded eviction, the process-global
//! singleton) stay in the engine crate next to the buffer that owns them:
//! `tinymemory_core::learning_candidate`. Nothing here touches a queue.
//!
//! What is pinned below is the part a *second* process can observe: the serde
//! discriminants, which are persisted alongside profile facets and which a
//! producer in the module and a consumer in the host have to agree on.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use super::{CueFamily, FacetClass, LearningCandidate};
use crate::evidence::EvidenceRef;

fn candidate(class: FacetClass, cue_family: CueFamily) -> LearningCandidate {
    LearningCandidate {
        class,
        key: "verbosity".into(),
        value: "terse".into(),
        cue_family,
        evidence: EvidenceRef::Episodic { episodic_id: 1 },
        initial_confidence: 0.8,
        observed_at: 1_700_000_000.0,
    }
}

#[test]
fn every_facet_class_serialises_to_its_stable_snake_case_name() {
    let cases = [
        (FacetClass::Style, "\"style\""),
        (FacetClass::Identity, "\"identity\""),
        (FacetClass::Tooling, "\"tooling\""),
        (FacetClass::Veto, "\"veto\""),
        (FacetClass::Goal, "\"goal\""),
        (FacetClass::Channel, "\"channel\""),
    ];
    for (class, wire) in cases {
        let json = serde_json::to_string(&class).expect("serialize");
        assert_eq!(json, wire, "wire form changed for {class:?}");
        let back: FacetClass = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, class);
    }
}

#[test]
fn every_cue_family_serialises_to_its_stable_snake_case_name() {
    let cases = [
        (CueFamily::Explicit, "\"explicit\""),
        (CueFamily::Structural, "\"structural\""),
        (CueFamily::Behavioral, "\"behavioral\""),
        (CueFamily::Recurrence, "\"recurrence\""),
    ];
    for (family, wire) in cases {
        let json = serde_json::to_string(&family).expect("serialize");
        assert_eq!(json, wire, "wire form changed for {family:?}");
        let back: CueFamily = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, family);
    }
}

#[test]
fn cue_family_weights_are_the_canonical_values() {
    assert_eq!(CueFamily::Explicit.weight(), 1.0);
    assert_eq!(CueFamily::Structural.weight(), 0.9);
    assert_eq!(CueFamily::Behavioral.weight(), 0.7);
    assert_eq!(CueFamily::Recurrence.weight(), 0.6);
}

#[test]
fn weights_are_ordered_explicit_down_to_recurrence() {
    // The formula only makes sense if a stated preference outranks an inferred
    // one. Asserting the ordering catches a retune that inverts two families
    // without anyone noticing the ranking flipped.
    assert!(CueFamily::Explicit.weight() > CueFamily::Structural.weight());
    assert!(CueFamily::Structural.weight() > CueFamily::Behavioral.weight());
    assert!(CueFamily::Behavioral.weight() > CueFamily::Recurrence.weight());
}

#[test]
fn a_candidate_round_trips_with_its_evidence_pointer() {
    let original = candidate(FacetClass::Tooling, CueFamily::Structural);
    let json = serde_json::to_string(&original).expect("serialize");
    let back: LearningCandidate = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(back.class, original.class);
    assert_eq!(back.key, original.key);
    assert_eq!(back.value, original.value);
    assert_eq!(back.cue_family, original.cue_family);
    assert_eq!(back.evidence, original.evidence);
    assert_eq!(back.initial_confidence, original.initial_confidence);
    assert_eq!(back.observed_at, original.observed_at);
}

#[test]
fn candidate_field_names_are_the_persisted_ones() {
    let json = serde_json::to_value(candidate(FacetClass::Goal, CueFamily::Explicit))
        .expect("serialize to value");
    let object = json.as_object().expect("candidate serialises as an object");
    for field in [
        "class",
        "key",
        "value",
        "cue_family",
        "evidence",
        "initial_confidence",
        "observed_at",
    ] {
        assert!(object.contains_key(field), "missing field {field}");
    }
}
