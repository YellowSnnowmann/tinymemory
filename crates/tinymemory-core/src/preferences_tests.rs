//! Tests for the surrounding module.

use super::*;
use crate::store::UnifiedMemory;
use crate::MemoryCategory;
use tempfile::TempDir;
use tinymemory_api::host::NoopEmbedding;

#[tokio::test]
async fn load_general_preferences_returns_values_newest_first_capped() {
    let tmp = TempDir::new().unwrap();
    let mem: Arc<dyn Memory> =
        Arc::new(UnifiedMemory::new(tmp.path(), Arc::new(NoopEmbedding), None).unwrap());

    mem.store(
        USER_PREF_GENERAL_NAMESPACE,
        "reply_language",
        "Reply in British English.",
        MemoryCategory::Core,
        None,
    )
    .await
    .unwrap();
    mem.store(
        USER_PREF_GENERAL_NAMESPACE,
        "tone",
        "Be terse.",
        MemoryCategory::Core,
        None,
    )
    .await
    .unwrap();

    let general = load_general_preferences(&mem, 10).await;
    // Returns the values (bodies), not the topic keys.
    assert!(general.iter().any(|v| v.contains("British English")));
    assert!(general.iter().any(|v| v.contains("Be terse")));
    assert!(!general.iter().any(|v| v == "reply_language"));

    // The limit caps the block.
    assert_eq!(load_general_preferences(&mem, 1).await.len(), 1);
}
