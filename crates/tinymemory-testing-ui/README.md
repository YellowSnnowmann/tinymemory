# TinyMemory testing UI

A throwaway harness for exercising TinyMemory engines by hand — not a host,
not shipped, not covered by the crate's default build/release surface. It
skips every policy layer a real host owns (tier enforcement, taint stamping,
redaction, egress checks); it exists so a person can point a browser at a
running server, pick an engine, connect it, and call `store` / `get` /
`recall` / `list` / `namespaces` / `forget` / `export` against it directly.

## Layout

```text
crates/tinymemory-testing-ui/
├── src/    tinymemory-testing-ui — an axum HTTP server wrapping the
│           MemoryProvider contract; a workspace member but deliberately left
│           out of default-members (see the root Cargo.toml)
└── web/    a static, dependency-free HTML/JS page served by the server
```

## Run it

```sh
git submodule update --init --recursive   # if not already done
cargo run -p tinymemory-testing-ui
```

Then open <http://127.0.0.1:4180>. The listen address can be overridden with
`TINYMEMORY_TESTING_UI_ADDR=host:port`.

## Selecting and connecting an engine

The page's left panel picks which engine `POST /api/connect` binds:

- **Local** — an in-process TinyCortex `InMemoryMemoryStore`, wrapped through
  `tinymemory-tinycortex::provider`. No endpoint, no API key, nothing
  persists past a server restart. This is the default and the fastest way to
  poke at the contract.
- **Supermemory / Mem0 / Cognee** — the `tinymemory-remote` native HTTP
  adapters. Mem0 and Cognee offer an explicit Cloud/self-hosted choice so the
  correct authentication scheme is used. These are real network calls to
  whatever endpoint you provide; nothing is mocked.

Only one engine is connected at a time — connecting again swaps the active
provider; disconnecting clears it. The server keeps credentials only in memory
and never logs them. The browser saves entered API keys in plain text in its
`localStorage`, where they can remain after the server exits; clear this site's
browser data to remove them.

## API surface

Every route lives under `/api` and maps directly onto
`tinymemory_api::provider::{MemoryCore, MemoryRecall, MemoryPortability, MemoryGraph}`:

| Route | Method | Contract call |
| --- | --- | --- |
| `/api/connect` | POST | bind a fresh provider |
| `/api/disconnect` | POST | clear the active provider |
| `/api/status` | GET | current connection state |
| `/api/store` | POST | `MemoryCore::store` |
| `/api/get` | GET | `MemoryCore::get` |
| `/api/forget` | POST | `MemoryCore::forget` |
| `/api/list` | GET | `MemoryCore::list` |
| `/api/namespaces` | GET | `MemoryCore::namespaces` |
| `/api/recall` | POST | `MemoryRecall::recall` |
| `/api/export` | GET | `MemoryPortability::export_page` |
| `/api/graph/relations` | GET | `MemoryGraph::relations` — 501 if the connected engine doesn't advertise Graph |

`MemoryCategory` is passed as its display string (`core`, `daily`,
`conversation`, or `custom:<name>`); `MemoryTaint` as `internal` or
`external_sync`.

The web UI's Graph tab only appears once `/api/connect` reports
`has_graph: true` for the bound engine.

## Testing against real local engines

`integration/remote-engines/` boots each self-hosted engine in Docker so this
harness can be driven end to end against the real thing, not a mock:

```sh
docker compose -f integration/remote-engines/docker-compose.yml --profile supermemory up -d --build
docker compose -f integration/remote-engines/docker-compose.yml logs supermemory   # copy the sm_... key

docker compose -f integration/remote-engines/docker-compose.yml --profile mem0 up -d --build
docker compose -f integration/remote-engines/docker-compose.yml --profile cognee up -d --build
```

Then connect the UI to `http://localhost:6767` (Supermemory, with its key),
`http://localhost:8888` (Mem0), or `http://localhost:8001` (Cognee), selecting
the self-hosted deployment for Mem0 and Cognee.

### Graph support per engine

- **Cognee** — real. `cognee_graph_provider` (`crates/tinymemory-remote/src/graph_provider.rs`,
  `cognee_graph.rs`) wraps Cognee's `GET /api/v1/datasets/{id}/graph` and
  reshapes its nodes/edges into `(subject, predicate, object)` triples. Only
  `relations` has a Cognee counterpart — `kv_get`/`kv_put`/`kv_delete`/`kv_list`
  and `put_relation` return `MemoryError::Other` because Cognee has no
  writable key/value store and its graph is derived by the `cognify` pipeline,
  not directly editable. **Cognee's graph only contains real entities/relations
  once `cognify` has run against a real LLM** — the harness's default
  `mock-inference` service is a deterministic HTTP-wiring stub (per
  `integration/remote-engines/README.md`) and produces only structural
  document/chunk/summary scaffold nodes, no extracted entities. Set
  `OPENAI_API_KEY`/`OPENAI_BASE_URL` before bringing the `cognee` profile up to
  see genuine entity extraction, and trigger `cognify` yourself — this UI's
  `store` only uploads via Cognee's `add` endpoint (`api/v1/remember`), it does
  not call `cognify`.
- **Mem0** — implemented, but **not Mem0's native graph**. The self-hosted
  OSS package's 2.x line (what this pinned server build actually resolves to)
  dropped Graph Memory entirely — `graph_store`/`GraphStoreFactory`
  (Neo4j-backed) only exist in the `mem0ai` 1.0.x line, and that feature moved
  to Mem0's *hosted* platform product from there (the
  `docs.mem0.ai/platform/graph-memory` docs describe that product, not this
  self-hosted server). Standing up Neo4j and pinning `mem0ai==1.0.11` in the
  Docker build was tried and works mechanically — `/configure` with a
  `graph_store` makes `/search` and `/memories` responses grow a `relations`
  key, no server code changes needed — but downgrading two major versions of a
  shared test harness's core dependency was judged too risky to keep, so it
  was reverted. Instead, `Mem0Graph` (`crates/tinymemory-remote/src/mem0_graph.rs`)
  derives a graph client-side: it lists a namespace's stored entries and runs
  a plain co-occurrence heuristic over each entry's content — no LLM, no NER,
  just grouping runs of capitalized words per sentence and linking consecutive
  ones with predicate `co_occurs_with`. Real computation over real stored
  content, but it will both miss real relations and surface spurious ones;
  `attrs.sentence` on every edge carries the exact source sentence so you can
  judge each one yourself. `kv_*`/`put_relation` return `MemoryError::Other` —
  no native or heuristic counterpart for those.
- **Supermemory** — not implemented. No graph/connections endpoint was found
  on the local lite server by probing its API; nothing to wire up without
  documentation for one.

## A caveat on hosted APIs

Mem0 Cloud and Cognee Cloud use dedicated adapter modes with their respective
authentication schemes. Cognee requires the tenant-specific URL shown on its
API-key dashboard. Supermemory still uses the adapter's self-hosted dialect
against its hosted default, so requests can fail if that hosted API has
diverged. The self-hosted Docker instances above are the deployments verified
end to end by this harness.
