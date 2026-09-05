//! Society & technology — each people accumulates lore, discovers arts,
//! passes through the great ages (Stone → Bronze → Iron → the High Age)
//! and binds itself into ever larger polities (tribes → chiefdom →
//! kingdom → empire). Discoveries depend on what the land actually
//! offers: no bronze without copper, no coin without gold or silver,
//! no sail without a shore.

use rand::Rng;
use rand_pcg::Pcg64Mcg;
use serde::Serialize;

use crate::ids::PeopleId;
use crate::culture::People;
use crate::resources::{Deposit, Good, GoodSet};
use crate::settlements::{territory_radius, Settlement};
use crate::event::EventKind;
use crate::event::Event;
use crate::state::Peoples;

pub const ERAS: [&str; 4] = [
    "Age of Stone",
    "Age of Bronze",
    "Age of Iron",
    "the High Age",
];

pub const POLITIES: [&str; 4] = ["tribes", "chiefdom", "kingdom", "empire"];
pub const RULER_TITLES: [&str; 4] = ["", "Chief", "King", "Emperor"];

// ---------------------------------------------------------------- techs

/// The 21 arts, one bit each (E1.9). Declared in `TECHS` order so
/// `TECHS[id as usize]` is the id's row; serialized as the same
/// snake_case ids the strings used.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    Serialize,
    strum::Display,
    strum::EnumString,
    strum::IntoStaticStr,
    strum::EnumCount,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum TechId {
    Pottery,
    Loom,
    HerbLore,
    Stonecraft,
    Bow,
    Bronze,
    Plough,
    Wheel,
    Sail,
    Script,
    Masonry,
    Iron,
    Coin,
    Law,
    Aqueduct,
    StarCharts,
    Medicine,
    Philosophy,
    Engineering,
    Steel,
    MithrilCraft,
}

impl TechId {
    #[inline]
    pub const fn bit(self) -> u32 {
        1 << self as u32
    }
}

/// Which arts a people has mastered — one u32, O(1) membership (E1.9).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct TechSet(u32);

impl TechSet {
    pub const EMPTY: TechSet = TechSet(0);

    #[inline]
    pub fn insert(&mut self, t: TechId) {
        self.0 |= t.bit();
    }

    #[inline]
    pub fn contains(self, t: TechId) -> bool {
        self.0 & t.bit() != 0
    }
}


pub struct Tech {
    pub id: TechId,
    pub name: &'static str,
    pub era: usize,
    pub cost: f64,
    /// all of these must already be known
    pub requires: &'static [TechId],
    /// at least one deposit of these kinds must lie in the people's lands
    pub any_deposit: &'static [Good],
    pub needs_coastal: bool,
    /// chronicle line; {P} is replaced with the people's name
    pub flavor: &'static str,
}

