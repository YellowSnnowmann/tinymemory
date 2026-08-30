# composio::catalogs

The curated Composio tool catalogs, and the lookups over them. Composio
publishes 60+ actions per toolkit; most are noise for an agent's planning
loop. Each toolkit that has one gets a hand-curated `&'static [CuratedTool]`
slice that pares the surface down to a useful subset and tags every action
with a [`ToolScope`](../scopes.rs), so a user's scope preference can gate
execution per action.

Moved here from the engine crate (`tinymemory-core`) by OpenHuman#5560: the
catalogs are `&'static str` action slugs with no dependency of any kind, and
the *host* is their heaviest reader (it filters the agent's visible tool
list, renders unlock hints, and decides which connected toolkits get the
"agent-ready" badge). While they lived in the engine crate, every one of
those host reads was a compile-time link to `tinymemory-core`; now a host can
read them by depending on `tinymemory-bus` (or `tinymemory-api`) alone.

## Responsibilities

- Hold one curated `&'static [CuratedTool]` table per catalogued toolkit,
  grouped by category (see Key files).
- Resolve a toolkit slug — including its known aliases and casing — to its
  catalog via [`catalog_for_toolkit`].
- Answer "should this action slug be visible to the agent, given a loaded
  user scope preference?" via [`is_action_visible_with_pref`], falling back
  to [`classify_unknown`] for toolkits with no curated catalog.
- Answer "what scope does this slug require?" via [`curated_scope_for`].
- Provide a short human-readable description of a toolkit for the UI via
  [`toolkit_description`] (`descriptions.rs`).
- Track which catalogued toolkits have a native `ComposioProvider` in the
  engine crate ([`NATIVE_PROVIDERS`]) and how often each one syncs
  ([`native_provider_sync_interval_secs`]), without depending on the engine
  crate's provider trait or registry.

## Key files

| File | Role |
| --- | --- |
| `mod.rs` | Module docs, category re-exports, `CAPABILITY_TOOLKITS`, `catalog_for_toolkit`, `is_action_visible_with_pref`, `curated_scope_for`, `toolkit_has_scope`, `NATIVE_PROVIDERS`, sync-interval helpers. |
| `descriptions.rs` | `toolkit_description` — one short sentence per toolkit slug (including aliases), generic fallback for anything uncatalogued. |
| `business.rs`, `google.rs`, `messaging.rs`, `microsoft.rs`, `productivity.rs`, `social_media.rs` | The category-grouped `CuratedTool` tables (Shopify/Stripe/HubSpot/…, Google apps, Slack/Discord/…, OneDrive/Excel, Outlook/Linear/Jira/…, Twitter/Spotify/YouTube). |
| `github.rs`, `gmail.rs`, `notion.rs`, `linear.rs`, `clickup.rs` | Provider-colocated catalogs for the five toolkits with a native `ComposioProvider` in the engine crate. |
| `mod_tests.rs`, `microsoft_tests.rs`, `productivity_tests.rs` | Module-local unit tests, wired from the bottom of `mod.rs` / the relevant category file with `#[cfg(test)] #[path = "…_tests.rs"] mod tests;`. |

## Public surface

Re-exported from `mod.rs` (and re-exported again from `tinymemory-api::composio::catalogs`,
and from `tinymemory-core::sync::composio::providers::catalogs` for the historical path —
see [`catalogs_compat`](../../../../../tinymemory-core/src/sync/composio/providers/catalogs_compat.rs)
for the six category module names that predate this move):

- `catalog_for_toolkit`, `is_action_visible_with_pref`, `curated_scope_for`, `toolkit_has_scope`, `has_native_provider`
- `CAPABILITY_TOOLKITS`, `NATIVE_PROVIDERS`
- `toolkit_description`
- `sync_interval_env_var`, `parse_sync_interval_override`, `native_provider_sync_interval_secs`
- every category module (`business`, `google`, `messaging`, `microsoft`, `productivity`,
  `social_media`) and every provider-colocated module (`gmail`, `notion`, `github`,
  `linear`, `clickup`), each exporting its `&'static [CuratedTool]` constants.

## Dependencies

None beyond `serde`/`std` (via [`CuratedTool`]/[`ToolScope`] in `../scopes.rs`). This
module must stay dependency-light — see the guard command in
`tinymemory-bus/Cargo.toml`'s doc comment (`cargo tree -p tinymemory-bus -e normal,build
--prefix none | grep -Ei 'rusqlite|libsqlite|git2|reqwest|regex|tokio|tinybus'`, expect no
match) before adding anything here.

## Used by

- `tinymemory-api::host::composio::capability_matrix` — the static integrations-overview
  RPC surface.
- `tinymemory-core::sync::composio::providers` — re-exports every symbol above at its
  historical path so in-engine callers (trigger dispatch, periodic sync, the six native
  `ComposioProvider` impls) keep resolving unchanged.
- The OpenHuman host — filters the agent's visible tool list and renders "agent-ready" /
  unlock-hint UI without linking `tinymemory-core`.

## Notes / gotchas

- `get_provider(..).curated_tools()` (the engine's provider-registry hop) is deliberately
  **not** consulted here. Every native provider's `curated_tools()` was verified to return
  exactly the slice `catalog_for_toolkit` returns for the same toolkit, so the hop was pure
  indirection — see the "`get_provider(..).curated_tools()` is not a separate source"
  section in `mod.rs`'s module docs.
- `resolve_sync_interval_secs` (engine-side) logs via `tracing::warn!` on a malformed
  interval override; `native_provider_sync_interval_secs` here applies the identical rule
  (`parse_sync_interval_override`) silently, because an observability read should not emit
  warnings. Do not add `tracing` to this crate to "fix" that — it is intentional.
