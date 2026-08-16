//! Society & technology — each people accumulates lore, discovers arts,
//! passes through the great ages (Stone → Bronze → Iron → the High Age)
//! and binds itself into ever larger polities (tribes → chiefdom →
//! kingdom → empire). Discoveries depend on what the land actually
//! offers: no bronze without copper, no coin without gold or silver,
//! no sail without a shore.

use rand::Rng;
use rand_pcg::Pcg64Mcg;
use serde::Serialize;

use crate::culture::Culture;
use crate::resources::Deposit;
use crate::settlements::{territory_radius, Settlement};
use crate::world::Event;

pub const ERAS: [&str; 4] = [
    "Age of Stone",
    "Age of Bronze",
    "Age of Iron",
    "the High Age",
];

pub const POLITIES: [&str; 4] = ["tribes", "chiefdom", "kingdom", "empire"];
pub const RULER_TITLES: [&str; 4] = ["", "Chief", "King", "Emperor"];

// ---------------------------------------------------------------- techs

pub struct Tech {
    pub id: &'static str,
    pub name: &'static str,
    pub era: usize,
    pub cost: f64,
    /// all of these must already be known
    pub requires: &'static [&'static str],
    /// at least one deposit of these kinds must lie in the people's lands
    pub any_deposit: &'static [&'static str],
    pub needs_coastal: bool,
    /// chronicle line; {P} is replaced with the people's name
    pub flavor: &'static str,
}

