# `sections`

Typed surfaces over the `<section>:<scope>` namespace convention
(`crates/tinymemory-bus/src/namespace.rs`): `Sections`, `SectionView`, and
`SectionRecall`. Nothing here is a new capability — every call composes
`MemoryCore` and `MemoryRecall`, which every driver implements as supertraits —
this module only stops a caller from hand-concatenating the `conversation:` /
`learning:` / `document:` prefix, where a typo silently produces a different,
valid namespace instead of an error.

## Design

```text
Sections::new(provider)
  ├── conversations()  ─┐
  ├── learnings()       ├─ SectionView  put / get / forget / list
  ├── documents()       │                scopes / list_section
  ├── section(custom)  ─┘
  └── recall()          ── SectionRecall  in_scope / across_section
```

- `Sections` is the entry point: one named accessor per routine section
  (`conversations`, `learnings`, `documents`) plus `section(&MemorySection)` for
  the rest of the vocabulary (`entity:`, `profile:`, `tool:`, `source:`, and
  `Custom`) and `recall()` for the cross-cutting query surface.
- `SectionView` addresses one section by scope — `put` / `get` / `forget` /
  `list` take the bare scope (`"thread-8f21"`), never the prefixed namespace —
  and enumerates it with `scopes()` / `list_section()`.
- `SectionRecall` answers two different questions, deliberately kept apart
  because they cost different amounts: `in_scope` is one provider call;
  `across_section` fans out to one call per namespace in the section.

Every handle borrows `&dyn MemoryProvider` (see `view.rs`, `recall.rs`): cheap
to construct, holds no state between calls, and cannot outlive the provider —
so a caller builds one where it is needed instead of threading it through a
struct.

`MemorySection` is normalised through `MemorySection::from_prefix` in
`SectionView::new`, so `Custom("conversation")` and `MemorySection::Conversation`
are the same view rather than two. Storing the caller's spelling verbatim would
let a write land under `conversation:` while a `scopes()` call — which compares
against this normalised field — reported the section as empty.

## Public surface

- `Sections::{new, conversations, learnings, documents, section, recall}`
- `SectionView::{put, get, forget, list, scopes, list_section}`
- `SectionRecall::{in_scope, across_section}`
- `SectionScope`, `SectionHits` — the value types `scopes()` / recall return
- `MAX_SECTION_NAMESPACES` — the fan-out cap `across_section` enforces
- `NAMESPACE_FILTER_CONFLICT`, `CROSS_SESSION_SECTION_CONFLICT` — the exact
  `MemoryError::Invalid` messages the two recall refusals carry, exposed so a
  caller's test can assert against the same string it sees

## Operational constraints

**`across_section` is a fan-out, not a filtered call.** `OwnedRecallOpts::namespace`
is exact-match, and `namespace: None` means the literal `global` namespace on
the embedded engine but *every* namespace on the reference driver
(`crates/tinymemory-conformance/src/reference/mod.rs`). A single unfiltered call
plus post-filtering would return nothing in production, so `across_section`
enumerates `scopes()` and issues one exact-namespace recall per scope instead,
capped at `MAX_SECTION_NAMESPACES` and reported through `SectionHits::truncated`
when the cap bites. Each namespace is asked for the full `limit`, never a
share of it — a share would let one scope's best hit lose to another's worst.

**`cross_session` is refused outside the conversation section.** The bundled
`UnifiedMemory` driver's `cross_session` recall option only ever surfaces
episodic *conversational* rows from other sessions, and relabels every such row
with whichever namespace the call was pinned to. Honouring `cross_session` on a
`learning:` or `document:` section would therefore return conversational
content mislabeled as that section's own hits, and on `across_section` the same
cross-session rows would repeat once per scope, crowding genuine hits out of
`limit`. Both `in_scope` and `across_section` reject `cross_session` with
`CROSS_SESSION_SECTION_CONFLICT` unless `section == MemorySection::Conversation`.

**Visit order is by entry count descending, not recency.** `SectionScope::last_updated`
is optional and no bundled driver currently populates it, so `scopes()` cannot
order by recency today. This is deliberate and raised as an open question in
`docs/specs/memory-section-api.md`, not an oversight.

**This is not the document intake path.** `Sections::documents` writes through
`MemoryCore`, for text a caller already holds. Handing the memory layer a
*file* — sniffing its format, converting it to markdown, then choosing between
`MemoryIngest`, `MemoryDocuments`, and `MemoryCore` — is `DocumentIntake`'s job
in the `documents` module, which is the right entry point for an upload.

**The `namespace: None` divergence between drivers is out of scope here.** The
embedded engine and the reference driver disagree on what an unfiltered recall
means, as noted above; fixing that divergence needs its own spec and is
deliberately not attempted by this module.
