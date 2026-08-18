//! Entities — the chronicle's cast. Every settlement, culture, ruler,
//! named person, artifact, war, feature and ruin gets one stable id for
//! its whole life; events reference these ids so the telling can be
//! sifted, browsed and cross-linked (M6). Deterministic: ids are handed
//! out in creation order, no wall-clock anywhere.

use crate::ids::{PeopleId, EntityId};
use serde::Serialize;
use std::collections::HashMap;

/// Closed vocabulary of the chronicle's cast (E1.5). Serialized as the
/// same lowercase names the strings used — the wire format is unchanged.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    serde_repr::Serialize_repr,
    strum::Display,
    strum::EnumString,
    strum::IntoStaticStr,
    strum::EnumIter,
)]
#[strum(serialize_all = "lowercase")]
#[repr(u8)]
pub enum EntityKind {
    Artifact,
    Culture,
    Feature,
    Good,
    Person,
    Ruin,
    Settlement,
    War,
    World,
    /// ADR-0018 — a crown: appended after `World` so every existing
    /// kind keeps its wire number.
    Realm,
    /// M13/ADR-0019 — the derived tier: a family of kindred peoples and
    /// the realms that carry them. Appended last; wire numbers hold.
    Civilization,
}

impl EntityKind {
    pub fn name(self) -> &'static str {
        self.into()
    }
}

#[derive(Serialize, Clone)]
pub struct Entity {
    pub id: EntityId,
    pub kind: EntityKind,
    pub name: String,
    /// Month of birth / creation (0 = the dawn).
    pub since: i64,
    /// Month of death / destruction, if it has come.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<i64>,
    /// Owning / home culture, when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub culture: Option<PeopleId>,
    /// Persons: "general" | "prospector" | "founder" | "merchant" | "ruler".
    #[serde(skip_serializing_if = "String::is_empty")]
    pub role: String,
    /// One closing line, written when the entity's story ends.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub fate: String,
    /// Map anchor; -1 when the entity has no fixed place.
    pub x: i64,
    pub y: i64,
    /// Earned epithets, in the order they were earned (M6.8).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub epithets: Vec<String>,
    /// How many times the chronicle has spoken of it (narration memory).
    #[serde(skip)]
    pub mentions: u32,
}

#[derive(Default)]
pub struct Registry {
    pub items: Vec<Entity>,
    /// E5.1 — current name → the ids now carrying it. Names are globally
    /// unique among the living, so each vec is one or two entries; lookups
    /// that used to scan the whole cast become one hash probe. The map is
    /// only ever probed by key (never iterated), so HashMap order cannot
    /// reach any output — determinism holds.
    by_name: HashMap<String, Vec<EntityId>>,
    /// E5.1 — (x, y) → placed entity ids in creation order, for the
    /// follow-the-ground lookups (`find_alive`).
    by_pos: HashMap<(i64, i64), Vec<EntityId>>,
}

impl Registry {
    pub fn add(
        &mut self,
        kind: EntityKind,
        name: &str,
        since: i64,
        culture: Option<PeopleId>,
        x: i64,
        y: i64,
    ) -> EntityId {
        let id = EntityId(self.items.len() as i64);
        self.items.push(Entity {
            id,
            kind,
            name: name.to_string(),
            since,
            until: None,
            culture,
            role: String::new(),
            fate: String::new(),
            x,
            y,
            epithets: Vec::new(),
            mentions: 0,
        });
        self.by_name.entry(name.to_string()).or_default().push(id);
        if x >= 0 {
            self.by_pos.entry((x, y)).or_default().push(id);
        }
        id
    }

    pub fn add_person(
        &mut self,
        name: &str,
        role: &str,
        since: i64,
        culture: Option<PeopleId>,
    ) -> EntityId {
        let id = self.add(EntityKind::Person, name, since, culture, -1, -1);
        self.items[id.idx()].role = role.to_string();
        id
    }

    pub fn get(&self, id: EntityId) -> Option<&Entity> {
        self.items.get(id.idx())
    }

    pub fn get_mut(&mut self, id: EntityId) -> Option<&mut Entity> {
        self.items.get_mut(id.idx())
    }

