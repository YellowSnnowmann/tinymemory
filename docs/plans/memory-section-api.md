# Implementation plan: the Section API

Specification: [`../specs/memory-section-api.md`](../specs/memory-section-api.md).

## Goal

Add `crates/tinymemory/src/sections/` — borrowing handles that give the
`conversation:`, `learning:` and `document:` sections a typed surface, plus a
section-wide recall built by fanning out over `namespaces()`.

## Non-goals for implementation

No HTTP. No change to any trait, driver, capability set or error enum. No
`section` field on `OwnedRecallOpts`. No edit under `vendor/`.

## Assumptions

- The façade crate is the home, not `tinymemory-api`: the contract crate carries
  no `[lints]` table and is held byte-identical to its `tinycortex-api` origin,
  so code required to document `# Errors` belongs where the lints run.
- `tinymemory_conformance::InMemoryProvider` is a public, retaining provider and
  is already an unconditional dev-dependency of the façade. It is the behavioural
  test double; `NullMemoryProvider` covers "retains nothing".
- `tokio` with `macros` and `rt-multi-thread` is already a dev-dependency, so the
  doctest can be fully runnable.

## Tasks

Each task lands its tests first.

1. **`src/sections/types.rs`** — `SectionScope`, `SectionHits`,
   `MAX_SECTION_NAMESPACES`, `NAMESPACE_FILTER_CONFLICT`, and the private merge.
   Tests: merge orders by score descending; absent scores sort last; ties break by
   `(namespace, key)`.
2. **`src/sections/view.rs`, reads and writes** — `SectionView::{new, section,
   namespace, put, get, forget, list}`. Tests: `put` writes under the section
   prefix; `get` reads back what `put` wrote; `forget` is idempotent; an invalid
   scope and an empty scope are both rejected without storing.
3. **`src/sections/view.rs`, section-wide** — `scopes`, `list_section`. Tests:
   only this section's namespaces are listed; unsectioned namespaces are excluded;
   a `Custom` section is never mistaken for a known one; both are empty on a
   provider that retains nothing.
4. **`src/sections/recall.rs`, `in_scope`** — one exact-namespace recall. Tests:
   confined to one namespace; `opts.namespace: Some(_)` returns
   `MemoryError::Invalid` carrying `NAMESPACE_FILTER_CONFLICT`.
5. **`src/sections/recall.rs`, `across_section`** — the fan-out. Tests: merges
   hits from every scope in the section; never returns another section's hit;
   orders by score descending; reports `namespaces_searched`; sets `truncated`
   only past the namespace cap; an empty store is `Ok` and empty.
6. **`src/sections/mod.rs`** — `Sections`, the module `//!` docs, and a runnable
   doctest over `InMemoryProvider`. Wires `#[cfg(test)] mod test;`.
7. **`src/lib.rs`** — `pub mod sections;`, a bullet in the crate docs, and
   `namespace` added to the contract re-export list, which omits it today.
8. **`tests/sections.rs`** — public-API-only regression: a round trip on
   `InMemoryProvider`, and the same script on `NullMemoryProvider` asserting every
   call is `Ok` and empty.
9. **Docs** — a subsection in the root `README.md` after `## The contract`, and
   the section handles named in `examples/tinycortex.rs`.

## Verification

Focused, while iterating:

```sh
cargo test -p tinymemory sections
cargo test --doc -p tinymemory
```

Full, before opening the pull request:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo build --all-targets --all-features
cargo test --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
cargo run -p tinymemory --features tinycortex --example tinycortex
```

## Completion checklist

- [ ] 1 `types.rs` and its tests
- [ ] 2 `SectionView` reads and writes
- [ ] 3 `scopes` and `list_section`
- [ ] 4 `SectionRecall::in_scope`
- [ ] 5 `SectionRecall::across_section`
- [ ] 6 `Sections`, module docs, doctest
- [ ] 7 `lib.rs` exports, including `namespace`
- [ ] 8 integration tests
- [ ] 9 README and example
