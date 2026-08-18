//! The living chronicle: rulers and successions, wars and peaces, omens,
//! festivals, founding myths. Everything here only *narrates and nudges* —
//! the hard simulation (growth, food, trade) lives in world.rs.

use smallvec::smallvec;
use std::collections::HashSet;

use rand::Rng;
use rand_pcg::Pcg64Mcg;
use serde::Serialize;

use crate::ids::{EntityId, RealmId};
use crate::culture::People;
use crate::entity::EntityKind;
use crate::entity::Registry;
use crate::naming::{self, Feature};
use crate::politics::{Claimant, CircletWar, Politics, Realm};
use crate::settlements::Settlement;
use crate::event::EventKind;
use crate::event::Event;

// ---------------------------------------------------------------- state

#[derive(Serialize, Clone)]
pub struct Ruler {
    /// The crown worn (ADR-0018): rulers sit on realms, not peoples.
    pub realm: RealmId,
    pub name: String,
    pub epithet: String,
    pub since: i64,
    #[serde(skip)]
    pub age_months: i64,
    /// Registry id of the person under the circlet (M6.2).
    #[serde(skip)]
    pub ent: EntityId,
}

impl Ruler {
    pub fn title(&self) -> String {
        if self.epithet.is_empty() {
            self.name.clone()
        } else {
            format!("{} {}", self.name, self.epithet)
        }
    }
}

#[derive(Default)]
pub struct ChronicleState {
    pub rulers: Vec<Ruler>,
    pub had_town: bool,
    pub had_city: bool,
}

// ---------------------------------------------------------------- banks

const EPITHETS: [&str; 20] = [
    "the Wise", "the Bold", "the Grey", "Stormborn", "the Oathkeeper",
    "the Young", "the Old", "Longstride", "the Quiet", "Ironhand",
    "the Fair", "the Unlucky", "Seaborn", "the Deep-minded", "Wolf-friend",
    "the Lawgiver", "the Generous", "Half-blind", "the Pilgrim", "the Red",
];


const OMENS: [&str; 9] = [
    "A bearded star hangs in the night sky for a full month; the priests read doom in its tail.",
    "The moon rises red as a forge-fire. Old women bar their doors and count their children.",
    "At midday the sun goes dark, and for a hundred heartbeats the birds fall silent across {W}.",
    "Green fire dances over the northern ice. The skalds call it the road of the dead.",
    "Two dawns are seen in one morning. No two seers agree on what it portends.",
    "A whale of monstrous size is cast up dead upon the shore, and men come from afar to see it.",
    "Rain of small stones falls upon the high pastures. Shepherds swear the sky groaned first.",
    "All the wells taste of iron for nine days, then sweeten again without cause.",
    "A white stag is seen at the edge of {F}, and vanishes when hunters give chase.",
];

/// M3.5 — omens that name a god: {G} god, {D} domain, {P} people.
const OMENS_GOD: [&str; 5] = [
    "{G} is silent this season; the priests of the {P} give a white bull to the fire and wait.",
    "Lightning splits the shrine of {G}, keeper of {D}. The augurs of the {P} quarrel over the meaning.",
    "A child among the {P} speaks three days in the voice of {G}, and afterward remembers nothing.",
    "The offerings to {G} are found untouched by morning — a thing the old ones of the {P} say means a hard year for {D}.",
    "Dreams of {G} trouble every hearth of the {P} on the same night, and no two dreamers dreamed alike.",
];

// ---------------------------------------------------------------- rulers

/// Crown a new ruler for a realm; the name comes in the crown people's
/// tongue (ADR-0018).
pub fn new_ruler(
    rng: &mut Pcg64Mcg,
    realm: &Realm,
    people: &People,
    taken: &mut HashSet<String>,
    since: i64,
    reg: &mut Registry,
) -> Ruler {
    let name = naming::make_word(rng, &people.style, taken);
    let epithet = EPITHETS[rng.gen_range(0..EPITHETS.len())].to_string();
    let ent = reg.add_person(&name, "ruler", since, Some(realm.people));
    reg.earn_epithet(ent, &epithet);
    Ruler {
        realm: realm.id,
        name,
        epithet,
        since,
        // takes power somewhere in adult life: 20..45 "years"
        age_months: rng.gen_range(240..540),
        ent,
    }
}

