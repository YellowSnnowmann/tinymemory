# The Section API: Conversations, Learnings, Documents, and Recall

**Status:** Implemented
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
  `(namespace, key)` ascending — then truncate to `limit`. Total, because
  `(namespace, key)` is the store's primary key. Scores that are not finite
  numbers rank with the absent ones rather than above every real hit.
- **Visit order is by size, not recency**, because `last_updated` is optional
  and no bundled driver populates it; ordering on it would be ordering on
  `None` and would cost the cap its determinism.
- **`truncated` means namespaces were skipped**, never that hits exceeded `limit`.
- **`opts.namespace` must be `None`.** `Some` is `MemoryError::Invalid` carrying
  `NAMESPACE_FILTER_CONFLICT`, rather than a silent override of the caller's filter.
- **`opts.cross_session` and `opts.session_id` are refused outside the
  conversation section**, with `CROSS_SESSION_SECTION_CONFLICT`, checked
  against the section's *normalised* form so `Custom("conversation")` is
  treated as `MemorySection::Conversation` here too. The bundled driver's
  cross-session path surfaces *episodic* rows from other sessions, and its
  `session_id` path independently appends that session's episodic rows; both
  relabel every such row with whichever namespace the call pinned, so
  honouring either on `learning:` or `document:` would return conversational
  content presented as a learning or a document.
- **`opts.cross_session` and `opts.session_id` are refused on
  `across_section` unconditionally**, including on the conversation section,
  with `CROSS_SESSION_FAN_OUT_CONFLICT`. The driver's episodic augmentation
  for either option runs once, independent of the pinned namespace, so the
  fan-out would repeat the same rows once per scope, crowding genuine hits
  out of `limit` before the fan-out over conversation scopes adds anything —
  `across_section` already visits every conversation scope on its own. A
  caller who wants cross-session or session-scoped recall uses `in_scope`
  instead, which issues exactly one call.

Scores come from separate calls to one driver with one query. They are comparable
in practice on every bundled driver; the contract does not guarantee it, and the
documentation says so rather than pretending otherwise.

### 4. The storage address and the logical namespace

`UnifiedMemory` cannot store a `:` in the value it uses as a namespace: that
string becomes a filesystem directory via `namespace_dir()`, and
`sanitize_namespace` maps every character outside `[A-Za-z0-9\-_/]` to `_` as a
path-traversal defence. So `conversation:thread-8f21` was stored — and
enumerated — as `conversation_thread-8f21`, which `Namespace::parse` reads as
*unsectioned*. Every enumerating call on this surface therefore returned empty
against the production store, after writes that had succeeded.

Widening that allow-list is not the fix. It is what keeps the address path-safe,
`:` is illegal in a Windows filename and denotes an NTFS alternate data stream,
and the sanitiser also performs the PII redaction that keeps a national ID from
becoming a storage address.

So the address and the name are now separate columns. `memory_docs.namespace`
keeps exactly the characters it has today and remains what addresses the row and
names the directory. A new nullable `memory_docs.logical_namespace` carries
`canonical_identifier(namespace)` — the delimiter-preserving form, still
PII-redacted — and `namespace_summaries` reports
`COALESCE(logical_namespace, namespace)`.

The `COALESCE` is the entire backfill, deliberately. A row written before the
migration has `NULL` and keeps exactly its previous behaviour; the upsert clause
sets the column, so such a row heals when it is next written. No migration tries
to turn an old `_` back into a `:` — that mapping is not invertible, because a
scope may legitimately contain `_`, and guessing would silently relabel
unrelated namespaces into a section they were never written to.

The physical address is not injective — `a:b_c` and `a_b:c` both sanitize to
`a_b_c` — so the logical column has to do more than label a summary. `get`,
`list`, and `forget` filter on it too: each addressed read is
`WHERE namespace = ?1 AND (logical_namespace = ?2 OR logical_namespace IS
NULL)`, not `WHERE namespace = ?1` alone. Without the second predicate, listing
`a:b_c` would also surface `a_b:c`'s rows — mislabelled as belonging to the
section that was listed, not the one that wrote them — and the two logical
names would be indistinguishable once written. The `OR logical_namespace IS
NULL` arm is required, not incidental: it is what keeps a pre-migration NULL
row visible under its sanitised address, matching the backfill guarantee above.
Every returned `MemoryEntry.namespace` is the row's own logical name (falling
back to the physical address only for a NULL row), never the caller's query
namespace, so the physical address stays an internal storage detail that never
reaches a `MemoryEntry`.