pub const TECHS: [Tech; 21] = [
    // -- Age of Stone ------------------------------------------------
    Tech { id: "pottery", name: "pottery", era: 0, cost: 60.0, requires: &[], any_deposit: &[], needs_coastal: false,
        flavor: "The {P} shape river clay into jars and kilns — the harvest no longer rots before spring." },
    Tech { id: "loom", name: "the loom", era: 0, cost: 60.0, requires: &[], any_deposit: &[], needs_coastal: false,
        flavor: "Weavers among the {P} string the first tall looms; good cloth becomes as good as coin." },
    Tech { id: "herb_lore", name: "herb-lore", era: 0, cost: 70.0, requires: &[], any_deposit: &[], needs_coastal: false,
        flavor: "The wise-women of the {P} set down which roots cure and which kill." },
    Tech { id: "stonecraft", name: "stone-cutting", era: 0, cost: 70.0, requires: &[], any_deposit: &["stone"], needs_coastal: false,
        flavor: "Masons of the {P} learn to split stone along its hidden grain." },
    Tech { id: "bow", name: "the bow", era: 0, cost: 50.0, requires: &[], any_deposit: &[], needs_coastal: false,
        flavor: "The {P} learn to bend seasoned yew — no beast nor foe outruns an arrow." },
    // -- Age of Bronze -----------------------------------------------
    Tech { id: "bronze", name: "bronze-working", era: 1, cost: 140.0, requires: &[], any_deposit: &["copper"], needs_coastal: false,
        flavor: "Smiths of the {P} marry copper and tin; the first bronze rings like a bell in the temple square." },
    Tech { id: "plough", name: "the plough", era: 1, cost: 130.0, requires: &["pottery"], any_deposit: &[], needs_coastal: false,
        flavor: "The {P} yoke oxen to a deep-cutting plough, and the heavy earth turns to wealth." },
    Tech { id: "wheel", name: "the wheel", era: 1, cost: 120.0, requires: &[], any_deposit: &[], needs_coastal: false,
        flavor: "Wainwrights of the {P} set carts on true-turning wheels; the roads grow short." },
    Tech { id: "sail", name: "the sail", era: 1, cost: 120.0, requires: &[], any_deposit: &[], needs_coastal: true,
        flavor: "The {P} raise woven sails above their hulls, and the wind carries trade beyond the horizon." },
    Tech { id: "script", name: "writing", era: 1, cost: 180.0, requires: &["pottery"], any_deposit: &[], needs_coastal: false,
        flavor: "Scribes of the {P} press marks into wet clay — words now outlive the speaker." },
    Tech { id: "masonry", name: "masonry", era: 1, cost: 150.0, requires: &["stonecraft"], any_deposit: &[], needs_coastal: false,
        flavor: "The {P} raise dressed-stone walls; raiders find gates where fields used to burn." },
    // -- Age of Iron -------------------------------------------------
    Tech { id: "iron", name: "iron-working", era: 2, cost: 260.0, requires: &["bronze"], any_deposit: &["iron"], needs_coastal: false,
        flavor: "The {P} draw iron from red earth; the age of bronze passes into fire." },
    Tech { id: "coin", name: "coinage", era: 2, cost: 240.0, requires: &["script"], any_deposit: &["gold", "silver"], needs_coastal: false,
        flavor: "The {P} strike their first coin, and every market from coast to coast learns its face." },
    Tech { id: "law", name: "law-codes", era: 2, cost: 220.0, requires: &["script"], any_deposit: &[], needs_coastal: false,
        flavor: "The lawspeakers of the {P} carve a code upon standing stones for every eye to read." },
    Tech { id: "aqueduct", name: "aqueducts", era: 2, cost: 260.0, requires: &["masonry"], any_deposit: &[], needs_coastal: false,
        flavor: "The {P} bring cold mountain water into their streets on arches of stone." },
    Tech { id: "star_charts", name: "star-charts", era: 2, cost: 240.0, requires: &["sail", "script"], any_deposit: &[], needs_coastal: false,
        flavor: "Pilots of the {P} chart the wheeling stars and sail out of sight of land unafraid." },
    Tech { id: "medicine", name: "medicine", era: 2, cost: 250.0, requires: &["herb_lore", "script"], any_deposit: &[], needs_coastal: false,
        flavor: "Physicians of the {P} found a house of healing; fewer are given to the plague-pits." },
    // -- the High Age ------------------------------------------------
    Tech { id: "philosophy", name: "philosophy", era: 3, cost: 380.0, requires: &["law"], any_deposit: &[], needs_coastal: false,
        flavor: "In the colonnades of the {P}, thinkers begin to ask why — and the world grows larger." },
    Tech { id: "engineering", name: "engineering", era: 3, cost: 400.0, requires: &["aqueduct", "iron"], any_deposit: &[], needs_coastal: false,
        flavor: "Engineers of the {P} span gorges and drive true roads through the hills." },
    Tech { id: "steel", name: "steel-smithing", era: 3, cost: 420.0, requires: &["iron"], any_deposit: &[], needs_coastal: false,
        flavor: "The forges of the {P} learn folded steel; their blades keep an edge through a whole war." },
    Tech { id: "mithril_craft", name: "mithril-craft", era: 3, cost: 600.0, requires: &["steel"], any_deposit: &["mithril"], needs_coastal: false,
        flavor: "Deep miners of the {P} bring up mithril, and their smiths work wonders whiter than moonlight." },
];

pub fn tech_by_id(id: &str) -> Option<&'static Tech> {
    TECHS.iter().find(|t| t.id == id)
}

const ERA_DAWNS: [&str; 4] = [
    "", // never emitted
    "The {P} pass into an age of bronze — the old stone tools are laid in barrows.",
    "An age of iron opens for the {P}; the world will not be soft again.",
    "The {P} enter a high age of coin, law and letters.",
];

const ASCENSIONS: [&str; 4] = [
    "", // never emitted
    "The scattered camps of the {P} bind themselves under one chief.",
    "The {P} chiefs bend the knee to one throne — a kingdom is proclaimed.",
    "The banners of the {P} fly over distant shores — men speak now of an empire.",
];

// ---------------------------------------------------------------- state

#[derive(Serialize, Clone)]
pub struct Society {
    pub culture: usize,
    pub era: usize,
    pub polity: usize,
    pub techs: Vec<String>,
    pub knowledge: f64,
    pub treasury: f64,
}

