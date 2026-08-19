# TinyMemory

The engine-neutral memory layer for TinyHumans agents.

A host that embeds TinyMemory performs every memory operation through one
contract, and picks which engine answers it by configuration rather than by
recompiling. [TinyCortex](https://github.com/tinyhumansai/tinycortex) is the
default embedded engine; a second engine implements the same traits and binds in
its place without the host learning anything new.

## Layout

```text
api/                    tinymemory-api — the contract. Dependency-light on
                        purpose: depending on it never drags in SQLite, git2,
                        reqwest, or an async runtime.
src/
├── lib.rs              re-exports the contract wholesale, so a host takes one
│                       dependency and the types are the same types
├── registry/           driver admission — which ids exist, what class each
│                       binds as, and the fail-closed external-driver gate
└── mandatory/          the three mandatory capability families, composed once
                        over the `Memory` storage trait
core/                   tinymemory-core — the substance: ingestion, the summary
                        tree, chunk storage, entities, the graph, the diff
                        ledger, goals, tool-memory, and the Composio sync layer.
                        The largest crate here by a wide margin, and the one a
                        real host actually depends on. Unlike `api/` it is not
                        dependency-light: today it links the TinyCortex engine,
                        a bundled SQLite, and an HTTP stack unconditionally.
adapters/
├── tinycortex/         the TinyCortex engine seen through the contract
└── remote/             native HTTP dialects for Supermemory, Mem0, and Cognee
crates/
└── tinymemory-module/  the TinyBus loadable-module driver. Excluded from the
                        workspace on purpose — see the note in `Cargo.toml`.
vendor/
├── tinycortex/         the engine, pinned as a submodule
├── tinyagents/         pinned TinyAgents submodule
└── tinybus/            pinned TinyBus submodule
```

Run `git submodule update --init --recursive` after cloning. Nothing in the
workspace builds without it — `core` names `tinyagents` and `tinycortex` by
path through `vendor/`, so an uninitialized checkout fails at manifest
resolution rather than at compile time, which reads as a confusing error.

## Using from your project

None of these crates are on crates.io yet, so you take the facade by git.
Which patch table you need depends on the engine you pick.

**Remote engines (Supermemory, Mem0, Cognee — hosted or self-hosted) — no patch table:**

```toml
[dependencies]
tinymemory = { git = "https://github.com/tinyhumansai/tinymemory", features = ["supermemory"] }
```

```rust,ignore
use std::sync::Arc;

let backend = tinymemory::remote::SupermemoryMemory::cloud("sm_...")?;
let provider = Arc::new(tinymemory::remote::supermemory_provider(backend));
```

The remote adapter reaches only crates.io dependencies, so cargo resolves it
without any `[patch]` entries.

**The embedded engine (TinyCortex) — vendor this repository as a submodule.**

The remote recipe above works by git because the remote adapter reaches only
published crates. The embedded engine does not: it pulls `tinycortex`,
`tinycortex-api` and `tinyagents`, none of which are published, and
`tinycortex-api` takes `tinymemory-api` *by git*, which cargo will resolve as a
second copy of a crate this workspace also provides by path. Patching that away
needs the crates on disk, so the embedded path is a submodule dependency until
these crates are published:

```sh
git submodule add https://github.com/tinyhumansai/tinymemory vendor/tinymemory
git -C vendor/tinymemory submodule update --init --recursive
```

```toml
[dependencies]
tinymemory = { path = "vendor/tinymemory", features = ["tinycortex"] }

# All four are required. The first three are unpublished crates the engine
# needs; the fourth collapses `tinycortex-api`'s git dependency on
# `tinymemory-api` onto the copy in this tree — without it two distinct
# `tinymemory_api::MemoryEntry` types exist and the seam stops type-checking.
[patch.crates-io]
tinycortex = { path = "vendor/tinymemory/vendor/tinycortex" }
tinycortex-api = { path = "vendor/tinymemory/vendor/tinycortex/api" }
tinyagents = { path = "vendor/tinymemory/vendor/tinyagents" }
[patch."https://github.com/tinyhumansai/tinymemory"]
tinymemory-api = { path = "vendor/tinymemory/api" }
```

This exact patch set is what the reference consumer in `examples/` and the
repository's own root manifest use; a build missing any of the four fails at
resolution, before compiling a line.

```rust,ignore
use std::sync::Arc;
use tinymemory::tinycortex::{provider, InMemoryMemoryStore};

let provider = Arc::new(provider(Arc::new(InMemoryMemoryStore::new())));
```

That is a complete embedded setup for the mandatory three families. The full
eighteen-family engine (`TinycortexProvider`) additionally needs the host
seams (`EmbeddingHost` et al.) installed — see
`adapters/tinycortex/tests/full_provider_conformance.rs` for the minimal
working wiring.

| Feature | Engine | Class | Families served |
| --- | --- | --- | --- |
| `tinycortex` | TinyCortex, in-process | embedded | 3 (mandatory) via `provider`; all 18 via `TinycortexProvider` |
| `supermemory` | Supermemory, hosted | external | 3 (mandatory) |
| `mem0` | Mem0, hosted (`cloud`) or self-hosted | external | 3 (mandatory) |
| `cognee` | Cognee, hosted or self-hosted | external | 3 (mandatory) |
| `memory-git` | add-on: git-backed diff snapshots | — | requires `tinycortex` |
| *(none)* | `NullMemoryProvider` | null | contract + registry only, 40 crates |

The `namespace` driver id you may see in the registry's reserved table is
host-internal: it names `tinymemory-core`'s own store, whose constructors live
in that crate — it is not selectable from the facade.

**A note on remote-engine performance:** recall is native to each hosted API,
but exact-CRUD operations (`get`, `list`, `count`, upsert-by-key) are
enumeration-based — the adapter pages the hosted API to find the record. Fine
for assistant-memory workloads; wrong for high-volume keyed storage.

## The contract

`MemoryProvider` is an object-safe trait with **three mandatory** capability
families and **fifteen optional** ones. The mandatory three are supertraits, so a
driver missing any of them cannot be constructed; the optional fifteen are reached
through `as_ingest()` / `as_tree()` / … accessors that default to `None`, so a
minimal driver implements what it supports and inherits correct absence for
everything else.

A driver's advertised set and its reachable accessors must agree.
`audit_provider` checks exactly that, which turns "advertised but not
implemented" into a detectable, testable mistake rather than a runtime surprise
on the first call.

Capabilities are asked **once, at bind time, and cached**: a host filters its RPC
surface and its agent-tool list from the answer, so a set that changed
afterwards would not be noticed.

## What lives here, and what deliberately does not

| Here | In the host |
| --- | --- |
| the contract; capability negotiation; driver admission; the shared mandatory families; per-engine adapters | RPC surface, agent tools, security policy, credentials, schedulers, event bus, config mapping |

**Policy is not here, on purpose.** Tier enforcement, scope predicates, taint
stamping, redaction, egress checks and audit belong in a decorator the *host*
owns, on the path every caller takes. A driver that could be swapped for one
that skips enforcement is the entire reason the policy layer exists.

## Adding an engine

1. Implement `tinymemory_api::traits::Memory` for the backend, **overriding
   `store_with_taint`** — the trait default silently drops the taint, which
   would launder externally-sourced content into internal-trust content.
2. Wrap it: `MemoryTraitProvider::new(backend, "my-engine")`. That yields a
   driver advertising Core, Recall and Portability, with the four
   easy-to-get-wrong parts (see `src/mandatory/mod.rs`) already handled.
3. Implement any optional families over the engine's own entry points, and
   widen `capabilities()` in lockstep with the accessors.
4. Reserve the driver id: `DriverRegistry::builtin().with_reserved("my-engine", DriverClass::Embedded)`.

## Remote engines

The `tinymemory-remote` crate supports the managed and self-hosted native APIs
of Supermemory and Cognee, plus self-hosted Mem0. Each adapter stores
TinyMemory's key, category, session, and provenance in backend metadata (or a
Cognee raw-data envelope), so exact CRUD and portability survive the seam while
recall remains engine-native. Provider-facing dataset names, container tags,
and filenames are bounded stable hashes, so every namespace and key accepted by
the TinyMemory contract remains valid on the remote API.

