//! Newtype ids (E1.6) — the three id spaces that cross module boundaries.
//!
//! Before this module, chronicle entities, settlements and cultures all
//! travelled as bare `i64`/`usize`; mixing two spaces compiled clean and
//! corrupted silently (an `Event.ids` slot fed a settlement id, say). Each
//! space is now its own type: misuse is a type error. Deposit indices stay
//! raw: they never leave `World::deposits` loops.
//!
//! All four are `#[serde(transparent)]` — the wire format (pack header,
//! tick JSON, entity tables) is byte-identical to the raw integers, so
//! nothing changes for the JS client or the determinism hashes.

use serde::Serialize;
use std::fmt;

/// Chronicle registry handle — index into `Registry.items`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize)]
#[serde(transparent)]
pub struct EntityId(pub i64);

impl EntityId {
    #[inline]
    pub fn idx(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Stable settlement id — survives death and reindexing of the
/// `World.settlements` vec; routes and the client key on it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize)]
#[serde(transparent)]
pub struct SettlementId(pub i64);

impl fmt::Display for SettlementId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Culture handle — index into `World.cultures` / `World.societies`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize)]
#[serde(transparent)]
pub struct CultureId(pub usize);

impl CultureId {
    #[inline]
    pub fn idx(self) -> usize {
        self.0
    }
}

impl fmt::Display for CultureId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
