#![cfg(test)]
//! Test-only reset helper for the process-global health-report latch.

use super::*;

pub(super) fn reset_health_gate_for_test() {
    OLLAMA_HEALTH_REPORTED.store(false, Ordering::Release);
}
