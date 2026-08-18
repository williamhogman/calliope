//! M6.3 — Artifacts with provenance. Great works leave the forge with a
//! name and an entity id; from then on the chronicle tracks whose hands
//! hold them — won with fallen towns, vanished in sacks, surfacing on a
//! trader's cloth generations later. The provenance IS the event trail.
//!
//! Deterministic: one rng stream (the world's), fixed iteration order.

use smallvec::smallvec;
use std::collections::HashSet;

use rand::Rng;
use rand_pcg::Pcg64Mcg;
use serde::Serialize;

use crate::ids::{EntityId, PeopleId, RealmId, SettlementId};
use crate::entity::EntityKind;
use crate::event::EventKind;
use crate::event::Event;
use crate::state::{Chronicle, Peoples};

#[derive(Serialize, Clone)]
pub struct Artifact {
    /// Entity id in the registry — the provenance key.
    pub ent: EntityId,
    pub name: String,
    /// "crown" | "blade" | "cup" | "torc" | "harp" | "stone"
    pub kind: String,
    /// Settlement id whose treasury holds it; −1 while lost.
    pub holder: SettlementId,
    /// People whose smiths wrought it (the maker never changes).
    pub maker: PeopleId,
    /// Crown that keeps it now — spoils follow banners, so the keeper
    /// drifts with conquest (ADR-0018: realm axis).
    pub keeper: RealmId,
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
    record: &mut Chronicle,
    month_evs: &[Event],
    peoples: &Peoples,
    taken: &mut HashSet<String>,
    month: i64,
    rng: &mut Pcg64Mcg,
) -> Vec<Event> {
    let Chronicle { artifacts: arts, registry: reg, .. } = record;
    let Peoples { settlements, peoples: cultures, realms, societies: socs, .. } = peoples;
    let mut events = Vec::new();

    // --- the forging: a settled age, a full treasury, a master smith.
    // Monuments (M13.2, raised by the civ pass) don't count against the
    // relic cap — stone is not treasure.
    let relics = arts.iter().filter(|a| a.kind != "monument").count();
    if relics < CAP && month.rem_euclid(7) == 3 {
        for (ci, cu) in cultures.iter().enumerate() {
            let cid = PeopleId(ci);
            let Some(so) = socs.get(ci) else { continue };
            if so.era < 2 {
                continue;
            }
            if rng.gen::<f64>() >= 0.035 {
                continue;
            }
            let Some(home) = settlements
                .iter()
                .filter(|s| s.people == cid)
                .max_by_key(|s| s.pop)
            else {
                continue;
            };
            // the crown over the smiths' town must be able to pay for it
            if realms.get(home.realm.0).map(|r| r.treasury).unwrap_or(0.0) < 60.0 {
                continue;
            }
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
            let ent = reg.add(EntityKind::Artifact, &name, month, Some(cid), home.x, home.y);
            let made = MADE[rng.gen_range(0..MADE.len())];
            events.push(Event {
                m: month,
                s: name.clone(),
                k: EventKind::Myth,
                text: format!(
                    "In {} the smiths of the {} finish {} — {}. Men come far to look on it.",
                    home.name, cu.people, name, made
                ),
                ids: smallvec![ent],
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
                keeper: home.realm,
                made: month,
                lost: false,
            });
            break; // at most one forging a month, world-wide
        }
    }

    // --- the wandering
    for ai in 0..arts.len() {
        // Monuments (M13.2) are architecture, not treasure: they never
        // surface on a trader's cloth, vanish in sacks, or walk out of a
        // treasury. Their keeper still drifts with conquest below, and a
        // dead town leaves them standing among their own ruins.
        let monument = arts[ai].kind == "monument";
        if arts[ai].lost {
            if monument {
                continue; // fallen stone stays fallen
            }
            // lost things surface on a trader's cloth, in the fullness of time
            if rng.gen::<f64>() < 0.0009 && !settlements.is_empty() {
                let si = rng.gen_range(0..settlements.len());
                let s = &settlements[si];
                arts[ai].lost = false;
                arts[ai].holder = s.id;
                arts[ai].keeper = s.realm;
                let name = arts[ai].name.clone();
                reg.relocate(arts[ai].ent, s.x, s.y);
                events.push(Event {
                    m: month,
                    s: name.clone(),
                    k: EventKind::Myth,
                    text: format!(
                        "A trader in {} unwraps {} from oil-cloth, asking a king's price — the relic returns to the light.",
                        s.name, name
                    ),
                    ids: smallvec![arts[ai].ent],
                    x: s.x,
                    y: s.y,
                    ..Default::default()
                });
            }
            continue;
        }

        let Some(town) = settlements.iter().find(|s| s.id == arts[ai].holder) else {
            // the town is gone from the map; the relic goes into the dark —
            // a monument instead stands over the ruin, and the record says so
            arts[ai].lost = true;
            arts[ai].holder = SettlementId(-1);
            if monument {
                let name = arts[ai].name.clone();
                events.push(Event {
                    m: month,
                    s: name.clone(),
                    k: EventKind::Myth,
                    text: format!(
                        "The town beneath {} is gone, but the stone still stands — shepherds fold their flocks in its shadow and cannot read the dedication.",
                        name
                    ),
                    ids: smallvec![arts[ai].ent],
                    ..Default::default()
                });
            }
            continue;
        };

        // conquest: the keeper's banner changed over the treasury
        if town.realm != arts[ai].keeper {
            let old = arts[ai].keeper;
            arts[ai].keeper = town.realm;
            let new_banner = realms
                .get(town.realm.0)
                .map(|r| r.name.clone())
                .unwrap_or_default();
            let old_banner = realms.get(old.0).map(|r| r.name.clone()).unwrap_or_default();
            let text = if monument {
                format!(
                    "The banners of {} now fly from {} — the conquerors of {} chisel their own dedication beside the old one.",
                    new_banner, arts[ai].name, town.name
                )
            } else {
                format!(
                    "With {} fallen, {} passes from {} into the hands of {} — spoils worth more than the walls.",
                    town.name, arts[ai].name, old_banner, new_banner
                )
            };
            events.push(Event {
                m: month,
                s: arts[ai].name.clone(),
                k: EventKind::Myth,
                text,
                ids: smallvec![arts[ai].ent],
                x: town.x,
                y: town.y,
                ..Default::default()
            });
            continue;
        }

        // stone does not vanish in sacks or walk out of treasuries
        if monument {
            continue;
        }

        // a sack or burning at the holder's gates may swallow the relic
        let struck = month_evs
            .iter()
            .any(|e| e.k == EventKind::War && e.s == town.name);
        if struck && rng.gen::<f64>() < 0.12 {
            arts[ai].lost = true;
            arts[ai].holder = SettlementId(-1);
            events.push(Event {
                m: month,
                s: arts[ai].name.clone(),
                k: EventKind::Myth,
                text: format!(
                    "In the burning of {}, {} vanishes from its shrine. Every survivor tells a different thief.",
                    town.name, arts[ai].name
                ),
                ids: smallvec![arts[ai].ent],
                x: town.x,
                y: town.y,
                ..Default::default()
            });
            continue;
        }

        // and sometimes the treasury simply cannot say where it went
        if rng.gen::<f64>() < 0.0004 {
            arts[ai].lost = true;
            arts[ai].holder = SettlementId(-1);
            events.push(Event {
                m: month,
                s: arts[ai].name.clone(),
                k: EventKind::Myth,
                text: format!(
                    "{} is missed from the treasury of {}; none will say how, and two stewards hang for it.",
                    arts[ai].name, town.name
                ),
                ids: smallvec![arts[ai].ent],
                x: town.x,
                y: town.y,
                ..Default::default()
            });
        }
    }

    events
}