pub fn init(cultures: &[Culture]) -> Vec<Society> {
    cultures
        .iter()
        .map(|c| Society {
            culture: c.id,
            era: 0,
            polity: 0,
            techs: Vec::new(),
            knowledge: 0.0,
            treasury: 25.0,
        })
        .collect()
}

impl Society {
    pub fn knows(&self, id: &str) -> bool {
        self.techs.iter().any(|t| t == id)
    }
}

// -------------------------------------------------------------- modifiers

/// Everything a people's arts change about the hard simulation.
#[derive(Clone)]
pub struct Mods {
    pub growth: f64,       // multiplier on monthly growth rate
    pub capacity: f64,     // multiplier on carrying capacity
    pub trade: f64,        // multiplier on trade income
    pub research: f64,     // multiplier on lore gained
    pub war: f64,          // raid strength when attacking
    pub defense: f64,      // multiplier on raid / quake losses (walls: < 1)
    pub health: f64,       // multiplier on plague / winter losses
    pub colony_range: f64, // multiplier on how far settlers dare to go
    pub production: f64,   // multiplier on goods output value
    pub prospecting: f64,  // multiplier on the odds of finding hidden seams
    /// Kaplan (2017): land per soul ∝ T^−0.5, so carrying capacity rises
    /// as √(arts mastered). Generic — on top of specific arts like plough.
    pub kaplan: f64,
}

impl Default for Mods {
    fn default() -> Self {
        Mods {
            growth: 1.0,
            capacity: 1.0,
            trade: 1.0,
            research: 1.0,
            war: 1.0,
            defense: 1.0,
            health: 1.0,
            colony_range: 1.0,
            production: 1.0,
            prospecting: 1.0,
            kaplan: 1.0,
        }
    }
}

pub fn mods_for(soc: &Society) -> Mods {
    let mut m = Mods::default();
    m.kaplan = (1.0 + 0.16 * soc.techs.len() as f64).sqrt();
    for id in &soc.techs {
        match id.as_str() {
            "pottery" => { m.capacity *= 1.10; m.production *= 1.05; }
            "loom" => { m.production *= 1.10; }
            "herb_lore" => { m.health *= 0.80; }
            "stonecraft" => { m.production *= 1.05; m.prospecting *= 1.25; }
            "bow" => { m.war *= 1.15; }
            "bronze" => { m.war *= 1.30; m.production *= 1.10; m.prospecting *= 1.15; }
            "plough" => { m.growth *= 1.12; m.capacity *= 1.15; }
            "wheel" => { m.trade *= 1.25; }
            "sail" => { m.trade *= 1.20; m.colony_range *= 1.35; }
            "script" => { m.research *= 1.35; m.trade *= 1.10; }
            "masonry" => { m.defense *= 0.65; }
            "iron" => { m.war *= 1.40; m.production *= 1.12; m.prospecting *= 1.30; }
            "coin" => { m.trade *= 1.50; m.production *= 1.08; }
            "law" => { m.defense *= 0.90; m.research *= 1.10; m.growth *= 1.05; }
            "aqueduct" => { m.capacity *= 1.30; m.health *= 0.85; }
            "star_charts" => { m.colony_range *= 1.40; m.research *= 1.15; }
            "medicine" => { m.health *= 0.55; }
            "philosophy" => { m.research *= 1.40; }
            "engineering" => { m.trade *= 1.20; m.capacity *= 1.15; m.prospecting *= 1.45; }
            "steel" => { m.war *= 1.50; }
            "mithril_craft" => { m.production *= 1.20; m.trade *= 1.15; }
            _ => {}
        }
    }
    m
}

// ---------------------------------------------------------------- monthly

