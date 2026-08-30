# tinymemory-bus

Every type that crosses the TinyMemory `TinyBus` boundary, and the names of the
members that carry them.

TinyMemory ships as a loadable module so a host does not compile the engine:
`crates/tinymemory-module` exports one object with 120 members on it, built as a
`cdylib`. A host can load that binary but cannot `use` anything out of it, so
the payload vocabulary has to be published as an ordinary library. This is it.

| module                                                           | what it holds                                  |
| ---------------------------------------------------------------- | ---------------------------------------------- |
| `names`                                                          | bus name, object path, one constant per member |
| `types`, `chunks`, `recall`, `tree`, `goals`, `tool_memory`, `health`, `capabilities`, `evidence` | the value vocabulary       |
| `provider`                                                       | the value types each capability family exchanges |
| `learning`                                                       | the learning-candidate taxonomy — what a producer asserts about the user, and how strongly |
| `composio`                                                       | the connector-sync vocabulary: run reports, task envelopes, per-connection sync state, scope preferences |
| `error`, `wire`                                                  | `MemoryError` and the name table it round-trips through |
| `version`                                                        | `CONTRACT_VERSION` and the bind rule           |

Seven dependencies, all pure Rust: `serde`, `serde_json`, `chrono`, `sha2`,
`uuid`, `anyhow`, `thiserror`.

## This crate sits underneath `tinymemory-api`

`tinymemory-api` **depends on this crate and re-exports all of it**. That
direction matters, and it is the opposite of the obvious one.

The payload types used to live in `tinymemory-api`. They moved down because a
*host* needs them and needs nothing else in that crate: it loads the module and
makes calls, so it names `MemoryEntry` and `MemoryCategory` but implements no
trait, binds no driver and parses no config. Making it depend on the whole
driver contract to spell a payload type was the wrong shape.

The alternative — a parallel set of payload types for hosts — is worse, and the
repository has already had the equivalent bug: when `tinymemory-api` resolved
twice, `MemoryCategory` from one copy was not the same type as `MemoryCategory`
from the other, and the mismatch only surfaced at the seam. The root
`Cargo.toml`'s `[patch]` table exists to stop that. One definition, here, at the
bottom.

Because the re-export is by module rather than by item, every historical path
keeps resolving unchanged — `tinymemory_api::types::MemoryEntry`,
`tinymemory::MemoryCategory`, `tinycortex::memory::types::*` — and they are the
same items, not twins.

So: a driver author depends on `tinymemory-api` and gets traits and vocabulary.
A host depends on `tinymemory-bus` and gets vocabulary alone.

## What is deliberately absent

**No traits.** `MemoryProvider` and the twenty capability-family traits
describe what an engine must implement, not what a frame carries. They stay in
`tinymemory-api`. The split is readable off the path: a name here is data, a
name there is an obligation.

**No transport.** This crate does not depend on `tinybus` and holds no
connection, client or codec. A host already owns its connection — its reconnect
policy, its timeouts, its tracing — and the useful part is the vocabulary.

That is also structural, not just preference: `tinybus` is vendored as a
submodule whose manifest inherits fields from its own nested
`[workspace.package]`, so a member of this workspace that depends on it makes
cargo resolve that inheritance against the wrong root and fail. It is why
`crates/tinymemory-module` is its own workspace root — see the note on `exclude`
in the root `Cargo.toml`. A crate every workspace member depends on has to stay
transport-free.

**No host configuration, no null driver, no composition helpers.** Those are
`tinymemory-api`'s, and none of them cross a frame.

## Making a call

Arguments travel as a positional JSON array — `#[tinybus::interface]` decodes
them into a tuple — and the member name comes from `names`:

```rust,ignore
use tinymemory_bus::names::{methods, BUS_NAME, OBJECT_PATH};
use tinymemory_bus::types::MemoryEntry;
use tinymemory_bus::wire;

let body = serde_json::json!([namespace, key]);
match connection.call(BUS_NAME, OBJECT_PATH, methods::GET, body).await {
    Ok(reply) => Ok(serde_json::from_value::<Option<MemoryEntry>>(reply)?),
    // The name is the contract, and `from_wire` is the same table the module
    // mapped out through, so the variant survives the round trip.
    Err(tinybus::Error::MethodFailed { name, message }) => {
        Err(wire::from_wire(&name, &message))
    }
    Err(other) => Err(other.into()),
}
```

`OpenStore` is the one member that returns an object *path* rather than a value:
a sibling store under the same workspace, exporting the identical interface.
Treat `OBJECT_PATH` as the root object, not the only one.

## Staying in step with the module

`names::METHODS` lists every member. `crates/tinymemory-module` asserts its
served members against that list, in order, in
`the_served_members_are_exactly_the_published_contract`. Nothing else links the
two — this crate lists members by hand, the module derives them from its
`#[tinybus::interface]` block — so that test is what turns a drift into a
`cargo test` failure instead of an `UnknownMethod` in a host at runtime.

Adding a member is two edits here: a constant in `names::methods` and an entry
in `names::METHODS`.

## Lints

`clippy::pedantic` is deliberately off, matching `tinymemory-tinycortex` and
`tinymemory-remote`. These modules arrived verbatim from `tinymemory-api`, which
opts into no lints at all; switching pedantic on over the move would have buried
a mechanical relocation under several hundred unrelated `#[must_use]` and
backtick edits. Turning it on is worth doing as its own change, over
`tinymemory-api` too, so the contract and the vocabulary stay lint-compatible.