pub const TECHS: [Tech; 21] = [
    // -- Age of Stone ------------------------------------------------
    Tech { id: TechId::Pottery, name: "pottery", era: 0, cost: 60.0, requires: &[], any_deposit: &[], needs_coastal: false,
        flavor: "The {P} shape river clay into jars and kilns — the harvest no longer rots before spring." },
    Tech { id: TechId::Loom, name: "the loom", era: 0, cost: 60.0, requires: &[], any_deposit: &[], needs_coastal: false,
        flavor: "Weavers among the {P} string the first tall looms; good cloth becomes as good as coin." },
    Tech { id: TechId::HerbLore, name: "herb-lore", era: 0, cost: 70.0, requires: &[], any_deposit: &[], needs_coastal: false,
        flavor: "The wise-women of the {P} set down which roots cure and which kill." },
    Tech { id: TechId::Stonecraft, name: "stone-cutting", era: 0, cost: 70.0, requires: &[], any_deposit: &[Good::Stone], needs_coastal: false,
        flavor: "Masons of the {P} learn to split stone along its hidden grain." },
    Tech { id: TechId::Bow, name: "the bow", era: 0, cost: 50.0, requires: &[], any_deposit: &[], needs_coastal: false,
        flavor: "The {P} learn to bend seasoned yew — no beast nor foe outruns an arrow." },
    // -- Age of Bronze -----------------------------------------------
    Tech { id: TechId::Bronze, name: "bronze-working", era: 1, cost: 140.0, requires: &[], any_deposit: &[Good::Copper], needs_coastal: false,
        flavor: "Smiths of the {P} marry copper and tin; the first bronze rings like a bell in the temple square." },
    Tech { id: TechId::Plough, name: "the plough", era: 1, cost: 130.0, requires: &[TechId::Pottery], any_deposit: &[], needs_coastal: false,
        flavor: "The {P} yoke oxen to a deep-cutting plough, and the heavy earth turns to wealth." },
    Tech { id: TechId::Wheel, name: "the wheel", era: 1, cost: 120.0, requires: &[], any_deposit: &[], needs_coastal: false,
        flavor: "Wainwrights of the {P} set carts on true-turning wheels; the roads grow short." },
    Tech { id: TechId::Sail, name: "the sail", era: 1, cost: 120.0, requires: &[], any_deposit: &[], needs_coastal: true,
        flavor: "The {P} raise woven sails above their hulls, and the wind carries trade beyond the horizon." },
    Tech { id: TechId::Script, name: "writing", era: 1, cost: 180.0, requires: &[TechId::Pottery], any_deposit: &[], needs_coastal: false,
        flavor: "Scribes of the {P} press marks into wet clay — words now outlive the speaker." },
    Tech { id: TechId::Masonry, name: "masonry", era: 1, cost: 150.0, requires: &[TechId::Stonecraft], any_deposit: &[], needs_coastal: false,
        flavor: "The {P} raise dressed-stone walls; raiders find gates where fields used to burn." },
    // -- Age of Iron -------------------------------------------------
    Tech { id: TechId::Iron, name: "iron-working", era: 2, cost: 260.0, requires: &[TechId::Bronze], any_deposit: &[Good::Iron], needs_coastal: false,
        flavor: "The {P} draw iron from red earth; the age of bronze passes into fire." },
    Tech { id: TechId::Coin, name: "coinage", era: 2, cost: 240.0, requires: &[TechId::Script], any_deposit: &[Good::Gold, Good::Silver], needs_coastal: false,
        flavor: "The {P} strike their first coin, and every market from coast to coast learns its face." },
    Tech { id: TechId::Law, name: "law-codes", era: 2, cost: 220.0, requires: &[TechId::Script], any_deposit: &[], needs_coastal: false,
        flavor: "The lawspeakers of the {P} carve a code upon standing stones for every eye to read." },
    Tech { id: TechId::Aqueduct, name: "aqueducts", era: 2, cost: 260.0, requires: &[TechId::Masonry], any_deposit: &[], needs_coastal: false,
        flavor: "The {P} bring cold mountain water into their streets on arches of stone." },
    Tech { id: TechId::StarCharts, name: "star-charts", era: 2, cost: 240.0, requires: &[TechId::Sail, TechId::Script], any_deposit: &[], needs_coastal: false,
        flavor: "Pilots of the {P} chart the wheeling stars and sail out of sight of land unafraid." },
    Tech { id: TechId::Medicine, name: "medicine", era: 2, cost: 250.0, requires: &[TechId::HerbLore, TechId::Script], any_deposit: &[], needs_coastal: false,
        flavor: "Physicians of the {P} found a house of healing; fewer are given to the plague-pits." },
    // -- the High Age ------------------------------------------------
    Tech { id: TechId::Philosophy, name: "philosophy", era: 3, cost: 380.0, requires: &[TechId::Law], any_deposit: &[], needs_coastal: false,
        flavor: "In the colonnades of the {P}, thinkers begin to ask why — and the world grows larger." },
    Tech { id: TechId::Engineering, name: "engineering", era: 3, cost: 400.0, requires: &[TechId::Aqueduct, TechId::Iron], any_deposit: &[], needs_coastal: false,
        flavor: "Engineers of the {P} span gorges and drive true roads through the hills." },
    Tech { id: TechId::Steel, name: "steel-smithing", era: 3, cost: 420.0, requires: &[TechId::Iron], any_deposit: &[], needs_coastal: false,
        flavor: "The forges of the {P} learn folded steel; their blades keep an edge through a whole war." },
    Tech { id: TechId::MithrilCraft, name: "mithril-craft", era: 3, cost: 600.0, requires: &[TechId::Steel], any_deposit: &[Good::Mithril], needs_coastal: false,
        flavor: "Deep miners of the {P} bring up mithril, and their smiths work wonders whiter than moonlight." },
];

