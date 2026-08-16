//! M6.3 — Artifacts with provenance. Great works leave the forge with a
//! name and an entity id; from then on the chronicle tracks whose hands
//! hold them — won with fallen towns, vanished in sacks, surfacing on a
//! trader's cloth generations later. The provenance IS the event trail.
//!
//! Deterministic: one rng stream (the world's), fixed iteration order.

use std::collections::HashSet;

use rand::Rng;
use rand_pcg::Pcg64Mcg;
use serde::Serialize;

use crate::culture::Culture;
use crate::entity::Registry;
use crate::settlements::Settlement;
use crate::society::Society;
use crate::world::Event;

#[derive(Serialize, Clone)]
pub struct Artifact {
    /// Entity id in the registry — the provenance key.
    pub ent: i64,
    pub name: String,
    /// "crown" | "blade" | "cup" | "torc" | "harp" | "stone"
    pub kind: String,
    /// Settlement id whose treasury holds it; −1 while lost.
    pub holder: i64,
    /// Culture whose smiths wrought it.
    pub maker: usize,
    /// Culture that keeps it now (drifts with conquest).
    pub keeper: usize,
    pub made: i64,
    pub lost: bool,
}

const KINDS: [&str; 6] = ["Crown", "Blade", "Cup", "Torc", "Harp", "Stone"];
const MADE: [&str; 6] = [
    "hammered from river-gold over a winter of nights",
    "folded a hundred times and quenched in oil and prayer",
    "cast in one pour under a favourable star",
    "twisted from silver wire by a smith gone half-blind at it",
    "strung with wire so fine it sings in a still room",
    "cut and polished until it holds a second, deeper light",
];
const ADJ: [&str; 8] = [
    "Winter", "Drowned", "Sundered", "Silent", "Ember", "Moon", "Salt", "Iron",
];
const NOUN: [&str; 8] = [
    "Kings", "Tides", "Oaths", "Sorrows", "Roads", "Stars", "Vows", "Hearths",
];

const CAP: usize = 8;