`namespace_summaries` groups by `COALESCE(logical_namespace, namespace)` for
the same reason: once reads are scoped by logical name, two logical names that
alias one physical address must report two summaries, each with its own count,
or `list` on one reported name would return only half its count while the
other alias never appears in enumeration at all.

One trade-off follows directly from filtering `get`/`forget` on the logical
name: the `UNIQUE(namespace, key)` constraint is still keyed on the physical
address only, so two colliding logical namespaces writing the *same* key still
collide at the storage layer — `ON CONFLICT(namespace, key) DO UPDATE` still
overwrites the row, and `logical_namespace` is set to whichever logical name
wrote it last. A caller addressing that key by the losing logical name's `get`
now returns `None` (the row's `logical_namespace` no longer matches) rather
than the pre-fix behaviour of silently reading the winning write's content.
This surfaces the collision instead of hiding it, but does not resolve it: two
distinct logical namespaces sharing a physical address can still contend for
one key. `idx_memory_docs_ns_updated` is unaffected — it still indexes
`(namespace, updated_at DESC)`, which every addressed query still filters on
first.

`assert_namespaces_preserve_their_section` in the conformance suite now holds
every *retaining* driver to this: a namespace written in a section must be
reported back in that section. It is skipped for a driver that retains nothing,
like the rest of the storage assertions, and it says nothing about a row written
before this change and never rewritten — see the invariant below for the exact
scope. It is the assertion whose absence let the two bundled drivers disagree
unnoticed.

## Invariants and constraints

- A `SectionView` never reads or writes a namespace outside its own section.
- A section is normalised at construction, so `Custom("conversation")` and
  `Conversation` name one view and not two. Without this a write lands in
  `conversation:` while the aliased view reports the section empty — the same
  hazard `Namespace::new` normalises to prevent, one layer up.
- An unusable section is an error, never an empty one: if a section's prefix
  fails validation, the enumerating calls fail rather than reporting no scopes,
  so they agree with the addressed calls about the same section.
- On a retaining driver, a namespace written or rewritten after this change is
  reported back in the section it was written in. A driver may re-address a
  namespace to suit its store, but it may not change which section the name
  belongs to; `assert_namespaces_preserve_their_section` enforces it for every
  retaining driver (`assert_provider` skips it, like the rest of the storage
  assertions, for a driver that accepts writes and discards them). A row
  written before this change and never rewritten keeps enumerating under its
  sanitised, unsectioned name — see "The storage address and the logical
  namespace" above for why that backfill is deliberately a no-op.
- A namespace never reaches the filesystem with a character the path allow-list
  excludes, and the PII redaction on the storage address is unchanged.
- `put` then `get` on the same `(scope, key)` round-trips on any retaining driver.
- Every call succeeds on a driver that retains nothing, returning empty rather
  than an error — the surface has no capability-absent path.
- What a section returns belongs to that section. A recall option that would
  make the driver surface another section's content under this section's
  namespace is refused, not filtered afterwards.
- Results are deterministic given a fixed store, on every ordering the API exposes.
- An invalid scope fails before any write, so a rejected call stores nothing.
- No driver, trait signature, capability set, or error enum changes.

## Acceptance criteria

- The full surface works against `NullMemoryProvider`, returning `Ok` and empty.
- `cross_session` and `session_id` recall are each refused on every section
  but `conversation:` at `in_scope`, and refused on `across_section`
  unconditionally, including on `conversation:`.
- A round trip works against `InMemoryProvider` through the public API only.
- `across_section` returns no hit belonging to another section, orders by score
  descending, reports `namespaces_searched`, and sets `truncated` only when the
  namespace cap skipped one.
- `across_section` with `opts.namespace: Some(_)` returns `MemoryError::Invalid`.
- A sectioned write to the production `UnifiedMemory` store is enumerable
  afterwards: `scopes()` reports it, proven by the tinycortex full-provider
  conformance test against a real on-disk workspace rather than an in-memory
  double.
- The storage address still contains no character outside the path allow-list,
  and a PII-bearing namespace is still redacted in both columns.
- The `logical_namespace` migration is idempotent, and a row predating it still
  enumerates under its sanitised name.
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
- **Should `across_section` visit by recency rather than size?** For
  `conversation:` recency is usually what a caller means, and a host with more
  than `MAX_SECTION_NAMESPACES` conversations currently searches the largest
  rather than the latest. It needs drivers to populate `last_updated` first.