/// M15.2 — the REQUIRES vocabulary bridge. `Good::requires()` labels are
/// client-facing gate names; every one must resolve here, and the assay
/// proves it. Three kinds: a tech's own name in `TECHS`, a family label
/// covering several arts, or a folkway — a pre-tech art every people
/// knows from the founding, which gates nothing.
pub const FOLKWAYS: [&str; 3] = ["farming", "fishing", "gathering"];

/// Family labels: one gate name, several qualifying arts.
pub const TECH_FAMILIES: [(&str, &[TechId]); 2] = [
    ("metal-working", &[TechId::Bronze, TechId::Iron]),
    ("mithril-smithing", &[TechId::MithrilCraft]),
];

/// Does a `Good::requires()` label name something real?
pub fn requires_resolves(label: &str) -> bool {
    TECHS.iter().any(|t| t.name == label)
        || FOLKWAYS.contains(&label)
        || TECH_FAMILIES.iter().any(|(f, ids)| *f == label && !ids.is_empty())
}

/// The id's row in `TECHS` — variant order and table order are one.
pub fn tech(id: TechId) -> &'static Tech {
    &TECHS[id as usize]
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
    pub people: usize,
    pub era: usize,
    pub polity: usize,
    pub techs: Vec<TechId>,
    /// Bitset mirror of `techs` for O(1) `knows` (E1.9); rebuilt nowhere —
    /// the two are only ever written together.
    #[serde(skip)]
    pub known: TechSet,
    pub knowledge: f64,
    /// M13.2 — golden-age research pace, owned by the civ pass: reset to
    /// 1.0 every civ year, raised for members of golden civilizations.
    /// Off the wire; the arts it buys are what the client sees.
    #[serde(skip)]
    pub boon: f64,
}

pub fn init(peoples: &[People]) -> Vec<Society> {
    peoples
        .iter()
        .map(|c| Society {
            people: c.id.0,
            era: 0,
            polity: 0,
            techs: Vec::new(),
            known: TechSet::EMPTY,
            knowledge: 0.0,
            boon: 1.0,
        })
        .collect()
}

impl Society {
    #[inline]
    pub fn knows(&self, id: TechId) -> bool {
        self.known.contains(id)
    }

    /// M96 — the storage tier a people's arts allow: the highest of the
    /// three crafts they know. Pure in the known set.
    #[inline]
    pub fn store_tier(&self) -> StoreTier {
        StoreTier::of(self)
    }
}

// ------------------------------------------------------------ M96 stores

/// M96 — *Granaries Against Lean Years*: how a people keeps grain past
/// the year it grew. Three crafts, three tiers, one ladder — each tier
/// keeps a larger share of a fat year's surplus, loses less of it to
/// damp, rot and vermin each year, and can hold more of it before the
/// pile outgrows the roofs. The tier is a property of the people's arts;
/// the store itself is per town (`Settlement::store`, person-years).
///
/// Anchors: sealed jars and pits keep grain a season or two (Neolithic
/// Near East, Çatalhöyük — losses of a quarter and more to pests are
/// the ethnographic norm for open household storage); raised masonry
/// granaries with ventilated floors keep it years (Harappa, the Egyptian
/// *shunet*, Hallstatt-era stilt granaries); and the storehouse proper is
/// an institution, not a building — Joseph's seven years, the Han
/// *ever-normal granary*, the Roman *horrea* — a law that levies the
/// surplus and holds it against the lean years across a whole realm.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StoreTier {
    /// no craft of keeping: the harvest is eaten in the year it grew
    None = 0,
    /// pottery — sealed jars and lined pits: a season's grain, at a loss
    Jars = 1,
    /// masonry — raised, ventilated granaries: years of grain, kept dry
    Granaries = 2,
    /// law-codes — the administered storehouse: a levy on every fat year,
    /// held against every lean one
    Storehouses = 3,
}

