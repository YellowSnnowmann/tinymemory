//! `Config` adapters for tinycortex's chunk connection and recovery manager.

use anyhow::Result;
use rusqlite::Connection;

use crate::engine::engine_config;
use crate::Config;

#[doc(hidden)]
pub fn with_connection<T>(config: &Config, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    crate::engine::backend::chunks::with_connection(&engine_config(config), f)
}

pub(crate) fn recover_corrupt_db(config: &Config) -> Result<bool> {
    log::warn!("[memory:chunks] checking corrupt database recovery");
    crate::engine::backend::chunks::recover_corrupt_db(&engine_config(config))
}