/// Resource kinds within reach of a people's settlements.
fn reachable_kinds(cid: usize, settlements: &[Settlement], deposits: &[Deposit]) -> Vec<String> {
    let mut kinds: Vec<String> = Vec::new();
    for s in settlements.iter().filter(|s| s.culture == cid) {
        let r = territory_radius(s.pop) * 2.2;
        let r2 = r * r;
        for d in deposits {
            if !d.known || d.left == 0.0 {
                continue; // no bronze from a seam nobody has found
            }
            let dx = (d.x - s.x) as f64;
            let dy = (d.y - s.y) as f64;
            if dx * dx + dy * dy <= r2 && !kinds.contains(&d.r) {
                kinds.push(d.r.clone());
            }
        }
    }
    kinds
}

/// Lore accumulates, arts are discovered, ages dawn, polities ascend.
pub fn monthly(
    socs: &mut [Society],
    settlements: &[Settlement],
    deposits: &[Deposit],
    cultures: &[Culture],
    month_abs: i64,
    rng: &mut Pcg64Mcg,
) -> Vec<Event> {
    let mut events = Vec::new();
    for si in 0..socs.len() {
        let cid = socs[si].culture;
        let mine: Vec<&Settlement> =
            settlements.iter().filter(|s| s.culture == cid).collect();
        if mine.is_empty() {
            continue;
        }
        let people = cultures[cid].people.clone();
        let m = mods_for(&socs[si]);

        // --- lore: towns think, cities argue, roads carry ideas.
        // Sub-linear in population — a million farmers are not a million
        // scholars — so the arts arrive over generations, not seasons.
        let mut kp = 0.0;
        for s in &mine {
            kp += ((s.pop as f64).sqrt() / 26.0)
                * (1.0 + 0.10 * (s.connections.min(4) as f64));
            if s.tier == "Town" {
                kp += 0.4;
            } else if s.tier == "City" {
                kp += 1.0;
            }
        }
        kp = (kp * m.research).max(0.2);
        socs[si].knowledge += kp;

        // --- discovery: one art at most per month per people. Every art
        // already mastered makes the next dearer — the easy truths go first.
        let dearness = 1.0 + 0.30 * socs[si].techs.len() as f64;
        let kinds = reachable_kinds(cid, settlements, deposits);
        let coastal = mine.iter().any(|s| s.coastal);
        let affordable: Vec<&'static Tech> = TECHS
            .iter()
            .filter(|t| {
                !socs[si].knows(t.id)
                    && socs[si].knowledge >= t.cost * dearness
                    && t.requires.iter().all(|r| socs[si].knows(r))
                    && (t.any_deposit.is_empty()
                        || t.any_deposit.iter().any(|d| kinds.iter().any(|k| k == d)))
                    && (!t.needs_coastal || coastal)
            })
            .collect();
        if !affordable.is_empty() {
            let t = affordable[rng.gen_range(0..affordable.len())];
            socs[si].knowledge -= t.cost * dearness;
            socs[si].techs.push(t.id.to_string());
            events.push(Event {
                m: month_abs,
                s: people.clone(),
                k: "tech".to_string(),
                text: t.flavor.replace("{P}", &people),
                ..Default::default()
            });
            if t.era > socs[si].era {
                socs[si].era = t.era;
                events.push(Event {
                    m: month_abs,
                    s: people.clone(),
                    k: "society".to_string(),
                    text: ERA_DAWNS[t.era].replace("{P}", &people),
                    ..Default::default()
                });
            }
        }

        // --- polity: one step at a time, when the people are ready
        let pop: i64 = mine.iter().map(|s| s.pop).sum();
        let n = mine.len();
        let has_town = mine.iter().any(|s| s.tier == "Town" || s.tier == "City");
        let next = socs[si].polity + 1;
        let ready = match next {
            1 => pop >= 700 && n >= 2,
            2 => pop >= 2600 && has_town && socs[si].knows("script"),
            3 => pop >= 9000 && n >= 4 && socs[si].knows("coin") && socs[si].knows("law"),
            _ => false,
        };
        if ready {
            socs[si].polity = next;
            events.push(Event {
                m: month_abs,
                s: people.clone(),
                k: "society".to_string(),
                text: ASCENSIONS[next].replace("{P}", &people),
                ..Default::default()
            });
        }
    }
    events
}