impl StoreTier {
    pub const ALL: [StoreTier; 4] = [StoreTier::None, StoreTier::Jars, StoreTier::Granaries, StoreTier::Storehouses];

    /// The tier a society's arts allow — the highest craft it knows.
    pub fn of(soc: &Society) -> StoreTier {
        if soc.knows(TechId::Law) {
            StoreTier::Storehouses
        } else if soc.knows(TechId::Masonry) {
            StoreTier::Granaries
        } else if soc.knows(TechId::Pottery) {
            StoreTier::Jars
        } else {
            StoreTier::None
        }
    }

    pub fn from_code(c: u8) -> StoreTier {
        match c {
            1 => StoreTier::Jars,
            2 => StoreTier::Granaries,
            3 => StoreTier::Storehouses,
            _ => StoreTier::None,
        }
    }

    #[inline]
    pub fn code(self) -> u8 {
        self as u8
    }

    /// The word the chronicle uses for the store.
    pub fn name(self) -> &'static str {
        match self {
            StoreTier::None => "no store",
            StoreTier::Jars => "jars",
            StoreTier::Granaries => "granaries",
            StoreTier::Storehouses => "storehouses",
        }
    }

    /// The share of a fat year's surplus that is laid by rather than
    /// eaten, sold or sown: the levy the craft (or the law) can take.
    ///
    /// Sized against the harvest law as measured, not guessed: over the
    /// storing towns' town-years the bare yield is symmetric about 1.0,
    /// a fat year gives 7–9 % over the town's eating and the surplus
    /// averages 0.04 person-years per year (civ 12345/777, 150 y). The
    /// store's steady state is `share × 0.04 / spoil`; at the first
    /// draft (0.10/0.20/0.30, spoil 0.25/0.12/0.08) storehouses settled
    /// at 1.8 months and stood at 0.3 months when the verdict came —
    /// days, not seasons. A storehouse worth the name keeps most of a
    /// fat year: jars 0.6 mo, granaries 1.9 mo, storehouses 5 mo at the
    /// steady state, less at the verdict because the lean run drains it
    /// first — the "the store holds seasons" band (1–12 months at the
    /// verdict) reads the result.
    pub fn share(self) -> f64 {
        match self {
            StoreTier::None => 0.0,
            StoreTier::Jars => 0.35,
            StoreTier::Granaries => 0.60,
            StoreTier::Storehouses => 0.85,
        }
    }

    /// The share of the store lost each year to damp, rot and vermin.
    pub fn spoil(self) -> f64 {
        match self {
            StoreTier::None => 1.0,
            StoreTier::Jars => 0.30,
            StoreTier::Granaries => 0.15,
            StoreTier::Storehouses => 0.08,
        }
    }

    /// The most the town can hold, in years of its own grain (× pop):
    /// a season in jars, three in granaries, a year and a half under the
    /// law's roof — reachable only through a long fat run, so the roof
    /// is a thing that happens and not a number.
    pub fn cap_years(self) -> f64 {
        match self {
            StoreTier::None => 0.0,
            StoreTier::Jars => 0.25,
            StoreTier::Granaries => 0.75,
            StoreTier::Storehouses => 1.5,
        }
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
    for &id in &soc.techs {
        match id {
            TechId::Pottery => { m.capacity *= 1.10; m.production *= 1.05; }
            TechId::Loom => { m.production *= 1.10; }
            TechId::HerbLore => { m.health *= 0.80; }
            TechId::Stonecraft => { m.production *= 1.05; m.prospecting *= 1.25; }
            TechId::Bow => { m.war *= 1.15; }
            TechId::Bronze => { m.war *= 1.30; m.production *= 1.10; m.prospecting *= 1.15; }
            TechId::Plough => { m.growth *= 1.12; m.capacity *= 1.15; }
            TechId::Wheel => { m.trade *= 1.25; }
            TechId::Sail => { m.trade *= 1.20; m.colony_range *= 1.35; }
            TechId::Script => { m.research *= 1.35; m.trade *= 1.10; }
            TechId::Masonry => { m.defense *= 0.65; }
            TechId::Iron => { m.war *= 1.40; m.production *= 1.12; m.prospecting *= 1.30; }
            TechId::Coin => { m.trade *= 1.50; m.production *= 1.08; }
            TechId::Law => { m.defense *= 0.90; m.research *= 1.10; m.growth *= 1.05; }
            TechId::Aqueduct => { m.capacity *= 1.30; m.health *= 0.85; }
            TechId::StarCharts => { m.colony_range *= 1.40; m.research *= 1.15; }
            TechId::Medicine => { m.health *= 0.55; }
            TechId::Philosophy => { m.research *= 1.40; }
            TechId::Engineering => { m.trade *= 1.20; m.capacity *= 1.15; m.prospecting *= 1.45; }
            TechId::Steel => { m.war *= 1.50; }
            TechId::MithrilCraft => { m.production *= 1.20; m.trade *= 1.15; }
        }
    }
    m
}