pub fn init_rulers(
    rng: &mut Pcg64Mcg,
    realms: &[Realm],
    peoples_v: &[People],
    taken: &mut HashSet<String>,
    reg: &mut Registry,
) -> Vec<Ruler> {
    realms
        .iter()
        .map(|r| new_ruler(rng, r, &peoples_v[r.people.idx()], taken, 0, reg))
        .collect()
}

// ---------------------------------------------------------------- myths

/// Creation myth + one origin line per people. Written once, at the dawn.
pub fn founding_myths(
    rng: &mut Pcg64Mcg,
    cultures: &[People],
    features: &[Feature],
    world_name: &str,
) -> Vec<Event> {
    let mut events = Vec::new();
    let ocean = features
        .iter()
        .find(|f| f.t == "ocean")
        .map(|f| f.name.clone())
        .unwrap_or_else(|| "the boundless sea".to_string());
    let continent = features
        .iter()
        .find(|f| f.t == "continent")
        .map(|f| f.name.clone())
        .unwrap_or_else(|| world_name.to_string());
    events.push(Event {
        m: 0,
        s: world_name.to_string(),
        k: EventKind::Myth,
        text: format!(
            "In the elder dark there was only {}. Then the gods drew {} up from the deep, and the first fires were kindled.",
            ocean, continent
        ),
        ..Default::default()
    });
    for c in cultures {
        let range = features.iter().find(|f| f.t == "range").map(|f| f.name.as_str());
        let forest = features.iter().find(|f| f.t == "forest").map(|f| f.name.as_str());
        let line = match c.style.as_str() {
            "hellenic" => format!(
                "The {} say they sprang from sea-foam where {} breaks on white stones.",
                c.people, ocean
            ),
            "nordic" => format!(
                "The {} tell that they were shaped of frost and ash beneath {}.",
                c.people,
                range.unwrap_or("the first mountains")
            ),
            "arid" => format!(
                "The {} remember walking out of the singing sands, following a star that moved.",
                c.people
            ),
            "sylvan" => format!(
                "The {} woke beneath the roots of {}, so their song-keepers swear.",
                c.people,
                forest.unwrap_or("the eldest trees")
            ),
            _ => format!(
                "The {} rode out of the endless grass at the world's edge, and no one rode after.",
                c.people
            ),
        };
        events.push(Event {
            m: 0,
            s: c.people.clone(),
            k: EventKind::Myth,
            text: line,
            ..Default::default()
        });
        // M3.5 — each people names its gods at the dawn of the telling.
        if c.pantheon.len() >= 2 {
            let g0 = &c.pantheon[0];
            let g1 = &c.pantheon[1];
            events.push(Event {
                m: 0,
                s: c.people.clone(),
                k: EventKind::Myth,
                text: format!(
                    "Chief among the gods of the {} stands {}, who holds {}; after them {}, who holds {}.",
                    c.people, g0.name, g0.domain, g1.name, g1.domain
                ),
                ..Default::default()
            });
        }
        let _ = rng; // style banks are deterministic; rng reserved for future variants
    }
    events
}

// ---------------------------------------------------------------- monthly

