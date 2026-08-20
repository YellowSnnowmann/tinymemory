//! Episodic turns and conversation segments.
//!
//! One [`BusCall`] per member; see [`crate::calls`] for how they are used.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use tinymemory_api::provider::episodic::{ConversationSegment, EpisodicTurn};

use crate::calls::BusCall;
use crate::error::Error;
use crate::names::methods;

/// Arguments for `InsertTurn`.
///
/// Record one turn, answering with the row id the engine assigned it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsertTurn {
    /// The `turn` argument — wire position 0.
    pub turn: EpisodicTurn,
}

impl BusCall for InsertTurn {
    const METHOD: &'static str = methods::INSERT_TURN;

    type Response = i64;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.turn,)).map_err(Error::Encode)
    }
}

/// Arguments for `SessionTurns`.
///
/// Every recorded turn for one session, oldest first.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTurns {
    /// The `session_id` argument — wire position 0.
    pub session_id: String,
}

impl BusCall for SessionTurns {
    const METHOD: &'static str = methods::SESSION_TURNS;

    type Response = Vec<EpisodicTurn>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.session_id,)).map_err(Error::Encode)
    }
}

/// Arguments for `OpenSegment`.
///
/// The open segment for a session, if there is one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenSegment {
    /// The `session_id` argument — wire position 0.
    pub session_id: String,
}

impl BusCall for OpenSegment {
    const METHOD: &'static str = methods::OPEN_SEGMENT;

    type Response = Option<ConversationSegment>;

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.session_id,)).map_err(Error::Encode)
    }
}

/// Arguments for `CreateSegment`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSegment {
    /// The `segment_id` argument — wire position 0.
    pub segment_id: String,
    /// The `session_id` argument — wire position 1.
    pub session_id: String,
    /// The `namespace` argument — wire position 2.
    pub namespace: String,
    /// The `start_episodic_id` argument — wire position 3.
    pub start_episodic_id: i64,
    /// The `start_timestamp` argument — wire position 4.
    pub start_timestamp: f64,
    /// The `now` argument — wire position 5.
    pub now: f64,
}

impl BusCall for CreateSegment {
    const METHOD: &'static str = methods::CREATE_SEGMENT;

    type Response = ();

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.segment_id, self.session_id, self.namespace, self.start_episodic_id, self.start_timestamp, self.now)).map_err(Error::Encode)
    }
}

/// Arguments for `AppendTurn`.
///
/// Extend a segment to include one more turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendTurn {
    /// The `segment_id` argument — wire position 0.
    pub segment_id: String,
    /// The `episodic_id` argument — wire position 1.
    pub episodic_id: i64,
    /// The `timestamp` argument — wire position 2.
    pub timestamp: f64,
    /// The `now` argument — wire position 3.
    pub now: f64,
}

impl BusCall for AppendTurn {
    const METHOD: &'static str = methods::APPEND_TURN;

    type Response = ();

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.segment_id, self.episodic_id, self.timestamp, self.now)).map_err(Error::Encode)
    }
}

/// Arguments for `CloseSegment`.
///
/// Mark a segment closed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloseSegment {
    /// The `segment_id` argument — wire position 0.
    pub segment_id: String,
    /// The `now` argument — wire position 1.
    pub now: f64,
}

impl BusCall for CloseSegment {
    const METHOD: &'static str = methods::CLOSE_SEGMENT;

    type Response = ();

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.segment_id, self.now)).map_err(Error::Encode)
    }
}

/// Arguments for `SetSegmentSummary`.
///
/// Attach a summary to a closed segment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetSegmentSummary {
    /// The `segment_id` argument — wire position 0.
    pub segment_id: String,
    /// The `summary` argument — wire position 1.
    pub summary: String,
    /// The `now` argument — wire position 2.
    pub now: f64,
}

impl BusCall for SetSegmentSummary {
    const METHOD: &'static str = methods::SET_SEGMENT_SUMMARY;

    type Response = ();

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.segment_id, self.summary, self.now)).map_err(Error::Encode)
    }
}

/// Arguments for `UpsertSegmentEmbedding`.
///
/// Store a segment's embedding under `model_signature`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertSegmentEmbedding {
    /// The `segment_id` argument — wire position 0.
    pub segment_id: String,
    /// The `model_signature` argument — wire position 1.
    pub model_signature: String,
    /// The `embedding` argument — wire position 2.
    pub embedding: Vec<f32>,
    /// The `created_at` argument — wire position 3.
    pub created_at: f64,
}

impl BusCall for UpsertSegmentEmbedding {
    const METHOD: &'static str = methods::UPSERT_SEGMENT_EMBEDDING;

    type Response = ();

    fn into_args(self) -> crate::Result<Value> {
        serde_json::to_value((self.segment_id, self.model_signature, self.embedding, self.created_at)).map_err(Error::Encode)
    }
}
