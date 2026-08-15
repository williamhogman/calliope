//! The living chronicle: rulers and successions, wars and peaces, omens,
//! festivals, founding myths. Everything here only *narrates and nudges* —
//! the hard simulation (growth, food, trade) lives in world.rs.

use std::collections::HashSet;

use rand::Rng;
use rand_pcg::Pcg64Mcg;
use serde::Serialize;

use crate::culture::Culture;
use crate::naming::{self, Feature};
use crate::settlements::Settlement;
use crate::world::Event;

// ---------------------------------------------------------------- state

#[derive(Serialize, Clone)]
pub struct Ruler {
    pub culture: usize,
    pub name: String,
    pub epithet: String,
    pub since: i64,
    #[serde(skip)]
    pub age_months: i64,
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

#[derive(Serialize, Clone)]
pub struct War {
    pub a: usize,
    pub b: usize,
    pub until: i64,
    pub name: String,
}

#[derive(Default)]
pub struct ChronicleState {
    pub rulers: Vec<Ruler>,
    pub wars: Vec<War>,
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

const WAR_NAMES: [&str; 12] = [
    "the Salt War", "the Amber War", "the Cattle War", "the Winter War",
    "the War of the Broken Oath", "the Border War", "the Bitter War",
    "the War of Low Tides", "the Nameless War", "the Widows' War",
    "the War of Two Rivers", "the Quarrel of Kings",
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

// ---------------------------------------------------------------- rulers

pub fn new_ruler(
    rng: &mut Pcg64Mcg,
    culture: &Culture,
    taken: &mut HashSet<String>,
    since: i64,
) -> Ruler {
    let name = naming::make_word(rng, &culture.style, taken);
    let epithet = EPITHETS[rng.gen_range(0..EPITHETS.len())].to_string();
    Ruler {
        culture: culture.id,
        name,
        epithet,
        since,
        // takes power somewhere in adult life: 20..45 "years"
        age_months: rng.gen_range(240..540),
    }
}

pub fn init_rulers(
    rng: &mut Pcg64Mcg,
    cultures: &[Culture],
    taken: &mut HashSet<String>,
) -> Vec<Ruler> {
    cultures.iter().map(|c| new_ruler(rng, c, taken, 0)).collect()
}

// ---------------------------------------------------------------- myths

/// Creation myth + one origin line per people. Written once, at the dawn.
pub fn founding_myths(
    rng: &mut Pcg64Mcg,
    cultures: &[Culture],
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
        k: "myth".to_string(),
        text: format!(
            "In the elder dark there was only {}. Then the gods drew {} up from the deep, and the first fires were kindled.",
            ocean, continent
        ),
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
            k: "myth".to_string(),
            text: line,
        });
        let _ = rng; // style banks are deterministic; rng reserved for future variants
    }
    events
}

// ---------------------------------------------------------------- monthly