/// Rulers age and die, omens pass over, festivals are held. War and
/// statecraft live in `politics.rs` — the chronicle keeps the human pulse.
///
/// `pace` is the drama-pacing modifier (M6.4): above 1 in quiet years
/// (the telling reaches for omens and feasts to fill the silence), below
/// 1 when the world is already loud with war and famine.
#[allow(clippy::too_many_arguments)]
pub fn monthly(
    record: &mut Chronicle,
    peoples: &mut Peoples,
    pol: &mut Politics,
    features: &[Feature],
    world_name: &str,
    taken: &mut HashSet<String>,
    month_abs: i64,
    rng: &mut Pcg64Mcg,
    pace: f64,
) -> Vec<Event> {
    let Chronicle { state, registry: reg, .. } = record;
    let Peoples { settlements, peoples: cultures, realms, .. } = peoples;
    let mut events = Vec::new();
    if cultures.is_empty() {
        return events;
    }

    // --- crowns struck from the rolls lose their rulers first
    let fallen: Vec<Ruler> = {
        let (dead, live): (Vec<Ruler>, Vec<Ruler>) = state
            .rulers
            .drain(..)
            .partition(|r| !realms[r.realm.idx()].alive);
        state.rulers = live;
        dead
    };
    for r in fallen {
        reg.close(
            r.ent,
            month_abs,
            &format!("outlived {} — the crown was struck from the rolls", realms[r.realm.idx()].name),
        );
    }

    // --- rulers age; the old ones die and heirs take up the circlet —
    // unless the crown is doubted, in which case the succession opens a
    // war of the circlet (M11.3) and no heir stands unquestioned
    for ri in 0..state.rulers.len() {
        state.rulers[ri].age_months += 1;
        let age = state.rulers[ri].age_months;
        let rid = state.rulers[ri].realm;
        // a throne already in dispute settles by arms, not by age
        if pol.crisis.get(rid.idx()).map_or(false, |x| x.is_some()) {
            continue;
        }
        let p = 0.0006 + ((age - 480).max(0) as f64) * 0.000045;
        if rng.gen::<f64>() < p {
            let realm = &realms[rid.idx()];
            let people = &cultures[realm.people.idx()];
            let old_title = state.rulers[ri].title();
            let old_ent = state.rulers[ri].ent;
            let years = ((month_abs - state.rulers[ri].since) / 12).max(0);
            // the named seat of the realm anchors the succession on the
            // map (M10.4); a mid-month dangling seat falls back to the
            // greatest town under the banner
            let seat = settlements
                .iter()
                .find(|s| s.id == realm.seat && s.realm == rid)
                .or_else(|| {
                    settlements
                        .iter()
                        .filter(|s| s.realm == rid)
                        .max_by_key(|s| s.pop)
                });
            let realm_ent = reg.find_kind(EntityKind::Realm, &realm.name);
            let weak = pol.legit.get(rid.idx()).copied().unwrap_or(1.0) < 0.55;
            let holdings = settlements.iter().filter(|s| s.realm == rid).count();
            if weak && holdings >= 3 && rid.idx() < pol.crisis.len() {
                // M11.3 — the war of the circlet opens: two or three
                // claimants, the first of the old house seated meanwhile.
                // Borders hold; the winner's house rules at the term.
                let k = rng.gen_range(2..=3usize);
                let mut claimants: Vec<Claimant> = Vec::new();
                for j in 0..k {
                    let cname = naming::make_word(rng, &people.style, taken);
                    let house = if j == 0 {
                        realm.house.clone()
                    } else if j == 1 && pol.deposed[rid.idx()].is_some() {
                        pol.deposed[rid.idx()].clone().unwrap()
                    } else {
                        format!("House {}", naming::make_word(rng, &people.style, taken))
                    };
                    let ent = reg.add_person(&cname, "claimant", month_abs, Some(realm.people));
                    claimants.push(Claimant { name: cname, house, ent });
                }
                reg.close(
                    old_ent,
                    month_abs,
                    &format!(
                        "laid to rest after {} years — and the circlet of {} fell into dispute",
                        years, realm.name
                    ),
                );
                reg.earn_epithet(claimants[0].ent, "the Contested");
                let seated = Ruler {
                    realm: rid,
                    name: claimants[0].name.clone(),
                    epithet: "the Contested".to_string(),
                    since: month_abs,
                    age_months: rng.gen_range(240..480),
                    ent: claimants[0].ent,
                };
                let mut ids: crate::event::EventIds = smallvec![old_ent];
                for cl in &claimants {
                    ids.push(cl.ent);
                }
                if let Some(ce) = realm_ent {
                    ids.insert(0, ce);
                }
                events.push(Event {
                    m: month_abs,
                    s: realm.name.clone(),
                    k: EventKind::Ruler,
                    text: format!(
                        "{} of {} is laid to rest, and no heir stands unquestioned: {} lords claim the circlet, and the realm holds its breath.",
                        old_title, realm.name, k
                    ),
                    ids,
                    x: seat.map(|s| s.x).unwrap_or(-1),
                    y: seat.map(|s| s.y).unwrap_or(-1),
                    ..Default::default()
                });
                pol.crisis[rid.idx()] = Some(CircletWar {
                    claimants,
                    seated: 0,
                    ends: month_abs + rng.gen_range(8..30),
                });
                pol.unrest[rid.idx()] = (pol.unrest[rid.idx()] + 0.15).min(1.0);
                state.rulers[ri] = seated;
                continue;
            }
            let mut heir = new_ruler(rng, realm, people, taken, month_abs, reg);
            heir.age_months = rng.gen_range(200..420);
            reg.close(
                old_ent,
                month_abs,
                &format!(
                    "laid to rest after {} years under the circlet of {}",
                    years, realm.name
                ),
            );
            let text = format!(
                "{} of {} is laid to rest. {} of {} takes up the circlet.",
                old_title,
                realm.name,
                heir.title(),
                realm.house
            );
            let mut ids: crate::event::EventIds = smallvec![old_ent, heir.ent];
            if let Some(ce) = realm_ent {
                ids.insert(0, ce);
            }
            events.push(Event {
                m: month_abs,
                s: realm.name.clone(),
                k: EventKind::Ruler,
                text,
                ids,
                x: seat.map(|s| s.x).unwrap_or(-1),
                y: seat.map(|s| s.y).unwrap_or(-1),
                ..Default::default()
            });
            state.rulers[ri] = heir;
        }
    }


    // --- omens pass over the world
    if rng.gen::<f64>() < 0.0035 * pace {
        let raw = OMENS[rng.gen_range(0..OMENS.len())];
        let feat = features
            .iter()
            .find(|f| f.t == "forest" || f.t == "range")
            .map(|f| f.name.clone())
            .unwrap_or_else(|| "the wild hills".to_string());
        let text = raw.replace("{W}", world_name).replace("{F}", &feat);
        events.push(Event {
            m: month_abs,
            s: world_name.to_string(),
            k: EventKind::Omen,
            text,
            ..Default::default()
        });
    }

    // --- and sometimes the gods themselves are read in the signs (M3.5)
    if rng.gen::<f64>() < 0.0022 * pace {
        let c = &cultures[rng.gen_range(0..cultures.len())];
        if c.alive && !c.pantheon.is_empty() {
            let g = &c.pantheon[rng.gen_range(0..c.pantheon.len())];
            let raw = OMENS_GOD[rng.gen_range(0..OMENS_GOD.len())];
            let text = raw
                .replace("{G}", &g.name)
                .replace("{D}", &g.domain)
                .replace("{P}", &c.people);
            events.push(Event {
                m: month_abs,
                s: c.people.clone(),
                k: EventKind::Omen,
                text,
                ..Default::default()
            });
        }
    }

    // --- festivals at midsummer and midwinter; in loud years the feasts
    // thin out (the telling has no room), in quiet ones they carry it
    let month = month_abs.rem_euclid(12);
    if month == 5 || month == 11 {
        for c in cultures.iter() {
            if rng.gen::<f64>() > 0.10 * pace {
                continue;
            }
            if !c.alive {
                continue;
            }
            let host = settlements
                .iter_mut()
                .filter(|s| s.people == c.id)
                .max_by_key(|s| s.pop);
            let Some(host) = host else { continue };
            let what = match (c.style.as_str(), month) {
                ("hellenic", 5) => "holds games in the dust of high summer; runners come barefoot from every deme",
                ("hellenic", _) => "pours dark wine to the winter dead and lights lamps in every doorway",
                ("nordic", 5) => "raises the sun-pole and dances until the pale night gives up",
                ("nordic", _) => "burns the great yule log and swears oaths over the boar",
                ("arid", 5) => "keeps the Feast of Wells, and even enemies may drink unharmed",
                ("arid", _) => "reads the year to come in the smoke of burnt myrrh",
                ("sylvan", 5) => "hangs the oldest oak with ribbons and honey-bread",
                ("sylvan", _) => "walks the woods in silence, leaving bowls of milk at every stump",
                (_, 5) => "races ten thousand horses beneath the open sky",
                _ => "burns sweet grass to the sky-father and feasts for three days",
            };
            host.pop += (host.pop / 100).max(1); // festivals draw folk in
            // M3.5 — feasts are held in a god's name: midsummer belongs to
            // the chief god, midwinter to the keeper of the dead or hearth.
            let dedication = if month == 5 {
                c.pantheon.first()
            } else {
                c.pantheon
                    .iter()
                    .find(|g| g.domain == "the dead" || g.domain == "the hearth")
                    .or_else(|| c.pantheon.first())
            };
            let feast = dedication
                .map(|g| format!(" — the feast of {}, who holds {}", g.name, g.domain))
                .unwrap_or_default();
            events.push(Event {
                m: month_abs,
                s: host.name.clone(),
                k: EventKind::Festival,
                text: format!("{} {} in {}{}.", c.people, what, host.name, feast),
                ..Default::default()
            });
        }
    }

    events
}

