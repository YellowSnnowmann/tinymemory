//! Client-facing `user_error` surfacing for memory-pipeline health causes.
//!
//! The memory pipeline already records typed causes for the status panel, but
//! the panel only exists while the user is looking at it. A cause the user must
//! act on outside the app — the local Ollama runtime being unusable — also
//! belongs in the durable UserErrorCenter.
//!
//! # What is here, and what is in the host
//!
//! Getting that into the UserErrorCenter means a `user_error` web-channel
//! event, and web channels are host surface. So this module owns only the
//! *decision to report*, published as
//! [`MemoryEvent::LocalModelUnavailable`]; the host's sink builds the wire
//! payload and broadcasts it. The `error_type` token both sides key on lives in
//! the contract crate ([`LOCAL_MODEL_UNAVAILABLE_KIND`]) so it cannot drift.
//!
//! The two producers — the embedder health gate in
//! [`crate::store::factories`] and the failure classifier in the parent module
//! — both go through [`publish_local_model_unavailable_user_error`], so they
//! emit one identical shape.

pub(crate) use tinymemory_api::host::LOCAL_MODEL_UNAVAILABLE_KIND;

/// Report that the local embedding runtime is unusable.
///
/// `origin` is a short, non-sensitive tag naming which producer fired
/// (`health_gate` / `embed_classify`) so the two paths stay distinguishable in
/// the log without threading a correlation id through the health API.
pub(crate) fn publish_local_model_unavailable_user_error(origin: &str) {
    log::debug!(
        "[memory_tree::health] action=surface_user_error kind={LOCAL_MODEL_UNAVAILABLE_KIND} \
         origin={origin}"
    );
    crate::events::publish(crate::events::MemoryEvent::LocalModelUnavailable {
        origin: origin.to_string(),
    });
}