/// One month of the relics' lives. `month_evs` is everything the world
/// has already logged this month — sacks and transfers move the relics.
#[allow(clippy::too_many_arguments)]
pub fn monthly(
    arts: &mut Vec<Artifact>,
    reg: &mut Registry,
    month_evs: &[Event],
    settlements: &[Settlement],
    cultures: &[Culture],
    socs: &[Society],
    taken: &mut HashSet<String>,
    month: i64,
    rng: &mut Pcg64Mcg,
) -> Vec<Event> {
    let mut events = Vec::new();

    // --- the forging: a settled age, a full treasury, a master smith
    if arts.len() < CAP && month.rem_euclid(7) == 3 {
        for (cid, cu) in cultures.iter().enumerate() {
            let Some(so) = socs.get(cid) else { continue };
            if so.era < 2 || so.treasury < 60.0 {
                continue;
            }
            if rng.gen::<f64>() >= 0.035 {
                continue;
            }
            let Some(home) = settlements
                .iter()
                .filter(|s| s.culture == cid)
                .max_by_key(|s| s.pop)
            else {
                continue;
            };
            let kind = KINDS[rng.gen_range(0..KINDS.len())];
            // named for a god one time in three, else for a phrase of song
            let name = if rng.gen::<f64>() < 0.33 && !cu.pantheon.is_empty() {
                let g = &cu.pantheon[rng.gen_range(0..cu.pantheon.len())];
                format!("the {} of {}", kind, g.name)
            } else {
                format!(
                    "the {} of the {} {}",
                    kind,
                    ADJ[rng.gen_range(0..ADJ.len())],
                    NOUN[rng.gen_range(0..NOUN.len())]
                )
            };
            if taken.contains(&name) {
                continue; // the song already knows that name; wait for another
            }
            taken.insert(name.clone());
            let ent = reg.add("artifact", &name, month, Some(cid), home.x, home.y);
            let made = MADE[rng.gen_range(0..MADE.len())];
            events.push(Event {
                m: month,
                s: name.clone(),
                k: "myth".to_string(),
                text: format!(
                    "In {} the smiths of the {} finish {} — {}. Men come far to look on it.",
                    home.name, cu.people, name, made
                ),
                ids: vec![ent],
                x: home.x,
                y: home.y,
                ..Default::default()
            });
            arts.push(Artifact {
                ent,
                name,
                kind: kind.to_lowercase(),
                holder: home.id,
                maker: cid,
                keeper: cid,
                made: month,
                lost: false,
            });
            break; // at most one forging a month, world-wide
        }
    }

    // --- the wandering
    for ai in 0..arts.len() {
        if arts[ai].lost {
            // lost things surface on a trader's cloth, in the fullness of time
            if rng.gen::<f64>() < 0.0009 && !settlements.is_empty() {
                let si = rng.gen_range(0..settlements.len());
                let s = &settlements[si];
                arts[ai].lost = false;
                arts[ai].holder = s.id;
                arts[ai].keeper = s.culture;
                let name = arts[ai].name.clone();
                if let Some(e) = reg.get_mut(arts[ai].ent) {
                    e.x = s.x;
                    e.y = s.y;
                }
                events.push(Event {
                    m: month,
                    s: name.clone(),
                    k: "myth".to_string(),
                    text: format!(
                        "A trader in {} unwraps {} from oil-cloth, asking a king's price — the relic returns to the light.",
                        s.name, name
                    ),
                    ids: vec![arts[ai].ent],
                    x: s.x,
                    y: s.y,
                    ..Default::default()
                });
            }
            continue;
        }

        let Some(town) = settlements.iter().find(|s| s.id == arts[ai].holder) else {
            // the town is gone from the map; the relic goes into the dark
            arts[ai].lost = true;
            arts[ai].holder = -1;
            continue;
        };

        // conquest: the keeper's banner changed over the treasury
        if town.culture != arts[ai].keeper {
            let old = arts[ai].keeper;
            arts[ai].keeper = town.culture;
            let people = cultures
                .get(town.culture)
                .map(|c| c.people.clone())
                .unwrap_or_default();
            let old_people = cultures.get(old).map(|c| c.people.clone()).unwrap_or_default();
            events.push(Event {
                m: month,
                s: arts[ai].name.clone(),
                k: "myth".to_string(),
                text: format!(
                    "With {} fallen, {} passes from the {} into the hands of the {} — spoils worth more than the walls.",
                    town.name, arts[ai].name, old_people, people
                ),
                ids: vec![arts[ai].ent],
                x: town.x,
                y: town.y,
                ..Default::default()
            });
            continue;
        }

        // a sack or burning at the holder's gates may swallow the relic
        let struck = month_evs
            .iter()
            .any(|e| e.k == "war" && e.s == town.name);
        if struck && rng.gen::<f64>() < 0.12 {
            arts[ai].lost = true;
            arts[ai].holder = -1;
            events.push(Event {
                m: month,
                s: arts[ai].name.clone(),
                k: "myth".to_string(),
                text: format!(
                    "In the burning of {}, {} vanishes from its shrine. Every survivor tells a different thief.",
                    town.name, arts[ai].name
                ),
                ids: vec![arts[ai].ent],
                x: town.x,
                y: town.y,
                ..Default::default()
            });
            continue;
        }

        // and sometimes the treasury simply cannot say where it went
        if rng.gen::<f64>() < 0.0004 {
            arts[ai].lost = true;
            arts[ai].holder = -1;
            events.push(Event {
                m: month,
                s: arts[ai].name.clone(),
                k: "myth".to_string(),
                text: format!(
                    "{} is missed from the treasury of {}; none will say how, and two stewards hang for it.",
                    arts[ai].name, town.name
                ),
                ids: vec![arts[ai].ent],
                x: town.x,
                y: town.y,
                ..Default::default()
            });
        }
    }

    events
}