/// Rulers age and die, wars kindle, rage and gutter out, omens pass over,
/// festivals are held. Returns the month's narrated events.
#[allow(clippy::too_many_arguments)]
pub fn monthly(
    state: &mut ChronicleState,
    rng: &mut Pcg64Mcg,
    taken: &mut HashSet<String>,
    month_abs: i64,
    settlements: &mut [Settlement],
    cultures: &[Culture],
    features: &[Feature],
    world_name: &str,
) -> Vec<Event> {
    let mut events = Vec::new();
    if cultures.is_empty() {
        return events;
    }

    // --- rulers age; the old ones die and heirs take up the circlet
    for ri in 0..state.rulers.len() {
        state.rulers[ri].age_months += 1;
        let age = state.rulers[ri].age_months;
        let p = 0.0006 + ((age - 480).max(0) as f64) * 0.000045;
        if rng.gen::<f64>() < p {
            let cid = state.rulers[ri].culture;
            let culture = &cultures[cid];
            let old_title = state.rulers[ri].title();
            let heir = new_ruler(rng, culture, taken, month_abs);
            let mut heir = heir;
            heir.age_months = rng.gen_range(200..420);
            let text = format!(
                "{} of the {} is laid to rest. {} takes up the circlet.",
                old_title,
                culture.people,
                heir.title()
            );
            events.push(Event {
                m: month_abs,
                s: culture.people.clone(),
                k: "ruler".to_string(),
                text,
            });
            state.rulers[ri] = heir;
        }
    }

    // --- wars end
    let mut ended = Vec::new();
    state.wars.retain(|w| {
        if month_abs >= w.until {
            ended.push(w.clone());
            false
        } else {
            true
        }
    });
    for w in ended {
        events.push(Event {
            m: month_abs,
            s: w.name.clone(),
            k: "war".to_string(),
            text: format!(
                "Peace is sworn between the {} and the {}; {} is over.",
                cultures[w.a].people, cultures[w.b].people, w.name
            ),
        });
    }

    // --- active wars: raids burn the borderlands
    for wi in 0..state.wars.len() {
        if rng.gen::<f64>() < 0.22 {
            let w = &state.wars[wi];
            let (attacker, victim_c) = if rng.gen::<bool>() { (w.a, w.b) } else { (w.b, w.a) };
            let victims: Vec<usize> = settlements
                .iter()
                .enumerate()
                .filter(|(_, s)| s.culture == victim_c && s.pop > 90)
                .map(|(i, _)| i)
                .collect();
            if let Some(&vi) = victims.get(rng.gen_range(0..victims.len().max(1))) {
                let loss = ((settlements[vi].pop as f64 * rng.gen_range(0.02..0.07)) as i64).max(5);
                settlements[vi].pop = (settlements[vi].pop - loss).max(40);
                events.push(Event {
                    m: month_abs,
                    s: settlements[vi].name.clone(),
                    k: "war".to_string(),
                    text: format!(
                        "Raiders of the {} burn the fields of {} — {} souls lost.",
                        cultures[attacker].people, settlements[vi].name, loss
                    ),
                });
            }
        }
    }

    // --- new wars kindle between neighbours
    if cultures.len() >= 2 && state.wars.len() < 2 && rng.gen::<f64>() < 0.0045 {
        let a = rng.gen_range(0..cultures.len());
        let mut b = rng.gen_range(0..cultures.len());
        if b == a {
            b = (b + 1) % cultures.len();
        }
        let already = state.wars.iter().any(|w| {
            (w.a == a && w.b == b) || (w.a == b && w.b == a)
        });
        let a_has = settlements.iter().any(|s| s.culture == a);
        let b_has = settlements.iter().any(|s| s.culture == b);
        if !already && a_has && b_has {
            let name = WAR_NAMES[rng.gen_range(0..WAR_NAMES.len())].to_string();
            let until = month_abs + rng.gen_range(8..30);
            events.push(Event {
                m: month_abs,
                s: name.clone(),
                k: "war".to_string(),
                text: format!(
                    "War kindles between the {} and the {} — men will call it {}.",
                    cultures[a].people, cultures[b].people, name
                ),
            });
            state.wars.push(War { a, b, until, name });
        }
    }

    // --- omens pass over the world
    if rng.gen::<f64>() < 0.0035 {
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
            k: "omen".to_string(),
            text,
        });
    }

    // --- festivals at midsummer and midwinter
    let month = month_abs.rem_euclid(12);
    if month == 5 || month == 11 {
        for c in cultures {
            if rng.gen::<f64>() > 0.10 {
                continue;
            }
            let host = settlements
                .iter_mut()
                .filter(|s| s.culture == c.id)
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
            events.push(Event {
                m: month_abs,
                s: host.name.clone(),
                k: "festival".to_string(),
                text: format!("{} {} in {}.", c.people, what, host.name),
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
    cultures: &[Culture],
    month_abs: i64,
) -> Vec<Event> {
    let mut events = Vec::new();
    let style = cultures
        .get(s.culture)
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
            k: "wonder".to_string(),
            text: format!("{} {}.", s.name, built),
        });
        if !state.had_town {
            state.had_town = true;
            events.push(Event {
                m: month_abs,
                s: s.name.clone(),
                k: "omen".to_string(),
                text: format!(
                    "Travellers carry the word to every shore: {} is the first true town of the age.",
                    s.name
                ),
            });
        }
    } else if s.tier == "City" && !state.had_city {
        state.had_city = true;
        events.push(Event {
            m: month_abs,
            s: s.name.clone(),
            k: "wonder".to_string(),
            text: format!(
                "{} has become a city — the first the world has known. Its walls swallow whole hills.",
                s.name
            ),
        });
    }
    let _ = rng;
    events
}
