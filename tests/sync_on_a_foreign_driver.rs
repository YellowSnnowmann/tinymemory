//! Composio payloads normalised and stored through a driver that is not TinyCortex.
//!
//! Issue #18 §B3's stated purpose — "so a non-TinyCortex engine gets Composio
//! sync for free" — and the first half of §B5's acceptance test, "Composio
//! Gmail sync completes end to end against a driver that is not TinyCortex".
//!
//! # What this proves, and what it does not
//!
//! It proves the *coupling* is gone. Before §B3 these normalisers lived inside
//! the TinyCortex engine, and `tinymemory-core` reached in through
//! `tinycortex::memory::sync::composio::providers::normalize::*` to use them —
//! so a host binding a different engine could not run them at all. This file
//! links `tinymemory-sync` and a provider, and never names an engine.
//!
//! It does **not** prove the full §B5 acceptance test. That drives a live
//! Composio API through the sync pipeline; the pipeline itself still lives
//! behind engine-owned state (§B1, §B2), which is why §B5 stays open. What is
//! testable today is that the transform and the storage tier have no engine
//! between them, which is the part §B3 was responsible for.

#![allow(clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use serde_json::json;
use tinymemory::api::null::NullMemoryProvider;
use tinymemory::api::provider::{MemoryCore, MemoryProvider};
use tinymemory::api::types::{MemoryCategory, MemoryTaint, GLOBAL_NAMESPACE};
use tinymemory_conformance::InMemoryProvider;

/// A raw Composio Gmail fetch response, in the shape the normaliser expects.
///
/// Field names are the upstream ones (`messageId`, `sender`, `messageTimestamp`)
/// rather than the reshaped ones; turning the first into the second is the
/// transform under test.
fn raw_gmail_response() -> serde_json::Value {
    json!({
        "messages": [
            {
                "messageId": "msg-1",
                "threadId": "t1",
                "subject": "Lunch?",
                "sender": "someone@example.com",
                "to": "me@example.com",
                "messageTimestamp": "2026-04-17T12:00:00Z",
                "labelIds": ["INBOX"],
                "messageText": "the cat sat on the mat",
                "payload": {}
            }
        ],
        "nextPageToken": "tok-1"
    })
}

/// Stores every normalised message into `provider`, returning the keys written.
///
/// The whole point of the exercise: this function is generic over the driver
/// and names no engine.
async fn ingest_into(provider: &dyn MemoryProvider, raw: serde_json::Value) -> Vec<String> {
    let mut data = raw;
    tinymemory_sync::gmail_post_process::post_process("GMAIL_FETCH_EMAILS", None, &mut data);

    let messages = data
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut written = Vec::new();
    for message in messages {
        let key = message
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown")
            .to_owned();
        let content = serde_json::to_string(&message).expect("a normalised message serialises");
        provider
            .store(
                GLOBAL_NAMESPACE,
                &key,
                &content,
                MemoryCategory::Core,
                None,
                // External by provenance: this came off somebody's inbox. A
                // driver that laundered it to `Internal` is the failure the
                // taint argument exists to prevent.
                MemoryTaint::ExternalSync,
            )
            .await
            .expect("store into the bound driver");
        written.push(key);
    }
    written
}

#[tokio::test]
async fn a_gmail_payload_normalises_and_stores_without_an_engine() {
    let provider = InMemoryProvider::new();
    let written = ingest_into(&provider, raw_gmail_response()).await;

    assert_eq!(
        written,
        vec!["msg-1".to_string()],
        "one message was written"
    );

    let stored = provider
        .get(GLOBAL_NAMESPACE, "msg-1")
        .await
        .expect("read back")
        .expect("the message was just stored");

    assert!(
        stored.content.contains("the cat sat on the mat"),
        "the normalised body did not survive the round trip: {}",
        stored.content
    );
    assert_eq!(
        stored.taint,
        MemoryTaint::ExternalSync,
        "provenance was laundered on the way in"
    );
}

#[tokio::test]
async fn the_same_payload_runs_against_a_second_unrelated_driver() {
    // The claim is "a non-TinyCortex engine", not "this one particular
    // non-TinyCortex engine". Running the identical path against a driver with
    // completely different retention semantics is what makes that general.
    let provider = NullMemoryProvider::new();
    let written = ingest_into(&provider, raw_gmail_response()).await;

    assert_eq!(written, vec!["msg-1".to_string()]);
    assert!(
        provider
            .get(GLOBAL_NAMESPACE, "msg-1")
            .await
            .expect("read back")
            .is_none(),
        "the null driver retains nothing, so the read must be empty — if this \
         returned a record the driver is not the one we think it is"
    );
}

#[tokio::test]
async fn the_normaliser_is_reachable_without_naming_an_engine() {
    // The structural assertion behind §B3. This file's dependencies are the
    // facade, the conformance reference driver, and `tinymemory-sync`. If the
    // normalisers still lived in the engine this would not compile, which is
    // the whole test — the body below just keeps it from being vacuous.
    let mut data = json!({ "messages": [] });
    tinymemory_sync::slack_post_process::post_process("SLACK_LIST_CONVERSATIONS", None, &mut data);
    assert!(
        data.is_object(),
        "the slack normaliser should leave an object in place"
    );

    let arc: Arc<dyn MemoryProvider> = Arc::new(InMemoryProvider::new());
    assert_eq!(arc.driver_id(), "reference");
}
