# The Section API: Conversations, Learnings, Documents, and Recall

**Status:** Draft
**Owner:** TinyMemory maintainers

A typed surface for the three content sections the namespace convention already
names, plus a recall that can span one of them.

## Problem

`docs/specs/graph-view-and-document-intake.md` §3 established the
`<section>:<scope>` namespace convention and `MemorySection` implements it. What
it did not do is give anyone a reason to use it. Every namespace still crosses
the contract as a bare `&str`, so:

1. **Callers concatenate prefixes by hand.** `"conversation:" + thread_id` is
   written at every call site that wants conversational memory, and a typo
   produces a valid, silently-wrong namespace rather than an error. The
   convention is documented and then left to discipline.

2. **There is no way to ask a section-wide question.** "Everything the agent has
   learned" spans every `learning:*` namespace, and the contract offers no way to
   express it. `MemoryCore::list` takes one exact namespace or none;
   `MemoryRecall::recall` takes one exact namespace.

3. **`namespace: None` means two different things on two bundled drivers.** It is
   documented as falling back to `GLOBAL_NAMESPACE` (`recall.rs`), and the
   embedded engine implements exactly that (`memory_trait.rs`) — but the
   reference driver treats it as *all* namespaces
   (`tinymemory-conformance/src/reference/mod.rs`). The conformance suite only
   ever asserts the `Some` case, so nothing catches the divergence.

The third is why the obvious implementation of the second does not work. "Recall
with no namespace filter, then keep the hits whose namespace is in the section"
returns everything on the reference driver and only the `global` namespace on
TinyCortex — correct in tests, empty in production.

## Goals

- One typed surface per section, working on **any** `MemoryProvider` through the
  mandatory three families alone.
- A section-wide recall whose cost, ordering, and truncation are stated rather
  than implied.
- No trait signature change, no new capability, no new error variant, and no
  change to any driver.

## Non-goals

- **HTTP routes or a server crate.** This is a Rust surface. A host that wants
  `/v1/conversations` builds it over this.
- **A `section` filter on `OwnedRecallOpts`.** Three blockers: field parity with
  the borrowed `RecallOpts` is enforced by two exhaustive destructures and a
  test, so it is two structs; eighteen construction sites, most of them struct
  literals without `..Default::default()`; and `RecallOpts` literals exist in
  `vendor/tinycortex`, which this repository must not edit. Above all, a filter
  field is a promise every driver must implement, and one that ignored it would
  silently return wrong results — the failure `audit_provider` exists to prevent.
- **Routing to optional capability families.** `documents()` here writes through
  `MemoryCore`. Handing the layer a *file* is `DocumentIntake`'s job, and it
  already routes between `MemoryIngest`, `MemoryDocuments` and `MemoryCore`.
- **Fixing the `namespace: None` divergence.** It is real and it needs a
  conformance assertion plus a contract sentence. That is its own change; this
  design is built to not depend on it.
- **An optional `query` on recall.** CortexDB's recall returns a filtered slice
  when the query is absent. Here that is `list_section`, because the contract
  already says an empty query yields `Ok(vec![])`.

## Proposed behavior

### 1. `Sections`, the entry point

```rust
let sections = Sections::new(provider.as_ref());

sections.conversations().put("thread-8f21", "turn-3", text, category, None, taint).await?;
let learned = sections.learnings().scopes().await?;
let hits = sections.recall().across_section(&MemorySection::Learning, "async", 10, &opts, None).await?;
```

`Sections`, `SectionView` and `SectionRecall` are borrowing handles, not owners —
the same shape as `DocumentIntake`. They hold `&dyn MemoryProvider` and allocate
nothing but the namespace strings they must build anyway.

`conversations()`, `learnings()` and `documents()` are named accessors returning
the same `SectionView` bound to a different `MemorySection`; `section()` reaches
the other four sections and `Custom`. One parameterised type rather than three
newtypes, because `MemorySection` is a closed vocabulary precisely so it can be a
value.

### 2. `SectionView` — reads and writes within a section

Every method takes the **scope** (`"thread-8f21"`), never the full namespace; the
handle applies the prefix through the existing `Namespace` constructors, so an
invalid scope is a `MemoryError::Invalid` before anything is written.

| Method | Maps onto |
| --- | --- |
| `put` | `MemoryCore::store`, returning the `Namespace` it wrote |
| `get` / `forget` | `MemoryCore::{get, forget}` |
| `list` | `MemoryCore::list` for one scope |
| `list_section` | `MemoryCore::list` fanned out over the section |
| `scopes` | `MemoryCore::namespaces`, filtered to the section |

`put` mirrors `MemoryCore::store`'s parameter order exactly, so the façade is
visibly thin.

### 3. `SectionRecall` — `in_scope` and `across_section`

`in_scope` is one `MemoryRecall::recall` against one exact namespace.

`across_section` enumerates `namespaces()`, keeps the section's, recalls each with
an exact namespace, and merges. This is the same strategy the contract already
uses for `list(None, ..)` in `mandatory/mod.rs`, adopted for the same reason: a
naive delegation returns one namespace and calls it "everything".

It promises, and its rustdoc states:

- **Cost is `1 + N` provider calls**, `N` capped at `MAX_SECTION_NAMESPACES`.
- **Visit order** is by entry count descending, ties by namespace ascending, so
  which namespaces the cap drops is deterministic.
- **Each namespace is asked for the full `limit`**, never a share of it: a share
  would let one namespace's best hit lose to another's worst.
- **Merge order** is score descending, absent scores last, ties by
  `(namespace, key)` ascending — then truncate to `limit`.
- **`truncated` means namespaces were skipped**, never that hits exceeded `limit`.
- **`opts.namespace` must be `None`.** `Some` is `MemoryError::Invalid` carrying
  `NAMESPACE_FILTER_CONFLICT`, rather than a silent override of the caller's filter.

Scores come from separate calls to one driver with one query. They are comparable
in practice on every bundled driver; the contract does not guarantee it, and the
documentation says so rather than pretending otherwise.

## Invariants and constraints

- A `SectionView` never reads or writes a namespace outside its own section.
- `put` then `get` on the same `(scope, key)` round-trips on any retaining driver.
- Every call succeeds on a driver that retains nothing, returning empty rather
  than an error — the surface has no capability-absent path.
- Results are deterministic given a fixed store, on every ordering the API exposes.
- An invalid scope fails before any write, so a rejected call stores nothing.
- No driver, trait signature, capability set, or error enum changes.

## Acceptance criteria

- The full surface works against `NullMemoryProvider`, returning `Ok` and empty.
- A round trip works against `InMemoryProvider` through the public API only.
- `across_section` returns no hit belonging to another section, orders by score
  descending, reports `namespaces_searched`, and sets `truncated` only when the
  namespace cap skipped one.
- `across_section` with `opts.namespace: Some(_)` returns `MemoryError::Invalid`.
- The four contract commands pass, and rustdoc builds with `-D warnings`.

## Open questions

- **One `SectionView` or three newtypes?** Newtypes would let
  `conversations().append()` and `documents().put()` diverge in vocabulary. The
  parameterised type is chosen for now; adding newtypes later is purely additive.
- **Positional `put`, or an `IntakeRequest`-style request struct?** Positional
  mirrors `MemoryCore::store` and is thin; a struct would survive parameter growth.
- **Should an all-sections `everywhere()` exist?** Only as a fan-out over every
  namespace. It cannot be built on `namespace: None` while that means two things.
- **Should the conformance suite pin the `namespace: None` semantics?** Yes — in
  its own change.
