#![cfg(test)]
//! Test-only helpers for initializing the process-global memory client.

use super::*;

pub fn init_default() -> Result<MemoryClientRef, String> {
    let workspace_dir = dirs::home_dir()
        .ok_or_else(|| "Could not find home directory".to_string())?
        .join(".openhuman")
        .join("workspace");
    init(workspace_dir)
}
