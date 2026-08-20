//! People: contact resolution + scoring — re-exported from the engine.
//!
//! # Why this is a shim
//!
//! The implementation moved down into [`crate::engine::backend::people`]. People is
//! *storage*: a SQLite database of people, handle aliases and interactions,
//! with its own migrations and its own workspace-keyed connection. Storage
//! belongs to the engine, which is what lets the memory contract stay
//! engine-neutral — an engine bound in TinyCortex's place brings its own people
//! store rather than inheriting this one.
//!
//! What is left here is the historical path. `crate::people::{store, types, …}`
//! keeps resolving so the module's own call sites, and the six `store/`
//! references to `people::types`, did not all have to move in the same change.
//!
//! This mirrors [`crate::store::chunks`], which has related the same way to
//! `crate::engine::backend::chunks` since the engine seam was drawn.
//!
//! # The address book rides two gates
//!
//! `address_book`'s macOS reader is gated on `contacts` *and* on the target, in
//! the engine exactly as it was here. This crate's `contacts` feature now
//! forwards to `tinycortex/contacts`; with it off — or anywhere but macOS — the
//! stub returns an empty contact list, so a refresh seeds nothing rather than
//! failing.

pub use crate::engine::backend::people::{
    address_book, migrations, resolver, scorer, store, types,
};
