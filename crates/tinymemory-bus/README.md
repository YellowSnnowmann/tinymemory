# tinymemory-bus

The wire contract for the TinyMemory `TinyBus` module, as a library a host
links.

TinyMemory ships as a loadable module so a host does not compile the engine.
`crates/tinymemory-module` exports one object with 89 members on it, and it
ships as a `cdylib` — a host can load it, but it cannot `use` anything out of
it. This crate is what the host compiles against instead:

| module   | what it holds                                                  |
| -------- | -------------------------------------------------------------- |
| `names`  | the bus name, the object path, one constant per member          |
| `types`  | every value type that crosses a frame                           |
| `calls`  | one struct per member: arguments in wire order, plus reply type |
| `wire`   | the error names, and the mapping back to `MemoryError`          |

Four dependencies, none of them heavy: `tinymemory-api` for the types, `serde`
and `serde_json` for the encoding, `thiserror` for one small error enum. No
engine, no storage, no async runtime — and no `tinybus`.

## Why the types are re-exported, not defined

The obvious reading of "a crate that holds the bus types" is a crate that
*defines* them. That would be a mistake, and the repository has already made
the equivalent one once: when `tinymemory-api` was resolved twice, by git and
by path, `MemoryCategory` from one copy was not the same type as
`MemoryCategory` from the other, and the mismatch only surfaced at the seam.
The root `Cargo.toml`'s `[patch]` table exists to prevent exactly that.

Defining structurally identical types here would reproduce it deliberately: the
module would serve `tinymemory_api::` types, the host would hold
`tinymemory_bus::` ones, and every call site would need a conversion whose
correctness nothing checks. So there is one definition, in `tinymemory-api`,
surfaced here. A host gets the types the module serves — the same types, not
equivalents.

## Why not just depend on `tinymemory-api`

It would compile. But `tinymemory-api` is the **driver** contract: it also
carries `MemoryProvider` and its eighteen capability traits, the
mandatory-family composition, the null driver, and the `host::` config sections
a host persists in `config.toml`. A host that loads the module implements none
of that — it makes calls.

This crate is the subset that crosses a frame. What a host compiles against is
what it can actually send and receive, and a trait method that is not exported
on the bus is absent here rather than tempting.

## Why arguments get a struct

`#[tinybus::interface]` puts a method's arguments on the wire as a positional
JSON array, decoded into a tuple on the far side. That is a fine encoding and a
bad thing to write by hand. `Store` takes six arguments:

```json
["work", "standup", "…", "core", null, "internal"]
```

Two are `Option`s, two are enums that serialize as strings, and swapping
`namespace` with `key` produces a call that succeeds and writes the entry to the
wrong place. Nothing on the module side can catch it — both are `String`, in
the right position count, and the engine has no way to know which one the caller
meant.

So a caller fills in named fields and `BusCall::into_args` does the positioning.
The reply type travels with the call for the same reason: `Get` answers
`Option<MemoryEntry>` and `Forget` answers `bool`, both are perfectly good JSON,
and decoding one as the other fails somewhere far from the call.

## There is no client here

This crate holds no connection and no `call()` that sends anything. Two reasons.

A host already owns its connection — its reconnect policy, its timeouts, its
tracing, its own idea of what a memory call costs it. A client here would either
duplicate that or fight it, and the useful part is already in `calls` and
`types`.

And structurally it could not work anyway: `tinybus` is a vendored submodule
whose manifest inherits fields from its own nested `[workspace.package]`, so a
member of this workspace that depends on it makes cargo resolve that inheritance
against the wrong root and fail. That is why `crates/tinymemory-module` is its
own workspace root — see the note on `exclude` in the root `Cargo.toml`. A
contract crate a host links has no business being a separate workspace, so it
stays transport-free.

Wiring it up host-side is small:

```rust,ignore
use tinymemory_bus::calls::BusCall;
use tinymemory_bus::names::{BUS_NAME, OBJECT_PATH};
use tinymemory_bus::{types::MemoryError, wire};

/// Make one call, and give a failure back as the driver's own error type.
async fn call<C: BusCall>(
    connection: &tinybus::Connection,
    call: C,
) -> Result<C::Response, MemoryError> {
    let args = call
        .into_args()
        .map_err(|e| MemoryError::Invalid(e.to_string()))?;

    match connection
        .call(BUS_NAME, OBJECT_PATH, C::METHOD, args)
        .await
    {
        Ok(body) => C::decode_response(body).map_err(|e| MemoryError::Other(e.into())),
        // The name is the contract; `from_wire` is the same table the module
        // mapped out through, so the variant survives the round trip.
        Err(tinybus::Error::MethodFailed { name, message }) => {
            Err(wire::from_wire(&name, &message))
        }
        Err(other) => Err(MemoryError::Other(other.into())),
    }
}
```

`OpenStore` is the one member that needs more than that: it returns an object
*path*, not a value, and calls against that path use the same `BUS_NAME` and the
same member names. Treat `OBJECT_PATH` as the root object rather than the only
one.

## Staying in step with the module

`names::METHODS` lists every member. `crates/tinymemory-module` asserts its
served members against that list, in order, in
`the_served_members_are_exactly_the_published_contract`. Nothing else links the
two — this crate lists members by hand, the module derives them from its
`#[tinybus::interface]` block — so that test is what turns a drift into a
`cargo test` failure instead of an `UnknownMethod` in a host at runtime.

Adding a member is therefore three edits in this crate: a constant in
`names::methods`, an entry in `names::METHODS`, and a call struct in the
matching `calls` family (which `calls::test::COVERED` also lists).
