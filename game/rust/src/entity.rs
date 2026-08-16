//! Entities — the chronicle's cast. Every settlement, culture, ruler,
//! named person, artifact, war, feature and ruin gets one stable id for
//! its whole life; events reference these ids so the telling can be
//! sifted, browsed and cross-linked (M6). Deterministic: ids are handed
//! out in creation order, no wall-clock anywhere.

use serde::Serialize;

#[derive(Serialize, Clone)]
pub struct Entity {
    pub id: i64,
    /// "settlement" | "culture" | "ruler" | "person" | "artifact" |
    /// "war" | "feature" | "ruin"
    pub kind: String,
    pub name: String,
    /// Month of birth / creation (0 = the dawn).
    pub since: i64,
    /// Month of death / destruction, if it has come.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<i64>,
    /// Owning / home culture, when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub culture: Option<usize>,
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
}

impl Registry {
    pub fn add(
        &mut self,
        kind: &str,
        name: &str,
        since: i64,
        culture: Option<usize>,
        x: i64,
        y: i64,
    ) -> i64 {
        let id = self.items.len() as i64;
        self.items.push(Entity {
            id,
            kind: kind.to_string(),
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
        id
    }

    pub fn add_person(
        &mut self,
        name: &str,
        role: &str,
        since: i64,
        culture: Option<usize>,
    ) -> i64 {
        let id = self.add("person", name, since, culture, -1, -1);
        self.items[id as usize].role = role.to_string();
        id
    }

    pub fn get(&self, id: i64) -> Option<&Entity> {
        self.items.get(id as usize)
    }

    pub fn get_mut(&mut self, id: i64) -> Option<&mut Entity> {
        self.items.get_mut(id as usize)
    }

    /// Latest entity carrying this exact name — the living one if any,
    /// else the most recent dead. Names are globally unique (the `taken`
    /// set), so this is the event-resolution workhorse.
    pub fn find(&self, name: &str) -> Option<i64> {
        let mut dead: Option<i64> = None;
        for e in self.items.iter().rev() {
            if e.name == name {
                if e.until.is_none() {
                    return Some(e.id);
                }
                if dead.is_none() {
                    dead = Some(e.id);
                }
            }
        }
        dead
    }

    /// Same, filtered to one kind.
    pub fn find_kind(&self, kind: &str, name: &str) -> Option<i64> {
        let mut dead: Option<i64> = None;
        for e in self.items.iter().rev() {
            if e.kind == kind && e.name == name {
                if e.until.is_none() {
                    return Some(e.id);
                }
                if dead.is_none() {
                    dead = Some(e.id);
                }
            }
        }
        dead
    }

    /// Close an entity's story: record when and how it ended.
    pub fn close(&mut self, id: i64, m: i64, fate: &str) {
        if let Some(e) = self.items.get_mut(id as usize) {
            e.until = Some(m);
            e.fate = fate.to_string();
        }
    }

    /// The living entity of `kind` anchored at (x, y), if any — used to
    /// follow a place through renames and ruin (M9).
    pub fn find_alive(&self, kind: &str, x: i64, y: i64) -> Option<i64> {
        self.items
            .iter()
            .find(|e| e.kind == kind && e.x == x && e.y == y && e.until.is_none())
            .map(|e| e.id)
    }

    /// A place's name changes but its story continues (M9.2/M9.3): keep
    /// the id, swap the name the chronicle will use from here on.
    pub fn rename(&mut self, id: i64, new_name: &str) {
        if let Some(e) = self.items.get_mut(id as usize) {
            e.name = new_name.to_string();
        }
    }

    /// Bump the mention counter and report how many mentions came before —
    /// templates use this to switch to callbacks on re-introduction (M6.8).
    pub fn mention(&mut self, id: i64) -> u32 {
        if let Some(e) = self.items.get_mut(id as usize) {
            let prior = e.mentions;
            e.mentions += 1;
            prior
        } else {
            0
        }
    }

    /// Award an epithet if it is not already carried; returns true when new.
    pub fn earn_epithet(&mut self, id: i64, epithet: &str) -> bool {
        if let Some(e) = self.items.get_mut(id as usize) {
            if !e.epithets.iter().any(|x| x == epithet) {
                e.epithets.push(epithet.to_string());
                return true;
            }
        }
        false
    }

    /// Shift every mapped entity east (the world widened).
    pub fn shift_x(&mut self, dx: i64) {
        for e in self.items.iter_mut() {
            if e.x >= 0 {
                e.x += dx;
            }
        }
    }
}