// ---------------------------------------------------------------- monthly

/// Resource kinds within reach of a people's settlements.
fn reachable_kinds(cid: PeopleId, settlements: &[Settlement], deposits: &[Deposit]) -> GoodSet {
    let mut kinds = GoodSet::EMPTY;
    for s in settlements.iter().filter(|s| s.people == cid) {
        let r = territory_radius(s.pop) * 2.2;
        let r2 = r * r;
        for d in deposits {
            if !d.live() {
                continue; // no bronze from a seam nobody has found
            }
            let dx = (d.x - s.x) as f64;
            let dy = (d.y - s.y) as f64;
            if dx * dx + dy * dy <= r2 {
                kinds.insert(d.r);
            }
        }
    }
    kinds
}

/// Lore accumulates, arts are discovered, ages dawn, polities ascend.
pub fn monthly(
    peoples: &mut Peoples,
    deposits: &[Deposit],
    month_abs: i64,
    rng: &mut Pcg64Mcg,
) -> Vec<Event> {
    let Peoples { settlements, peoples: cultures, societies: socs, .. } = peoples;
    let mut events = Vec::new();
    for si in 0..socs.len() {
        let cid = PeopleId(socs[si].people);
        let mine: Vec<&Settlement> =
            settlements.iter().filter(|s| s.people == cid).collect();
        if mine.is_empty() {
            continue;
        }
        let people = cultures[cid.0].people.clone();
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
        kp = (kp * m.research * socs[si].boon).max(0.2);
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
                    && t.requires.iter().all(|&r| socs[si].knows(r))
                    && (t.any_deposit.is_empty()
                        || t.any_deposit.iter().any(|&d| kinds.contains(d)))
                    && (!t.needs_coastal || coastal)
            })
            .collect();
        if !affordable.is_empty() {
            let t = affordable[rng.gen_range(0..affordable.len())];
            socs[si].knowledge -= t.cost * dearness;
            socs[si].techs.push(t.id);
            socs[si].known.insert(t.id);
            events.push(Event {
                m: month_abs,
                s: people.clone(),
                k: EventKind::Tech,
                text: t.flavor.replace("{P}", &people),
                ..Default::default()
            });
            if t.era > socs[si].era {
                socs[si].era = t.era;
                events.push(Event {
                    m: month_abs,
                    s: people.clone(),
                    k: EventKind::Society,
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
            2 => pop >= 2600 && has_town && socs[si].knows(TechId::Script),
            3 => pop >= 9000 && n >= 4 && socs[si].knows(TechId::Coin) && socs[si].knows(TechId::Law),
            _ => false,
        };
        if ready {
            socs[si].polity = next;
            events.push(Event {
                m: month_abs,
                s: people.clone(),
                k: EventKind::Society,
                text: ASCENSIONS[next].replace("{P}", &people),
                ..Default::default()
            });
        }
    }
    events
}
