//! Product Config adapters over tinycortex's SQLite queue store.

use anyhow::Result;
use rusqlite::Transaction;

use crate::tree::health::PipelineFailure;
use crate::Config;

use super::types::{Job, JobFailure, JobStatus, NewJob};
use crate::engine::engine_config;

pub use crate::engine::backend::queue::DEFAULT_LOCK_DURATION_MS;

pub fn enqueue(config: &Config, job: &NewJob) -> Result<Option<String>> {
    crate::engine::backend::queue::enqueue(&engine_config(config), job)
}

pub fn enqueue_tx(tx: &Transaction<'_>, job: &NewJob) -> Result<Option<String>> {
    crate::engine::backend::queue::enqueue_tx(tx, job)
}

pub fn claim_next(config: &Config, lock_duration_ms: i64) -> Result<Option<Job>> {
    crate::engine::backend::queue::claim_next(&engine_config(config), lock_duration_ms)
}

pub fn mark_done(config: &Config, job: &Job) -> Result<()> {
    crate::engine::backend::queue::mark_done(&engine_config(config), job)
}

pub fn mark_failed(config: &Config, job: &Job, error: &str) -> Result<()> {
    crate::engine::backend::queue::mark_failed(&engine_config(config), job, error)
}

pub fn mark_failed_typed(
    config: &Config,
    job: &Job,
    error: &str,
    failure: Option<&PipelineFailure>,
) -> Result<()> {
    let failure = failure.map(|failure| JobFailure {
        code: failure.code.as_str(),
        class: failure.class.as_str(),
    });
    crate::engine::backend::queue::mark_failed_typed(
        &engine_config(config),
        job,
        error,
        failure.as_ref(),
    )
}

pub fn mark_deferred(config: &Config, job: &Job, until_ms: i64, reason: &str) -> Result<()> {
    crate::engine::backend::queue::mark_deferred(&engine_config(config), job, until_ms, reason)
}

pub fn recover_stale_locks(config: &Config) -> Result<usize> {
    crate::engine::backend::queue::recover_stale_locks(&engine_config(config))
}

pub fn requeue_failed(config: &Config) -> Result<u64> {
    crate::engine::backend::queue::requeue_failed(&engine_config(config))
}

pub fn requeue_transient_failed(config: &Config) -> Result<u64> {
    crate::engine::backend::queue::requeue_transient_failed(&engine_config(config))
}

pub fn release_running_locks(config: &Config) -> Result<usize> {
    crate::engine::backend::queue::release_running_locks(&engine_config(config))
}

pub fn count_by_status(config: &Config, status: JobStatus) -> Result<u64> {
    crate::engine::backend::queue::count_by_status(&engine_config(config), status)
}

pub fn count_failed_unrecoverable(config: &Config) -> Result<u64> {
    crate::engine::backend::queue::count_failed_unrecoverable(&engine_config(config))
}

pub fn count_total(config: &Config) -> Result<u64> {
    crate::engine::backend::queue::count_total(&engine_config(config))
}

pub fn retry_all_failed(config: &Config) -> Result<u64> {
    crate::engine::backend::queue::retry_all_failed(&engine_config(config))
}

pub fn get_job(config: &Config, id: &str) -> Result<Option<Job>> {
    crate::engine::backend::queue::get_job(&engine_config(config), id)
}
