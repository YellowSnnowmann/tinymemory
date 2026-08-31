# Granular ingestion and retrieval API

## Purpose

TinyMemory exposes six product-facing memory operations:

1. document ingestion;
2. conversation ingestion;
3. learning ingestion;
4. event ingestion;
5. recall; and
6. answer.

They are capability-negotiated independently. A connector must never advertise
an operation merely because it implements a neighbouring one.

## Contract

Document and conversation ingestion accept the existing `IngestItem` wire
shape. This preserves source identity, ownership, timestamps, provenance taint,
and citations without adding a parallel payload model. Conversations are
ordered batches and every item must share one `source_id`.

Learning ingestion accepts `LearningCandidate`, including its cue family,
confidence, and typed evidence pointer. Event ingestion accepts
`EpisodicEvent`, the durable event shape already used by the episodic store.

Recall remains the mandatory deterministic ranked-retrieval mechanism. Answer
is optional: it retrieves evidence, asks a configured inference route to
synthesise grounded prose, and returns the answer together with citations and a
content-free execution trace.

The five optional operation capabilities are appended to the capability bit
order as `document_ingest`, `conversation_ingest`, `learning_ingest`,
`event_ingest`, and `answer`. Existing capability indices do not move.

## Adapter matrix

| Adapter | Document | Conversation | Learning | Event | Recall | Answer |
| --- | --- | --- | --- | --- | --- | --- |
| TinyCortex lightweight provider | yes | no | no | no | yes | no |
| Full TinyCortex/Cortex provider | yes | yes | yes | yes | yes | yes |
| Mem0 | no | yes | no | no | yes | no |
| Supermemory, Cognee, AgentMemory | no | no | no | no | yes | no |
| Null | no | no | no | no | yes, empty | no |

The full embedded provider uses the native document and chat canonicalisation
pipelines. Learnings are stored durably under `learning:<facet-class>` and also
enter the candidate buffer. Events use the episodic event store. Answer uses
the host-provided chat route and never owns credentials.

Mem0 stores each ordered message through its native memory endpoint with a
deterministic key and conversation namespace. It deliberately advertises no
other ingestion capability.

## Failure and safety rules

- Empty identifiers, empty content, invalid confidence, and zero answer limits
  are `MemoryError::Invalid`.
- Provenance taint is passed through document and conversation routes.
- Source allowlists are applied inside recall before answer synthesis.
- An answer response exposes retrieved evidence but never exposes prompts,
  credentials, or hidden model reasoning.
- Unsupported operations are absent from capability negotiation and provider
  accessors; callers do not discover them by invoking a failing method.
