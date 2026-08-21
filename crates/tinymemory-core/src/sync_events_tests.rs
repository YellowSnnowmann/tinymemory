//! Tests for sync lifecycle values and source-id decoding.

use super::*;
use crate::events::RecordingSink;

#[test]
fn source_id_decoder_preserves_colons_in_item_ids_and_rejects_malformed_values() {
    assert_eq!(
        extract_mem_src_id("mem_src:feed_7:https://example.com/posts/1"),
        Some("feed_7")
    );
    assert_eq!(
        extract_mem_src_id("mem_src:folder:notes/a.md"),
        Some("folder")
    );
    for malformed in [
        "slack:workspace-1",
        "mem_src:",
        "mem_src:source-only",
        "mem_src:source:",
    ] {
        assert_eq!(
            extract_mem_src_id(malformed),
            None,
            "accepted {malformed:?}"
        );
    }
}

#[test]
fn trigger_and_stage_strings_match_their_serde_wire_values() {
    for (trigger, expected) in [
        (MemorySyncTrigger::Manual, "manual"),
        (MemorySyncTrigger::Cron, "cron"),
    ] {
        assert_eq!(trigger.as_str(), expected);
        assert_eq!(serde_json::to_value(trigger).unwrap(), expected);
    }
    for (stage, expected) in [
        (MemorySyncStage::Requested, "requested"),
        (MemorySyncStage::Fetching, "fetching"),
        (MemorySyncStage::Stored, "stored"),
        (MemorySyncStage::Queued, "queued"),
        (MemorySyncStage::Ingesting, "ingesting"),
        (MemorySyncStage::Completed, "completed"),
        (MemorySyncStage::Failed, "failed"),
    ] {
        assert_eq!(stage.as_str(), expected);
        assert_eq!(serde_json::to_value(stage).unwrap(), expected);
    }
}

#[test]
fn emitting_a_sync_stage_preserves_all_optional_context() {
    let previous = crate::events::event_sink();
    let sink = RecordingSink::install();
    emit_sync_stage(
        MemorySyncTrigger::Manual,
        MemorySyncStage::Failed,
        Some("rss"),
        Some("connection-4"),
        Some("bad feed".to_string()),
        Some("source-9"),
    );

    let events = sink.drain();
    assert_eq!(events.len(), 1);
    match &events[0] {
        crate::events::MemoryEvent::SyncStageChanged {
            trigger,
            stage,
            provider,
            connection_id,
            detail,
            source_id,
        } => {
            assert_eq!(trigger, "manual");
            assert_eq!(stage, "failed");
            assert_eq!(provider.as_deref(), Some("rss"));
            assert_eq!(connection_id.as_deref(), Some("connection-4"));
            assert_eq!(detail.as_deref(), Some("bad feed"));
            assert_eq!(source_id.as_deref(), Some("source-9"));
        }
        event => panic!("unexpected event: {event:?}"),
    }

    match previous {
        Some(sink) => crate::events::set_event_sink(sink),
        None => crate::events::clear_event_sink(),
    }
}
