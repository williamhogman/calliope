//! Newtype ids (E1.6) — the id spaces that cross module boundaries.
//!
//! Before this module, chronicle entities, settlements and cultures all
//! travelled as bare `i64`/`usize`; mixing two spaces compiled clean and
//! corrupted silently (an `Event.ids` slot fed a settlement id, say). Each
//! space is now its own type: misuse is a type error. Deposit indices stay
//! raw: they never leave `World::deposits` loops.
//!
//! ADR-0018 split the old `PeopleId` into two axes: `PeopleId` (tongue,
//! gods, arts — the generational clock) and `RealmId` (crown, treasury,
//! wars — the political clock). A settlement carries one of each, and the
//! compiler refuses to let politics read the wrong one.
//!
//! All are `#[serde(transparent)]` — the wire format (pack header,
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

/// People handle — index into `Peoples.peoples` / `Peoples.societies`.
/// The generational axis (ADR-0018): tongue, gods, demonym, arts.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize)]
#[serde(transparent)]
pub struct PeopleId(pub usize);

impl PeopleId {
    #[inline]
    pub fn idx(self) -> usize {
        self.0
    }
}

impl fmt::Display for PeopleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Realm handle — index into `Peoples.realms` and every political table
/// (opinion, dread, asabiyyah, legitimacy, vassalage). The political axis
/// (ADR-0018): crown, house, seat, treasury, wars.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize)]
#[serde(transparent)]
pub struct RealmId(pub usize);

impl RealmId {
    #[inline]
    pub fn idx(self) -> usize {
        self.0
    }
}

impl fmt::Display for RealmId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Civilization handle — index into `Peoples.civs` (M13). The derived
/// tier (ADR-0019): the kinship-closure of peoples plus the realms that
/// carry them. Rows are never deleted; `alive` flips on collapse.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize)]
#[serde(transparent)]
pub struct CivId(pub usize);

impl CivId {
    #[inline]
    pub fn idx(self) -> usize {
        self.0
    }
}

impl fmt::Display for CivId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
