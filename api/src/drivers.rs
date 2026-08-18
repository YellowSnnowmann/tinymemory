//! The reserved driver ids.
//!
//! These name the engines this workspace ships. They live in the contract crate
//! rather than in the facade's registry for the same reason
//! [`crate::null::NULL_DRIVER_ID`] already did: an adapter has to spell the id
//! it binds under, and reaching into the facade for it made every adapter
//! depend on the facade — which in turn made the facade unable to depend on the
//! adapters, a package cycle cargo forbids. That cycle is what blocked #18
//! §D1's per-engine features.
//!
//! Reserving them here does **not** mean the contract knows about these
//! engines. It knows their *names*, so that admission can refuse something else
//! binding under one — a host that compiles an adapter out must still reject an
//! impostor claiming its id. The class each id is admitted under stays in the
//! facade's registry, where the trust decision belongs.
//!
//! `tinymemory::registry` re-exports all of these, so existing paths resolve
//! unchanged.

/// The driver id of the bundled TinyCortex embedded engine.
pub const TINYCORTEX_DRIVER_ID: &str = "tinycortex";

/// Driver id of the native Supermemory HTTP adapter.
pub const SUPERMEMORY_DRIVER_ID: &str = "supermemory";

/// Driver id of the native Mem0 HTTP adapter.
pub const MEM0_DRIVER_ID: &str = "mem0";

/// Driver id of the native Cognee HTTP adapter.
pub const COGNEE_DRIVER_ID: &str = "cognee";