    /// The one preference rule (E2.6): the most recent match still alive,
    /// else the most recent dead one. E5.1 turned the full-cast reverse
    /// scan into a probe of the name index — candidates are the handful of
    /// ids that ever carried the name; "most recent" is the largest id,
    /// exactly what the old newest-first scan returned.
    fn best_of(&self, ids: &[EntityId], pred: impl Fn(&Entity) -> bool) -> Option<EntityId> {
        let mut alive: Option<EntityId> = None;
        let mut dead: Option<EntityId> = None;
        for &id in ids {
            let e = &self.items[id.idx()];
            if !pred(e) {
                continue;
            }
            let slot = if e.until.is_none() { &mut alive } else { &mut dead };
            if slot.is_none_or(|b| id.0 > b.0) {
                *slot = Some(id);
            }
        }
        alive.or(dead)
    }

    /// Latest entity carrying this exact name — the living one if any,
    /// else the most recent dead. Names are globally unique (the `taken`
    /// set), so this is the event-resolution workhorse.
    pub fn find(&self, name: &str) -> Option<EntityId> {
        let ids = self.by_name.get(name)?;
        self.best_of(ids, |_| true)
    }

    /// Same, filtered to one kind.
    pub fn find_kind(&self, kind: EntityKind, name: &str) -> Option<EntityId> {
        let ids = self.by_name.get(name)?;
        self.best_of(ids, |e| e.kind == kind)
    }

    /// Close an entity's story: record when and how it ended.
    pub fn close(&mut self, id: EntityId, m: i64, fate: &str) {
        if let Some(e) = self.items.get_mut(id.idx()) {
            e.until = Some(m);
            e.fate = fate.to_string();
        }
    }

    /// The living entity of `kind` anchored at (x, y), if any — used to
    /// follow a place through renames and ruin (M9). E5.1: one probe of
    /// the position index; first creation-order match, as the old forward
    /// scan returned.
    pub fn find_alive(&self, kind: EntityKind, x: i64, y: i64) -> Option<EntityId> {
        let ids = self.by_pos.get(&(x, y))?;
        ids.iter()
            .map(|id| &self.items[id.idx()])
            .find(|e| e.kind == kind && e.until.is_none())
            .map(|e| e.id)
    }

    /// A place's name changes but its story continues (M9.2/M9.3): keep
    /// the id, swap the name the chronicle will use from here on.
    pub fn rename(&mut self, id: EntityId, new_name: &str) {
        if let Some(e) = self.items.get_mut(id.idx()) {
            let old = std::mem::replace(&mut e.name, new_name.to_string());
            if let Some(v) = self.by_name.get_mut(&old) {
                v.retain(|&i| i != id);
                if v.is_empty() {
                    self.by_name.remove(&old);
                }
            }
            self.by_name.entry(new_name.to_string()).or_default().push(id);
        }
    }

    /// Bump the mention counter and report how many mentions came before —
    /// templates use this to switch to callbacks on re-introduction (M6.8).
    pub fn mention(&mut self, id: EntityId) -> u32 {
        if let Some(e) = self.items.get_mut(id.idx()) {
            let prior = e.mentions;
            e.mentions += 1;
            prior
        } else {
            0
        }
    }

    /// Award an epithet if it is not already carried; returns true when new.
    pub fn earn_epithet(&mut self, id: EntityId, epithet: &str) -> bool {
        if let Some(e) = self.items.get_mut(id.idx()) {
            if !e.epithets.iter().any(|x| x == epithet) {
                e.epithets.push(epithet.to_string());
                return true;
            }
        }
        false
    }

    /// Move a placed entity — the position index follows (E5.1). All
    /// position writes must come through here or `shift_x`.
    pub fn relocate(&mut self, id: EntityId, x: i64, y: i64) {
        if let Some(e) = self.items.get_mut(id.idx()) {
            if e.x >= 0 {
                if let Some(v) = self.by_pos.get_mut(&(e.x, e.y)) {
                    v.retain(|&i| i != id);
                    if v.is_empty() {
                        self.by_pos.remove(&(e.x, e.y));
                    }
                }
            }
            e.x = x;
            e.y = y;
            if x >= 0 {
                // keep every candidate vec id-sorted (= creation order),
                // the order the old forward scan established
                let v = self.by_pos.entry((x, y)).or_default();
                let at = v.partition_point(|&i| i.0 < id.0);
                v.insert(at, id);
            }
        }
    }

    /// Shift every mapped entity east (the world widened). Positions are
    /// index keys, so the position index is rebuilt (E5.1).
    pub fn shift_x(&mut self, dx: i64) {
        for e in self.items.iter_mut() {
            if e.x >= 0 {
                e.x += dx;
            }
        }
        self.by_pos.clear();
        for e in &self.items {
            if e.x >= 0 {
                self.by_pos.entry((e.x, e.y)).or_default().push(e.id);
            }
        }
    }
}