```rust
use tinymemory_remote::{SupermemoryMemory, supermemory_provider};

let memory = SupermemoryMemory::self_hosted("http://localhost:6767", "sm_...")?;
let provider = supermemory_provider(memory);
# Ok::<_, anyhow::Error>(provider)
```

Managed APIs have explicit constructors so their authentication cannot be
confused with a self-hosted token:

```rust
use tinymemory_remote::{CogneeMemory, SupermemoryMemory};

// Cognee Cloud issues a per-tenant base URL (the API-key dashboard shows it);
// there is no shared endpoint.
let cognee = CogneeMemory::api("https://tenant-<uuid>.aws.cognee.ai", "cognee-api-key")?;
let supermemory = SupermemoryMemory::cloud("sm_...")?;

// Cognee also issues tenant-specific API origins.
let tenant = CogneeMemory::api("https://tenant.example.cognee.ai", "api-key")?;
# Ok::<_, anyhow::Error>((cognee, supermemory, tenant))
```

Cognee Cloud uses `X-Api-Key`; authenticated self-hosted Cognee uses a bearer
access token. Supermemory uses bearer API keys for both deployment modes. All
constructors redact credentials from `Debug` output and transport errors.

All three advertise the mandatory Core, Recall, and Portability families. The
live Docker harness and conformance command are documented in
[`integration/remote-engines/`](integration/remote-engines/README.md).

## Development

```bash
git submodule update --init --recursive
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Engine adapters name their engines by **version requirement, not path**, so a
host that already pins its own engine checkout unifies onto one copy through its
own `[patch.crates-io]`. The workspace root patches them to the nested `vendor/`
submodules for a standalone build. A path dependency in an adapter would defeat
that and hand a host two copies of one engine with two incompatible `Memory`
traits.
