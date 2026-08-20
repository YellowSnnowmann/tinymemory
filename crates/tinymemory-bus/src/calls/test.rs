//! Completeness and encoding tests for the generated call structs.
//!
//! The interesting property is coverage. A member the module serves but this
//! crate has no struct for is not a compile error anywhere — it is a host
//! discovering at runtime that the only way to make the call is to hand-build
//! the argument array, which is exactly what this crate exists to prevent. So
//! the table below is checked against `crate::names::METHODS` in both
//! directions.

// A failed assertion in a test is a panic either way; `expect` here says what
// the invariant was. Same allowance the crate's other test modules take.
#![allow(clippy::expect_used, clippy::panic)]

use serde_json::json;

use crate::calls::BusCall;
use crate::names::METHODS;

/// The member every call struct in this crate names, one entry per struct.
///
/// Written out rather than derived, because deriving it from the same source
/// the structs come from would make the test agree with itself by
/// construction.
const COVERED: [&str; 89] = [
    <crate::calls::driver::DriverId as BusCall>::METHOD,
    <crate::calls::driver::Capabilities as BusCall>::METHOD,
    <crate::calls::driver::Health as BusCall>::METHOD,
    <crate::calls::driver::Shutdown as BusCall>::METHOD,
    <crate::calls::driver::OpenStore as BusCall>::METHOD,
    <crate::calls::core::Store as BusCall>::METHOD,
    <crate::calls::core::Get as BusCall>::METHOD,
    <crate::calls::core::Forget as BusCall>::METHOD,
    <crate::calls::core::List as BusCall>::METHOD,
    <crate::calls::core::Namespaces as BusCall>::METHOD,
    <crate::calls::recall::Recall as BusCall>::METHOD,
    <crate::calls::recall::RecallNamespaceScored as BusCall>::METHOD,
    <crate::calls::portability::ExportPage as BusCall>::METHOD,
    <crate::calls::portability::ImportRecords as BusCall>::METHOD,
    <crate::calls::ingest::IngestDocument as BusCall>::METHOD,
    <crate::calls::ingest::IngestChat as BusCall>::METHOD,
    <crate::calls::documents::PutDocument as BusCall>::METHOD,
    <crate::calls::documents::GetDocument as BusCall>::METHOD,
    <crate::calls::documents::ListDocuments as BusCall>::METHOD,
    <crate::calls::documents::ListNamespaces as BusCall>::METHOD,
    <crate::calls::documents::DeleteDocument as BusCall>::METHOD,
    <crate::calls::documents::ClearNamespace as BusCall>::METHOD,
    <crate::calls::documents::QueryDocuments as BusCall>::METHOD,
    <crate::calls::documents::RecallDocuments as BusCall>::METHOD,
    <crate::calls::tree::Append as BusCall>::METHOD,
    <crate::calls::tree::QuerySource as BusCall>::METHOD,
    <crate::calls::tree::DrillDown as BusCall>::METHOD,
    <crate::calls::tree::Seal as BusCall>::METHOD,
    <crate::calls::tree::Cascade as BusCall>::METHOD,
    <crate::calls::graph::Entities as BusCall>::METHOD,
    <crate::calls::graph::EntityEdges as BusCall>::METHOD,
    <crate::calls::graph::TouchEntities as BusCall>::METHOD,
    <crate::calls::graph::SearchEntities as BusCall>::METHOD,
    <crate::calls::graph::Relations as BusCall>::METHOD,
    <crate::calls::graph::PutRelation as BusCall>::METHOD,
    <crate::calls::graph::KvGet as BusCall>::METHOD,
    <crate::calls::graph::KvPut as BusCall>::METHOD,
    <crate::calls::graph::KvDelete as BusCall>::METHOD,
    <crate::calls::graph::KvList as BusCall>::METHOD,
    <crate::calls::sources::CaptureSnapshot as BusCall>::METHOD,
    <crate::calls::sources::Snapshots as BusCall>::METHOD,
    <crate::calls::sources::Diff as BusCall>::METHOD,
    <crate::calls::sources::AcceptSourceItems as BusCall>::METHOD,
    <crate::calls::sources::ForgetSource as BusCall>::METHOD,
    <crate::calls::goals::Goals as BusCall>::METHOD,
    <crate::calls::goals::SetGoals as BusCall>::METHOD,
    <crate::calls::tool_memory::ToolRules as BusCall>::METHOD,
    <crate::calls::tool_memory::PutToolRule as BusCall>::METHOD,
    <crate::calls::tool_memory::DeleteToolRule as BusCall>::METHOD,
    <crate::calls::maintenance::Reembed as BusCall>::METHOD,
    <crate::calls::maintenance::Compact as BusCall>::METHOD,
    <crate::calls::maintenance::Consolidate as BusCall>::METHOD,
    <crate::calls::maintenance::Doctor as BusCall>::METHOD,
    <crate::calls::people::ListPeople as BusCall>::METHOD,
    <crate::calls::people::GetPerson as BusCall>::METHOD,
    <crate::calls::people::ResolveHandle as BusCall>::METHOD,
    <crate::calls::people::AddHandleAlias as BusCall>::METHOD,
    <crate::calls::people::ScorePerson as BusCall>::METHOD,
    <crate::calls::people::RecordInteraction as BusCall>::METHOD,
    <crate::calls::people::SeedFromAddressBook as BusCall>::METHOD,
    <crate::calls::chunks::ListChunks as BusCall>::METHOD,
    <crate::calls::chunks::GetChunk as BusCall>::METHOD,
    <crate::calls::chunks::ChunkDetail as BusCall>::METHOD,
    <crate::calls::chunks::StorageKinds as BusCall>::METHOD,
    <crate::calls::chunks::ChunkEmbeddings as BusCall>::METHOD,
    <crate::calls::retrieval::FastRetrieve as BusCall>::METHOD,
    <crate::calls::retrieval::CoverWindow as BusCall>::METHOD,
    <crate::calls::retrieval::RetrieveSource as BusCall>::METHOD,
    <crate::calls::retrieval::RetrieveChildren as BusCall>::METHOD,
    <crate::calls::retrieval::RetrieveLeaves as BusCall>::METHOD,
    <crate::calls::profile::ListActiveFacets as BusCall>::METHOD,
    <crate::calls::profile::ListAllFacets as BusCall>::METHOD,
    <crate::calls::profile::GetFacet as BusCall>::METHOD,
    <crate::calls::profile::FacetsByType as BusCall>::METHOD,
    <crate::calls::profile::UpsertFacet as BusCall>::METHOD,
    <crate::calls::profile::UpsertProviderFacet as BusCall>::METHOD,
    <crate::calls::profile::SetFacetUserState as BusCall>::METHOD,
    <crate::calls::profile::DeleteFacet as BusCall>::METHOD,
    <crate::calls::profile::DeleteFacetById as BusCall>::METHOD,
    <crate::calls::profile::DropFacetsBelow as BusCall>::METHOD,
    <crate::calls::profile::WorkflowIdentityMatches as BusCall>::METHOD,
    <crate::calls::episodic::InsertTurn as BusCall>::METHOD,
    <crate::calls::episodic::SessionTurns as BusCall>::METHOD,
    <crate::calls::episodic::OpenSegment as BusCall>::METHOD,
    <crate::calls::episodic::CreateSegment as BusCall>::METHOD,
    <crate::calls::episodic::AppendTurn as BusCall>::METHOD,
    <crate::calls::episodic::CloseSegment as BusCall>::METHOD,
    <crate::calls::episodic::SetSegmentSummary as BusCall>::METHOD,
    <crate::calls::episodic::UpsertSegmentEmbedding as BusCall>::METHOD,
];

