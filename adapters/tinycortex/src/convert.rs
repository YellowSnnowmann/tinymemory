//! Value conversions between `tinycortex-api` and `tinymemory-api`.
//!
//! The two contracts describe the same values and, today, describe them
//! identically — `tinymemory-api` was moved out of `tinycortex-api`. They are
//! nonetheless distinct Rust types in distinct crates, so a value has to be
//! rebuilt to cross.
//!
//! ## Every conversion destructures exhaustively
//!
//! This is the whole discipline of this file, and the reason it is not written
//! with `..` or field-by-field assignment onto a `Default`. Two contracts that
//! are allowed to drift *will* drift: someone adds a field to one side, and a
//! lenient conversion silently drops it. A struct literal built from a full
//! destructuring pattern turns that into a compile error on the very next
//! build, naming the field.
//!
//! The same applies to the enums: each `match` lists every variant, so a new
//! category or a third taint level cannot fall into a catch-all arm and be
//! quietly downgraded.
//!
//! ## Taint conversion is the security-relevant one
//!
//! [`tinymemory_api::types::MemoryTaint`] records whether content came from outside. Mapping it
//! wrongly — or defaulting it on an unrecognised value — would let
//! externally-sourced content be treated as internal-trust content. Both sides
//! fail closed to `ExternalSync` when decoding an unknown persisted string, and
//! the mapping here is a total two-arm match with no default, so there is
//! nowhere for a wrong answer to come from.

use tinycortex::memory::types as tc;
use tinymemory_api::types as tm;

/// Converts a category to the TinyMemory contract's form.
#[must_use]
pub fn category_to_tinymemory(category: tc::MemoryCategory) -> tm::MemoryCategory {
    match category {
        tc::MemoryCategory::Core => tm::MemoryCategory::Core,
        tc::MemoryCategory::Daily => tm::MemoryCategory::Daily,
        tc::MemoryCategory::Conversation => tm::MemoryCategory::Conversation,
        tc::MemoryCategory::Custom(name) => tm::MemoryCategory::Custom(name),
    }
}

/// Converts a category to the TinyCortex engine's form.
#[must_use]
pub fn category_to_tinycortex(category: tm::MemoryCategory) -> tc::MemoryCategory {
    match category {
        tm::MemoryCategory::Core => tc::MemoryCategory::Core,
        tm::MemoryCategory::Daily => tc::MemoryCategory::Daily,
        tm::MemoryCategory::Conversation => tc::MemoryCategory::Conversation,
        tm::MemoryCategory::Custom(name) => tc::MemoryCategory::Custom(name),
    }
}

/// Converts provenance to the TinyMemory contract's form.
///
/// A total match with no default arm: see the module docs on why this one may
/// not be lenient.
#[must_use]
pub fn taint_to_tinymemory(taint: tc::MemoryTaint) -> tm::MemoryTaint {
    match taint {
        tc::MemoryTaint::Internal => tm::MemoryTaint::Internal,
        tc::MemoryTaint::ExternalSync => tm::MemoryTaint::ExternalSync,
    }
}

/// Converts provenance to the TinyCortex engine's form.
#[must_use]
pub fn taint_to_tinycortex(taint: tm::MemoryTaint) -> tc::MemoryTaint {
    match taint {
        tm::MemoryTaint::Internal => tc::MemoryTaint::Internal,
        tm::MemoryTaint::ExternalSync => tc::MemoryTaint::ExternalSync,
    }
}

/// Converts an entry to the TinyMemory contract's form.
#[must_use]
pub fn entry_to_tinymemory(entry: tc::MemoryEntry) -> tm::MemoryEntry {
    // Exhaustive destructuring: a field added to the engine's entry breaks this
    // line rather than being dropped on the floor.
    let tc::MemoryEntry {
        id,
        key,
        content,
        namespace,
        category,
        timestamp,
        session_id,
        score,
        taint,
    } = entry;
    tm::MemoryEntry {
        id,
        key,
        content,
        namespace,
        category: category_to_tinymemory(category),
        timestamp,
        session_id,
        score,
        taint: taint_to_tinymemory(taint),
    }
}

/// Converts a namespace summary to the TinyMemory contract's form.
#[must_use]
pub fn namespace_summary_to_tinymemory(summary: tc::NamespaceSummary) -> tm::NamespaceSummary {
    let tc::NamespaceSummary {
        namespace,
        count,
        last_updated,
    } = summary;
    tm::NamespaceSummary {
        namespace,
        count,
        last_updated,
    }
}

/// The owned recall filters, in the engine's owned form.
///
/// Returned owned rather than borrowed because the engine's `RecallOpts`
/// borrows its string fields, and a borrow of a value built inside a conversion
/// function cannot outlive the call. Callers keep this alive and borrow from it.
#[must_use]
pub fn recall_opts_to_tinycortex(opts: &tm::OwnedRecallOpts) -> tc::OwnedRecallOpts {
    let tm::OwnedRecallOpts {
        namespace,
        category,
        session_id,
        min_score,
        cross_session,
    } = opts;
    tc::OwnedRecallOpts {
        namespace: namespace.clone(),
        category: category.clone().map(category_to_tinycortex),
        session_id: session_id.clone(),
        min_score: *min_score,
        cross_session: *cross_session,
    }
}

#[cfg(test)]
#[path = "convert_test.rs"]
mod test;

// ── The reverse direction, for a driver whose contract is TinyMemory's ────────
//
// Everything above converts engine values *into* the TinyMemory contract, which
// is what wrapping TinyCortex as a TinyMemory driver needs. A module-backed
// driver runs the other way: it speaks TinyMemory, and a host whose binding
// still speaks TinyCortex has to convert its answers back.
//
// Same discipline as above — exhaustive destructuring, total matches, no `..`
// and no `Default` — for the same reason: two contracts allowed to drift will.

/// Converts an entry to the `TinyCortex` contract's form.
#[must_use]
pub fn entry_to_tinycortex(entry: tm::MemoryEntry) -> tc::MemoryEntry {
    let tm::MemoryEntry {
        id,
        key,
        content,
        namespace,
        category,
        timestamp,
        session_id,
        score,
        taint,
    } = entry;
    tc::MemoryEntry {
        id,
        key,
        content,
        namespace,
        category: category_to_tinycortex(category),
        timestamp,
        session_id,
        score,
        taint: taint_to_tinycortex(taint),
    }
}

/// Converts a namespace summary to the `TinyCortex` contract's form.
#[must_use]
pub fn namespace_summary_to_tinycortex(summary: tm::NamespaceSummary) -> tc::NamespaceSummary {
    let tm::NamespaceSummary {
        namespace,
        count,
        last_updated,
    } = summary;
    tc::NamespaceSummary {
        namespace,
        count,
        last_updated,
    }
}
