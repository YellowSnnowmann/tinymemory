//! Test-only construction helpers for bounded ingestion queues.

use super::*;

impl IngestionQueue {
    pub(super) fn from_parts(
        tx: mpsc::Sender<IngestionJob>,
        state: IngestionState,
        capacity: usize,
    ) -> Self {
        Self {
            tx,
            state,
            capacity,
        }
    }
}