/// A settlement has crossed into a new tier — narrate what rises there.
pub fn wonder_for(
    state: &mut ChronicleState,
    rng: &mut Pcg64Mcg,
    s: &Settlement,
    cultures: &[People],
    month_abs: i64,
) -> Vec<Event> {
    let mut events = Vec::new();
    let style = cultures
        .get(s.people.idx())
        .map(|c| c.style.as_str())
        .unwrap_or("hellenic");
    if s.tier == "Town" {
        let built = match style {
            "hellenic" => "raises a marble temple ringed with olive columns",
            "nordic" => "raises a great hall of black timber, its beams carved with serpents",
            "arid" => "raises a ziggurat of sun-baked brick above the wells",
            "sylvan" => "weaves a moot-hall between three standing oaks",
            _ => "raises a stone kurgan crowned with horse-tail banners",
        };
        events.push(Event {
            m: month_abs,
            s: s.name.clone(),
            k: EventKind::Wonder,
            text: format!("{} {}.", s.name, built),
            ..Default::default()
        });
        if !state.had_town {
            state.had_town = true;
            events.push(Event {
                m: month_abs,
                s: s.name.clone(),
                k: EventKind::Omen,
                text: format!(
                    "Travellers carry the word to every shore: {} is the first true town of the age.",
                    s.name
                ),
                ..Default::default()
            });
        }
    } else if s.tier == "City" && !state.had_city {
        state.had_city = true;
        events.push(Event {
            m: month_abs,
            s: s.name.clone(),
            k: EventKind::Wonder,
            text: format!(
                "{} has become a city — the first the world has known. Its walls swallow whole hills.",
                s.name
            ),
            ..Default::default()
        });
    }
    let _ = rng;
    events
}

// ---------------------------------------------------------------- bands

use crate::util::Band;
use crate::state::{Chronicle, Peoples};

/// Diagnostics bands (E2.7): the pace of the telling.
pub const BANDS: &[Band] = &[
    // sweet ceiling raised 40 → 48 with M11: the unrest ladder (riots,
    // charters, coups, circlet wars) is a new legitimate event class.
    Band { name: "events per year", sweet: (2.0, 48.0), hard: (0.5, 100.0), target: "sweet 2–48 · hard 0.5–100" },
    Band { name: "events mappable (coords)", sweet: (0.65, 1.0), hard: (0.45, 1.0), target: "most entries can fly the camera" },
];