#[test]
fn every_member_has_a_call_struct() {
    let mut missing: Vec<&str> = METHODS
        .into_iter()
        .filter(|member| !COVERED.contains(member))
        .collect();
    missing.sort_unstable();
    assert!(
        missing.is_empty(),
        "members with no call struct: {missing:?}"
    );
}

#[test]
fn every_call_struct_names_a_known_member() {
    let mut unknown: Vec<&str> = COVERED
        .into_iter()
        .filter(|member| !METHODS.contains(member))
        .collect();
    unknown.sort_unstable();
    assert!(
        unknown.is_empty(),
        "call structs naming no member: {unknown:?}"
    );
}

#[test]
fn no_member_is_covered_twice() {
    let mut seen = COVERED;
    seen.sort_unstable();
    let mut unique = seen.to_vec();
    unique.dedup();
    assert_eq!(
        unique.len(),
        seen.len(),
        "two call structs name the same member"
    );
}

#[test]
fn arguments_encode_as_a_positional_array_in_declaration_order() {
    // `Diff` is the useful shape to pin: three arguments, the middle one
    // optional. A struct field reordering that a reader would not notice
    // shows up here as a moved `null`.
    let args = crate::calls::sources::Diff {
        source_id: "src-1".to_string(),
        from: None,
        to: "snap-2".to_string(),
    }
    .into_args()
    .expect("plain data serializes");
    assert_eq!(args, json!(["src-1", null, "snap-2"]));
}

#[test]
fn a_member_with_no_arguments_encodes_as_an_empty_array() {
    // Not `null`: `#[tinybus::interface]` skips argument decoding entirely
    // for a zero-argument member, and every caller sends `[]`.
    let args = crate::calls::maintenance::Doctor
        .into_args()
        .expect("no fields to serialize");
    assert_eq!(args, json!([]));
}

#[test]
fn a_reply_decodes_into_the_calls_response_type() {
    use crate::calls::core::Forget;

    let decoded = Forget::decode_response(json!(true)).expect("a bool reply");
    assert!(decoded);
}

#[test]
fn a_reply_of_the_wrong_shape_is_a_decode_error() {
    use crate::calls::core::Forget;
    use crate::error::Error;

    // The version-skew case: a module built from a different contract
    // answering something this build cannot read.
    let failure = Forget::decode_response(json!("yes")).expect_err("a string is not a bool");
    assert!(matches!(failure, Error::Decode(_)));
}
