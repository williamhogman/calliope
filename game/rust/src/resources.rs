//! Resources — the goods ontology, typed (E1).
//!
//! Every good is a `Copy` enum; the old ISA / REQUIRES / ABUNDANCE string
//! relations are const tables and precomputed closure flags on `Good`.
//! Variant order is ALPHABETICAL and load-bearing: `EnumMap` iteration must
//! match the old `BTreeMap<String, _>` order so the determinism hash and
//! every client-visible ordering survive the migration byte-for-byte.

use std::fmt;

use ndarray::Array2;
use serde::Serialize;
use serde_json::{json, Value};
use strum::{EnumCount, IntoEnumIterator};

use crate::constants as gc;
use crate::ndimage;
use crate::noisegen::Perlin3;
use rand::Rng;

/// Every tradeable good in the world. Alphabetical — see module docs.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    strum::Display,
    strum::EnumString,
    strum::EnumIter,
    strum::IntoStaticStr,
    strum::EnumCount,
    enum_map::Enum,
)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Good {
    Bananas,
    Blackberries,
    Blueberries,
    Brick,
    Cattle,
    Clay,
    Cloth,
    Coal,
    Copper,
    Deer,
    Dyes,
    Elk,
    Fish,
    Furs,
    Gems,
    Gold,
    Grain,
    Grapes,
    Hides,
    Horse,
    Iron,
    Jewelry,
    Leather,
    Marble,
    Mithril,
    Pig,
    Pottery,
    Salt,
    Sheep,
    Silver,
    Spices,
    Stone,
    Strawberries,
    Timber,
    Tools,
    Weapons,
    Wine,
    Wool,
}

/// Debug prints the quoted lowercase name — `{:?}` on collections of goods
/// stays byte-identical to the old `Vec<String>` debug output (the
/// determinism hash formats settlement goods this way).
impl fmt::Debug for Good {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\"{}\"", self.name())
    }
}

impl Good {
    pub const COUNT: usize = <Good as EnumCount>::COUNT;

    #[inline]
    pub fn name(self) -> &'static str {
        self.into()
    }

    /// The ontology row (M14.1/ADR-0021): every declarative fact about this
    /// good in the one GOODS table, indexed by variant.
    #[inline]
    pub const fn spec(self) -> &'static GoodSpec {
        &GOODS[self as usize]
    }

    /// ISA-closure flag: the old `isa_chain(g).contains("food")`.
    /// NOTE: grain is deliberately NOT food here — the legacy ontology gave
    /// grain no parent, and the price math must not change (M8 hash gate).
    /// The GOODS row still shelves grain under "food" for the client; the
    /// ontology lint (`ontology_lint`) knows this one exemption.
    #[inline]
    pub const fn is_food(self) -> bool {
        matches!(
            self,
            Good::Bananas
                | Good::Blackberries
                | Good::Blueberries
                | Good::Cattle
                | Good::Deer
                | Good::Elk
                | Good::Fish
                | Good::Horse
                | Good::Pig
                | Good::Sheep
                | Good::Strawberries
        )
    }

    #[inline]
    pub const fn is_metal(self) -> bool {
        matches!(
            self,
            Good::Copper | Good::Gold | Good::Iron | Good::Mithril | Good::Silver
        )
    }

    /// metal ⊂ material, plus timber, stone, salt (M14.2), the animal
    /// secondaries wool and hides (M14.3), and the earth goods clay and
    /// raw gems (M14.5).
    #[inline]
    pub const fn is_material(self) -> bool {
        self.is_metal()
            || matches!(
                self,
                Good::Timber
                    | Good::Stone
                    | Good::Salt
                    | Good::Wool
                    | Good::Hides
                    | Good::Clay
                    | Good::Gems
            )
    }

    #[inline]
    pub const fn is_craft(self) -> bool {
        matches!(
            self,
            Good::Tools
                | Good::Weapons
                | Good::Jewelry
                | Good::Pottery
                | Good::Brick
                | Good::Cloth
                | Good::Leather
                | Good::Wine
        )
    }

    /// M14.3/M14.4/M14.5 — bought with surplus and nothing else: demand is
    /// almost all taste. Furs from the cold, grapes off the warm hills,
    /// spices off the fever coast, dyes from the murex shore — and marble,
    /// the luxury stone, the one Bulk luxury: it crosses the world only
    /// where water carries it.
    #[inline]
    pub const fn is_luxury(self) -> bool {
        matches!(
            self,
            Good::Furs | Good::Grapes | Good::Spices | Good::Dyes | Good::Marble
        )
    }

    #[inline]
    pub const fn is_fuel(self) -> bool {
        matches!(self, Good::Coal)
    }

    /// A mineral seam that mines work and rushes chase. Salt counts
    /// (M14.2): rock-salt seams are prospected, worked and exhausted like
    /// any ore, and they pull mining colonies the same way. Marble
    /// quarries and gem seams join in M14.5 — worked ground with a
    /// reserve, prospected like the rest.
    #[inline]
    pub const fn is_mineral(self) -> bool {
        self.is_metal()
            || matches!(
                self,
                Good::Stone | Good::Coal | Good::Salt | Good::Marble | Good::Gems
            )
    }

    #[inline]
    pub const fn abundance(self) -> Abundance {
        self.spec().abundance
    }

    /// First REQUIRES up the ISA chain.
    #[inline]
    pub const fn requires(self) -> Option<&'static str> {
        self.spec().requires
    }

    /// The ISA chain above the good itself (for client meta).
    #[inline]
    pub const fn isa(self) -> &'static [&'static str] {
        self.spec().isa
    }

    /// Top-of-chain category — the client's shelf label.
    #[inline]
    pub const fn category(self) -> &'static str {
        self.spec().category
    }

    #[inline]
    pub const fn color(self) -> &'static str {
        self.spec().color
    }

    /// M14.7 vocabulary — value-density tier, declared with the good.
    #[inline]
    pub const fn transport(self) -> Transport {
        self.spec().transport
    }

    #[inline]
    pub const fn perishable(self) -> bool {
        self.spec().perishable
    }
}

/// How rare a good runs in the world.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, strum::Display, strum::IntoStaticStr)]
#[strum(serialize_all = "lowercase")]
pub enum Abundance {
    Common,
    Uncommon,
    Rare,
    Legendary,
}

impl Abundance {
    pub const fn quantile(self) -> f64 {
        match self {
            Abundance::Common => 0.945,
            Abundance::Uncommon => 0.975,
            Abundance::Rare => 0.988,
            Abundance::Legendary => 0.9965,
        }
    }
}

// ------------------------------------------------------------- GOODS table
// M14.1/ADR-0021 — the goods ontology as ONE declarative table. Every fact
// about a good (ISA chain, tech gate, rarity, FOUNDIN, color, transport
// class, perishability, placement rule, prospecting odds, reserve) is a
// column here; the accessor methods, `suitability`, `resource_meta` and the
// deposit machinery all derive from these rows. Adding a good is a table
// row, never a new match arm.

/// How a good travels: value-density tier priced into route viability
/// (M14.7 consumes this; declared beside the good it describes).
#[derive(Clone, Copy, PartialEq, Eq, strum::Display, strum::IntoStaticStr)]
#[strum(serialize_all = "lowercase")]
pub enum Transport {
    /// Moves short or by water: grain, timber, stone, coal.
    Bulk,
    /// The everyday middle of the caravan.
    Ordinary,
    /// Crosses the map for its weight in coin.
    Precious,
}

/// Where deposits of a good may lie — placement as data. One interpreter
/// (`suitability`) evaluates these rules; thresholds stay `f32` because the
/// legacy match arms compared against `f32` literals and the masks must
/// stay byte-identical (ADR-0003).
#[derive(Clone, Copy)]
pub enum Place {
    /// Produced, never placed: crafts and farmland grain.
    None,
    /// Any of these biome ids.
    Biomes(&'static [u8]),
    /// Land above this height.
    Above(f32),
    /// Land in the (lo, hi] height band.
    Band(f32, f32),
    /// The biomes, or the (lo, hi] hill band — sheep country.
    BiomesOrBand(&'static [u8], f32, f32),
    /// Coastal water, rivers and lakes.
    Waters,
    /// Land above the first height, or river placer above the second — gold.
    AboveOrPlacer(f32, f32),
    /// Coastal land of the first biome list (salt pans where the sun does
    /// the work), or land of the second list in the (lo, hi] height band
    /// (rock-salt beds in dry basins) — M14.2.
    CoastOrBand(&'static [u8], &'static [u8], f32, f32),
    /// Coastal land of these biomes only — spice coasts and murex
    /// shores (M14.4): the shore itself is the estate.
    Coast(&'static [u8]),
    /// The intersection: these biomes AND the (lo, hi] height band —
    /// vineyard hills (M14.4), where the belt must be tight both ways.
    BiomesAndBand(&'static [u8], f32, f32),
    /// Low land (height ≤ t) touching a river or lake — the alluvial
    /// margins where clay pits lie (M14.5).
    RiverBanks(f32),
}

/// One row of the ontology.
pub struct GoodSpec {
    pub good: Good,
    /// ISA chain above the good (client meta; the closure flags on `Good`
    /// must agree — `ontology_lint` checks, never trusts).
    pub isa: &'static [&'static str],
    /// Client shelf label (top of chain).
    pub category: &'static str,
    /// Tech gate: first REQUIRES up the chain.
    pub requires: Option<&'static str>,
    pub abundance: Abundance,
    /// (FOUNDIN, :right, a) -> b — metals lie in mountains.
    pub foundin: Option<&'static str>,
    pub color: &'static str,
    pub transport: Transport,
    pub perishable: bool,
    pub place: Place,
    /// Chance a placed deposit starts the story already found.
    pub known_p: f64,
    /// Reserve (base, spread) in months-of-working; None = the land renews.
    pub reserve: Option<(f64, f64)>,
}

/// Variant order (= alphabetical), one row per good. `Good::spec()` indexes
/// this directly; the const block below makes a misordered row a compile
/// error, not a wrong world.
pub const GOODS: [GoodSpec; Good::COUNT] = [
    GoodSpec { good: Good::Bananas, isa: &["fruit", "food"], category: "food", requires: None, abundance: Abundance::Common, foundin: None, color: "#f5d442", transport: Transport::Ordinary, perishable: true, place: Place::Biomes(&[gc::TROPICAL_RAIN_FOREST]), known_p: 1.0, reserve: None },
    GoodSpec { good: Good::Blackberries, isa: &["berry", "fruit", "food"], category: "food", requires: Some("gathering"), abundance: Abundance::Common, foundin: None, color: "#6b3fa0", transport: Transport::Ordinary, perishable: true, place: Place::Biomes(&[gc::WOODLAND, gc::SEASONAL_RAIN_FOREST]), known_p: 1.0, reserve: None },
    GoodSpec { good: Good::Blueberries, isa: &["berry", "fruit", "food"], category: "food", requires: Some("gathering"), abundance: Abundance::Common, foundin: None, color: "#5b6ee1", transport: Transport::Ordinary, perishable: true, place: Place::Biomes(&[gc::BOREAL_FOREST, gc::TUNDRA, gc::WET_TUNDRA]), known_p: 1.0, reserve: None },
    GoodSpec { good: Good::Brick, isa: &["craft"], category: "craft", requires: None, abundance: Abundance::Uncommon, foundin: None, color: "#b3543e", transport: Transport::Bulk, perishable: false, place: Place::None, known_p: 1.0, reserve: None },
    GoodSpec { good: Good::Cattle, isa: &["livestock", "animal", "food"], category: "food", requires: None, abundance: Abundance::Uncommon, foundin: None, color: "#c98d5a", transport: Transport::Ordinary, perishable: false, place: Place::Biomes(&[gc::GRASSLAND]), known_p: 1.0, reserve: None },
    GoodSpec { good: Good::Clay, isa: &["material"], category: "material", requires: None, abundance: Abundance::Common, foundin: Some("riverbank"), color: "#ad6a4e", transport: Transport::Bulk, perishable: false, place: Place::RiverBanks(0.35), known_p: 1.0, reserve: None },
    GoodSpec { good: Good::Cloth, isa: &["craft"], category: "craft", requires: None, abundance: Abundance::Uncommon, foundin: None, color: "#d9cfa8", transport: Transport::Ordinary, perishable: false, place: Place::None, known_p: 1.0, reserve: None },
    GoodSpec { good: Good::Coal, isa: &["fuel"], category: "fuel", requires: None, abundance: Abundance::Common, foundin: None, color: "#3a3f46", transport: Transport::Bulk, perishable: false, place: Place::Band(0.3, 0.6), known_p: 0.25, reserve: Some((480.0, 960.0)) },
    GoodSpec { good: Good::Copper, isa: &["metal", "material"], category: "material", requires: Some("metal-working"), abundance: Abundance::Common, foundin: Some("mountain"), color: "#d97742", transport: Transport::Ordinary, perishable: false, place: Place::Above(0.45), known_p: 0.25, reserve: Some((520.0, 980.0)) },
    GoodSpec { good: Good::Deer, isa: &["game", "animal", "food"], category: "food", requires: None, abundance: Abundance::Common, foundin: None, color: "#b08968", transport: Transport::Ordinary, perishable: false, place: Place::Biomes(&[gc::WOODLAND, gc::SEASONAL_RAIN_FOREST, gc::TEMPERATE_RAIN_FOREST]), known_p: 1.0, reserve: None },
    GoodSpec { good: Good::Dyes, isa: &["luxury"], category: "luxury", requires: None, abundance: Abundance::Rare, foundin: Some("shore"), color: "#a03a75", transport: Transport::Precious, perishable: false, place: Place::Coast(&[gc::GRASSLAND, gc::WOODLAND, gc::SAVANNA]), known_p: 1.0, reserve: None },
    GoodSpec { good: Good::Elk, isa: &["game", "animal", "food"], category: "food", requires: None, abundance: Abundance::Common, foundin: None, color: "#8a6f52", transport: Transport::Ordinary, perishable: false, place: Place::Biomes(&[gc::BOREAL_FOREST, gc::TUNDRA, gc::WET_TUNDRA]), known_p: 1.0, reserve: None },
    GoodSpec { good: Good::Fish, isa: &["food"], category: "food", requires: Some("fishing"), abundance: Abundance::Common, foundin: None, color: "#7fd4e8", transport: Transport::Ordinary, perishable: true, place: Place::Waters, known_p: 1.0, reserve: None },
    GoodSpec { good: Good::Furs, isa: &["luxury"], category: "luxury", requires: None, abundance: Abundance::Rare, foundin: Some("cold"), color: "#7a5c44", transport: Transport::Precious, perishable: false, place: Place::Biomes(&[gc::BOREAL_FOREST, gc::TUNDRA, gc::WET_TUNDRA]), known_p: 1.0, reserve: None },
    GoodSpec { good: Good::Gems, isa: &["material"], category: "material", requires: None, abundance: Abundance::Rare, foundin: Some("deep rock"), color: "#59c9a5", transport: Transport::Precious, perishable: false, place: Place::Above(0.6), known_p: 0.04, reserve: Some((260.0, 520.0)) },
    GoodSpec { good: Good::Gold, isa: &["metal", "material"], category: "material", requires: Some("metal-working"), abundance: Abundance::Rare, foundin: Some("mountain"), color: "#f2c14e", transport: Transport::Precious, perishable: false, place: Place::AboveOrPlacer(0.6, 0.35), known_p: 0.03, reserve: Some((260.0, 520.0)) },
    GoodSpec { good: Good::Grain, isa: &["food"], category: "food", requires: Some("farming"), abundance: Abundance::Common, foundin: None, color: "#e3c96b", transport: Transport::Bulk, perishable: false, place: Place::None, known_p: 1.0, reserve: None },
    GoodSpec { good: Good::Grapes, isa: &["vine", "luxury"], category: "luxury", requires: Some("farming"), abundance: Abundance::Uncommon, foundin: Some("warm hills"), color: "#8a4fbe", transport: Transport::Ordinary, perishable: false, place: Place::BiomesAndBand(&[gc::WOODLAND, gc::SEASONAL_RAIN_FOREST], 0.12, 0.5), known_p: 1.0, reserve: None },
    GoodSpec { good: Good::Hides, isa: &["material"], category: "material", requires: None, abundance: Abundance::Common, foundin: None, color: "#a5825f", transport: Transport::Bulk, perishable: false, place: Place::None, known_p: 1.0, reserve: None },
    GoodSpec { good: Good::Horse, isa: &["livestock", "animal", "food"], category: "food", requires: None, abundance: Abundance::Uncommon, foundin: None, color: "#a9754f", transport: Transport::Ordinary, perishable: false, place: Place::Biomes(&[gc::GRASSLAND, gc::SAVANNA]), known_p: 1.0, reserve: None },
    GoodSpec { good: Good::Iron, isa: &["metal", "material"], category: "material", requires: Some("iron-working"), abundance: Abundance::Common, foundin: Some("mountain"), color: "#8f4f38", transport: Transport::Ordinary, perishable: false, place: Place::Above(0.45), known_p: 0.20, reserve: Some((520.0, 980.0)) },
    GoodSpec { good: Good::Jewelry, isa: &["craft"], category: "craft", requires: None, abundance: Abundance::Rare, foundin: None, color: "#d79ae0", transport: Transport::Precious, perishable: false, place: Place::None, known_p: 1.0, reserve: None },
    GoodSpec { good: Good::Leather, isa: &["craft"], category: "craft", requires: None, abundance: Abundance::Uncommon, foundin: None, color: "#8a5a33", transport: Transport::Ordinary, perishable: false, place: Place::None, known_p: 1.0, reserve: None },
    GoodSpec { good: Good::Marble, isa: &["luxury"], category: "luxury", requires: Some("masonry"), abundance: Abundance::Rare, foundin: Some("high crags"), color: "#e8e6df", transport: Transport::Bulk, perishable: false, place: Place::Above(0.55), known_p: 0.45, reserve: Some((900.0, 1400.0)) },
    GoodSpec { good: Good::Mithril, isa: &["metal", "material"], category: "material", requires: Some("mithril-smithing"), abundance: Abundance::Legendary, foundin: Some("mountain"), color: "#8ef0e2", transport: Transport::Precious, perishable: false, place: Place::Above(0.8), known_p: 0.0, reserve: Some((1400.0, 1200.0)) },
    GoodSpec { good: Good::Pig, isa: &["livestock", "animal", "food"], category: "food", requires: None, abundance: Abundance::Common, foundin: None, color: "#e0a3a3", transport: Transport::Ordinary, perishable: false, place: Place::Biomes(&[gc::WOODLAND, gc::SEASONAL_RAIN_FOREST]), known_p: 1.0, reserve: None },
    GoodSpec { good: Good::Pottery, isa: &["craft"], category: "craft", requires: None, abundance: Abundance::Common, foundin: None, color: "#c47a4a", transport: Transport::Ordinary, perishable: false, place: Place::None, known_p: 1.0, reserve: None },
    GoodSpec { good: Good::Salt, isa: &["material"], category: "material", requires: None, abundance: Abundance::Uncommon, foundin: Some("basin"), color: "#eef2f5", transport: Transport::Ordinary, perishable: false, place: Place::CoastOrBand(&[gc::DESERT, gc::SAVANNA, gc::GRASSLAND], &[gc::DESERT, gc::SAVANNA, gc::GRASSLAND], 0.15, 0.5), known_p: 0.30, reserve: Some((620.0, 900.0)) },
    GoodSpec { good: Good::Sheep, isa: &["livestock", "animal", "food"], category: "food", requires: None, abundance: Abundance::Common, foundin: None, color: "#e8e2d0", transport: Transport::Ordinary, perishable: false, place: Place::BiomesOrBand(&[gc::GRASSLAND, gc::TUNDRA], 0.3, 0.6), known_p: 1.0, reserve: None },
    GoodSpec { good: Good::Silver, isa: &["metal", "material"], category: "material", requires: Some("metal-working"), abundance: Abundance::Uncommon, foundin: Some("mountain"), color: "#c8d0da", transport: Transport::Precious, perishable: false, place: Place::Above(0.55), known_p: 0.06, reserve: Some((320.0, 640.0)) },
    GoodSpec { good: Good::Spices, isa: &["luxury"], category: "luxury", requires: None, abundance: Abundance::Rare, foundin: Some("tropic coast"), color: "#c9772f", transport: Transport::Precious, perishable: false, place: Place::Coast(&[gc::TROPICAL_RAIN_FOREST, gc::SEASONAL_RAIN_FOREST]), known_p: 1.0, reserve: None },
    GoodSpec { good: Good::Stone, isa: &["material"], category: "material", requires: None, abundance: Abundance::Common, foundin: None, color: "#9aa2ad", transport: Transport::Bulk, perishable: false, place: Place::Above(0.5), known_p: 0.60, reserve: None },
    GoodSpec { good: Good::Strawberries, isa: &["berry", "fruit", "food"], category: "food", requires: Some("gathering"), abundance: Abundance::Common, foundin: None, color: "#e4485b", transport: Transport::Ordinary, perishable: true, place: Place::Biomes(&[gc::GRASSLAND, gc::WOODLAND]), known_p: 1.0, reserve: None },
    GoodSpec { good: Good::Timber, isa: &["material"], category: "material", requires: None, abundance: Abundance::Common, foundin: None, color: "#4f8f3a", transport: Transport::Bulk, perishable: false, place: Place::Biomes(&[gc::WOODLAND, gc::SEASONAL_RAIN_FOREST, gc::TEMPERATE_RAIN_FOREST, gc::BOREAL_FOREST, gc::TROPICAL_RAIN_FOREST]), known_p: 1.0, reserve: None },
    GoodSpec { good: Good::Tools, isa: &["craft"], category: "craft", requires: None, abundance: Abundance::Uncommon, foundin: None, color: "#8fa3b0", transport: Transport::Ordinary, perishable: false, place: Place::None, known_p: 1.0, reserve: None },
    GoodSpec { good: Good::Weapons, isa: &["craft"], category: "craft", requires: None, abundance: Abundance::Rare, foundin: None, color: "#b8524a", transport: Transport::Ordinary, perishable: false, place: Place::None, known_p: 1.0, reserve: None },
    GoodSpec { good: Good::Wine, isa: &["craft"], category: "craft", requires: None, abundance: Abundance::Rare, foundin: None, color: "#93264f", transport: Transport::Ordinary, perishable: false, place: Place::None, known_p: 1.0, reserve: None },
    GoodSpec { good: Good::Wool, isa: &["material"], category: "material", requires: None, abundance: Abundance::Uncommon, foundin: None, color: "#f2ead8", transport: Transport::Ordinary, perishable: false, place: Place::None, known_p: 1.0, reserve: None },
];

// A misordered table row is a compile error, not a wrong world.
const _: () = {
    let mut i = 0;
    while i < GOODS.len() {
        assert!(GOODS[i].good as usize == i);
        i += 1;
    }
};

/// M14.1 — the one seam where the const closure flags could drift from the
/// GOODS table: checked, never trusted. Returns human-readable violations;
/// the resources diagnostic prints any as a [FAIL].
pub fn ontology_lint() -> Vec<String> {
    let mut bad = Vec::new();
    for g in Good::iter() {
        let s = g.spec();
        let has = |t: &str| s.isa.contains(&t);
        // grain: shelved as food for the client, excluded from the food
        // closure — the M8 hash-gate exemption documented on `is_food`.
        if g != Good::Grain && g.is_food() != has("food") {
            bad.push(format!("{g}: is_food={} but isa says {}", g.is_food(), has("food")));
        }
        if g.is_metal() != has("metal") {
            bad.push(format!("{g}: is_metal={} but isa says {}", g.is_metal(), has("metal")));
        }
        if g.is_material() != has("material") {
            bad.push(format!("{g}: is_material={} but isa says {}", g.is_material(), has("material")));
        }
        if g.is_craft() != has("craft") {
            bad.push(format!("{g}: is_craft={} but isa says {}", g.is_craft(), has("craft")));
        }
        if g.is_luxury() != has("luxury") {
            bad.push(format!("{g}: is_luxury={} but isa says {}", g.is_luxury(), has("luxury")));
        }
        if g.is_fuel() != has("fuel") {
            bad.push(format!("{g}: is_fuel={} but isa says {}", g.is_fuel(), has("fuel")));
        }
        // metals lie in mountains, and only metals
        if (s.foundin == Some("mountain")) != g.is_metal() {
            bad.push(format!("{g}: foundin={:?} disagrees with is_metal={}", s.foundin, g.is_metal()));
        }
    }
    bad
}

// ---------------------------------------------------------------- GoodSet

/// A set of goods as one u32 — the ISA-closure bitmask idea (E1.3) applied
/// to every "collection of goods" in the engine. Iteration order is variant
/// (= alphabetical) order, matching the old `BTreeSet<String>` everywhere.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct GoodSet(u64);

impl GoodSet {
    pub const EMPTY: GoodSet = GoodSet(0);

    #[inline]
    pub fn insert(&mut self, g: Good) {
        self.0 |= 1 << g as u64;
    }

    #[inline]
    pub fn contains(self, g: Good) -> bool {
        self.0 & (1 << g as u64) != 0
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    pub fn union(self, other: GoodSet) -> GoodSet {
        GoodSet(self.0 | other.0)
    }

    /// Alphabetical iteration (variant order).
    pub fn iter(self) -> impl Iterator<Item = Good> {
        Good::iter().filter(move |&g| self.contains(g))
    }
}

impl FromIterator<Good> for GoodSet {
    fn from_iter<T: IntoIterator<Item = Good>>(iter: T) -> Self {
        let mut s = GoodSet::EMPTY;
        for g in iter {
            s.insert(g);
        }
        s
    }
}

impl Extend<Good> for GoodSet {
    fn extend<T: IntoIterator<Item = Good>>(&mut self, iter: T) {
        for g in iter {
            self.insert(g);
        }
    }
}

// ---------------------------------------------------------------- deposits

/// Placement order is the LEGACY order — noise planes and rng streams are
/// indexed by position in this array, so reordering re-rolls every world.
/// New goods APPEND (M14.2 salt): earlier planes and streams stay untouched.
pub const ALL_PLACEABLE: [Good; 27] = [
    Good::Bananas,
    Good::Blueberries,
    Good::Strawberries,
    Good::Blackberries,
    Good::Cattle,
    Good::Sheep,
    Good::Horse,
    Good::Pig,
    Good::Deer,
    Good::Elk,
    Good::Fish,
    Good::Timber,
    Good::Stone,
    Good::Coal,
    Good::Copper,
    Good::Iron,
    Good::Silver,
    Good::Gold,
    Good::Mithril,
    Good::Salt,
    Good::Furs,
    Good::Grapes,
    Good::Spices,
    Good::Dyes,
    Good::Clay,
    Good::Marble,
    Good::Gems,
];

/// (FOUNDIN, :right, a) -> b — a GOODS column since M14.1.
pub fn foundin(g: Good) -> Option<&'static str> {
    g.spec().foundin
}

/// M14.2 PRESERVES — the perishables a salting yard can cure for the road:
/// flesh and fish, not fruit (fruit dries without salt; its haul cost is
/// M14.7's business). Today that is fish; meats join when they perish.
pub fn salt_cured(g: Good) -> bool {
    g.perishable() && !g.isa().contains(&"fruit")
}

#[derive(Serialize, Clone)]
pub struct Deposit {
    pub r: Good,
    pub x: i64,
    pub y: i64,
    pub rich: f64,
    /// surface goods start known; buried seams wait for prospectors
    pub known: bool,
    /// months of working left before exhaustion; -1 = renews forever
    pub left: f64,
    /// M14.8 — standing stock as a fraction of carrying capacity. Wild
    /// grounds (timber, fish, game) breathe: logistic regrowth against
    /// harvest pressure. Minerals stay pinned at 1.0 — a seam does not
    /// grow back.
    pub stock: f64,
    /// M14.8 — hysteresis latch: 0 healthy · 1 thinned (told once) ·
    /// 2 collapsed (the good withdraws until the stock stands again).
    pub phase: u8,
}

impl Deposit {
    pub fn new(r: Good, x: i64, y: i64, rich: f64, known: bool, left: f64) -> Deposit {
        Deposit { r, x, y, rich, known, left, stock: 1.0, phase: 0 }
    }

    /// The one qualification every consumer asks: does this ground yield?
    /// Unfound seams, spent pits and collapsed wild stocks all say no —
    /// one method, so the answer cannot drift between call sites (M14.8).
    pub fn live(&self) -> bool {
        self.known && self.left != 0.0 && self.phase != 2
    }
}

/// M14.8 — monthly logistic regrowth rate for the wild goods whose stocks
/// carry memory; None for everything the land does not thin. Max sustainable
/// harvest is r/4 (at half stock), and one town's pressure is
/// 0.0025·crews with crews in [1,2] — so a lone hamlet logs sustainably,
/// a crowded coast strips its woods. Forests fail before fisheries.
pub fn regrow_rate(g: Good) -> Option<f64> {
    match g {
        Good::Timber => Some(0.020),
        Good::Furs | Good::Deer | Good::Elk => Some(0.030),
        Good::Fish => Some(0.040),
        _ => None,
    }
}

/// Chance a deposit starts the story already found. Everything that walks,
/// grows or swims is plain to see; buried seams mostly are not — the age of
/// prospectors has to actually happen on stage. A GOODS column since M14.1.
fn initial_known_p(g: Good) -> f64 {
    g.spec().known_p
}

/// Reserve in months-of-working; -1 for goods the land renews.
/// (base, spread) live in the GOODS table; the formula lives here.
fn reserve_months(g: Good, rich: f64, roll: f64) -> f64 {
    match g.spec().reserve {
        Some((base, spread)) => ((base + spread * rich) * (0.85 + 0.30 * roll)).round(),
        None => -1.0,
    }
}

/// Coastal water cells (sea adjacent to land).
fn coastal_water(height: &Array2<f32>) -> Array2<bool> {
    let (h, w) = height.dim();
    let sea = |y: usize, x: usize| height[[y, x]] < 0.0;
    Array2::from_shape_fn((h, w), |(y, x)| {
        if !sea(y, x) {
            return false;
        }
        (y > 0 && !sea(y - 1, x))
            || (y + 1 < h && !sea(y + 1, x))
            || (x > 0 && !sea(y, x - 1))
            || (x + 1 < w && !sea(y, x + 1))
    })
}

/// Coastal land cells (land adjacent to sea) — the mirror of
/// `coastal_water`, where the pans lie (M14.2).
fn coastal_land(height: &Array2<f32>) -> Array2<bool> {
    let (h, w) = height.dim();
    let sea = |y: usize, x: usize| height[[y, x]] < 0.0;
    Array2::from_shape_fn((h, w), |(y, x)| {
        if sea(y, x) {
            return false;
        }
        (y > 0 && sea(y - 1, x))
            || (y + 1 < h && sea(y + 1, x))
            || (x > 0 && sea(y, x - 1))
            || (x + 1 < w && sea(y, x + 1))
    })
}

fn biome_mask(biomes: &Array2<u8>, ids: &[u8]) -> Array2<bool> {
    biomes.mapv(|b| ids.contains(&b))
}

/// One interpreter over the GOODS placement column (M14.1) — the old
/// nineteen match arms, now seven rule shapes. Height thresholds compare in
/// `f32` exactly as the legacy arms did, so every mask is byte-identical.
fn suitability(
    g: Good,
    biomes: &Array2<u8>,
    height: &Array2<f32>,
    rivers: &Array2<bool>,
    lakes: &Array2<bool>,
) -> Array2<bool> {
    let land = |y: usize, x: usize| height[[y, x]] >= 0.0;
    let h = height;
    let dim = height.dim();
    match g.spec().place {
        Place::None => Array2::from_elem(dim, false),
        Place::Biomes(ids) => biome_mask(biomes, ids),
        Place::Above(t) => Array2::from_shape_fn(dim, |(y, x)| land(y, x) && h[[y, x]] > t),
        Place::Band(lo, hi) => Array2::from_shape_fn(dim, |(y, x)| {
            land(y, x) && h[[y, x]] > lo && h[[y, x]] <= hi
        }),
        Place::BiomesOrBand(ids, lo, hi) => Array2::from_shape_fn(dim, |(y, x)| {
            let b = ids.contains(&biomes[[y, x]]);
            let hills = land(y, x) && h[[y, x]] > lo && h[[y, x]] <= hi;
            b || hills
        }),
        Place::Waters => {
            let coast = coastal_water(height);
            Array2::from_shape_fn(dim, |(y, x)| {
                coast[[y, x]] || rivers[[y, x]] || lakes[[y, x]]
            })
        }
        Place::AboveOrPlacer(a, b) => Array2::from_shape_fn(dim, |(y, x)| {
            (land(y, x) && h[[y, x]] > a) || (rivers[[y, x]] && h[[y, x]] > b)
        }),
        Place::CoastOrBand(pans, beds, lo, hi) => {
            let coast = coastal_land(height);
            Array2::from_shape_fn(dim, |(y, x)| {
                let b = biomes[[y, x]];
                let pan = coast[[y, x]] && pans.contains(&b);
                let bed =
                    land(y, x) && beds.contains(&b) && h[[y, x]] > lo && h[[y, x]] <= hi;
                pan || bed
            })
        }
        Place::Coast(ids) => {
            let coast = coastal_land(height);
            Array2::from_shape_fn(dim, |(y, x)| {
                coast[[y, x]] && ids.contains(&biomes[[y, x]])
            })
        }
        Place::BiomesAndBand(ids, lo, hi) => Array2::from_shape_fn(dim, |(y, x)| {
            ids.contains(&biomes[[y, x]]) && land(y, x) && h[[y, x]] > lo && h[[y, x]] <= hi
        }),
        Place::RiverBanks(t) => {
            let (rows, cols) = dim;
            let wet = |y: usize, x: usize| rivers[[y, x]] || lakes[[y, x]];
            Array2::from_shape_fn(dim, |(y, x)| {
                if !land(y, x) || h[[y, x]] > t {
                    return false;
                }
                let y1 = (y + 1).min(rows - 1);
                let x1 = (x + 1).min(cols - 1);
                for yy in y.saturating_sub(1)..=y1 {
                    for xx in x.saturating_sub(1)..=x1 {
                        if wet(yy, xx) {
                            return true;
                        }
                    }
                }
                false
            })
        }
    }
}

/// ADR-0013 — the floor of fate, declared once (ADR-0015): every mineral
/// the late game hangs on is guaranteed this many seams, placed into the
/// highest fitting ground when the noise race starves the world of them.
/// `place_resources` enforces it; the M15 assay proves it on arbitrary
/// seeds. (good, minimum seams, preferred height floor)
pub const STRATEGIC_MINIMA: [(Good, usize, f64); 9] = [
    (Good::Stone, 4, 0.45),
    (Good::Coal, 4, 0.28),
    (Good::Copper, 4, 0.40),
    (Good::Iron, 4, 0.40),
    (Good::Silver, 2, 0.48),
    (Good::Gold, 2, 0.42),
    (Good::Mithril, 1, 0.55),
    (Good::Marble, 2, 0.50),
    (Good::Gems, 2, 0.55),
];

/// M19 — where ore belongs. Each tracked mineral names the rock provinces
/// (M18) its seams favor: gold in the shields and the volcanic intrusions,
/// coal in the sedimentary basins, marble in the metamorphic fold belts,
/// iron in the banded shields and the stacked belts, copper and silver in
/// the arcs. Placement narrows a good's suitability mask to its home
/// provinces whenever the homes offer enough fitting ground; a world whose
/// shields all lie under ice keeps the full mask — the floor of fate
/// (ADR-0013) is untouched and runs after, province-blind, exactly as
/// before. Mithril and stone sit where they will.
pub const ORE_HOMES: &[(Good, &[u8])] = &[
    (Good::Gold, &[crate::rock::SHIELD, crate::rock::VOLCANIC]),
    (Good::Silver, &[crate::rock::VOLCANIC, crate::rock::FOLD_BELT]),
    (Good::Copper, &[crate::rock::VOLCANIC, crate::rock::FOLD_BELT]),
    (Good::Iron, &[crate::rock::SHIELD, crate::rock::FOLD_BELT]),
    (Good::Coal, &[crate::rock::BASIN]),
    (Good::Gems, &[crate::rock::SHIELD, crate::rock::VOLCANIC]),
    (Good::Marble, &[crate::rock::FOLD_BELT]),
];

/// The home provinces of a good, if geology has an opinion (M19).
pub fn homes_of(g: Good) -> Option<&'static [u8]> {
    ORE_HOMES.iter().find(|(good, _)| *good == g).map(|&(_, h)| h)
}

/// M19 honesty floor: a narrowed mask must keep at least this many cells
/// or the good falls back to its full mask — geology guides, it never
/// starves a world of an essential seam.
const MIN_HOME_CELLS: usize = 40;

/// E10.1 — the in-mask 5×5 race, decided pointwise: true iff no in-mask
/// cell in the reflected 5×5 window carries a strictly larger value.
/// Exactly the blanked-`maximum_filter` test at a masked cell — the
/// window includes the cell itself, so "equals the window max" and "no
/// strictly larger in-mask rival" are the same predicate — without ever
/// materializing the blanked grid.
fn wins_masked_race(fj: &Array2<f64>, mask: &Array2<bool>, y: usize, x: usize) -> bool {
    let (h, w) = fj.dim();
    let v = fj[[y, x]];
    for dy in -2isize..=2 {
        let yy = ndimage::reflect(y as isize + dy, h as isize);
        for dx in -2isize..=2 {
            let xx = ndimage::reflect(x as isize + dx, w as isize);
            if mask[[yy, xx]] && fj[[yy, xx]] > v {
                return false;
            }
        }
    }
    true
}

/// E10.1 — the whole-map 5×5 race, decided pointwise: true iff no cell
/// in the reflected window carries a strictly larger value. Exactly the
/// `fj == maximum_filter(fj, 5)` test, evaluated only where a candidate
/// actually stands.
fn wins_open_race(fj: &Array2<f64>, y: usize, x: usize) -> bool {
    let (h, w) = fj.dim();
    let v = fj[[y, x]];
    for dy in -2isize..=2 {
        let yy = ndimage::reflect(y as isize + dy, h as isize);
        for dx in -2isize..=2 {
            let xx = ndimage::reflect(x as isize + dx, w as isize);
            if fj[[yy, xx]] > v {
                return false;
            }
        }
    }
    true
}

/// M19 lane — for each homed good: (good, seams in a home province,
/// seams total). Deposit coordinates index the rock grid directly (the
/// widen pass shifts both together).
pub fn province_consistency(
    deposits: &[Deposit],
    rock: &Array2<u8>,
) -> Vec<(Good, usize, usize)> {
    ORE_HOMES
        .iter()
        .map(|&(good, homes)| {
            let mut in_home = 0usize;
            let mut total = 0usize;
            for d in deposits.iter().filter(|d| d.r == good) {
                total += 1;
                if homes.contains(&rock[[d.y as usize, d.x as usize]]) {
                    in_home += 1;
                }
            }
            (good, in_home, total)
        })
        .collect()
}

/// M15.6 — the flow meter: cumulative reserve drawn and net stock movement
/// per deposit, metered at the exact mutation sites in `prospecting.rs`.
/// The conservation ledger in `diagnose economy` balances these meters
/// against the state deltas, so any unmetered mutation of `left` or
/// `stock` — now or in a future edit — breaks the balance loudly. Pure
/// bookkeeping: never packed, hashed or serialized.
#[derive(Clone, Default)]
pub struct Flows {
    /// Reserve drawn from each deposit, in units of `left` (crew-months).
    pub extracted: Vec<f64>,
    /// Net movement of each renewable ground's `stock` (regrowth − harvest).
    pub dstock: Vec<f64>,
    /// M57 — the month each seam came to light, −1 while still hidden.
    /// Pure bookkeeping: never packed, never hashed, read by the outcrop
    /// gate, which asks *when* ground gave up its ore, not merely whether.
    pub found_m: Vec<i32>,
}

impl Flows {
    pub fn for_deposits(n: usize) -> Flows {
        Flows { extracted: vec![0.0; n], dstock: vec![0.0; n], found_m: vec![-1; n] }
    }
}

/// Deposits thinned to local maxima — port of place_resources.
pub fn place_resources(
    biomes: &Array2<u8>,
    height: &Array2<f32>,
    rivers: &Array2<bool>,
    lakes: &Array2<bool>,
    rock: &Array2<u8>,
    seed: i64,
) -> Vec<Deposit> {
    let size = height.dim().0;
    let half = size / 2;
    let noise = Perlin3::new(seed + 5000);
    let mut deposits = Vec::new();

    for (i, &good) in ALL_PLACEABLE.iter().enumerate() {
        let mask = suitability(good, biomes, height, rivers, lakes);
        // M19 — deposits re-seated: a homed mineral's mask narrows to its
        // rock provinces when the homes offer enough fitting ground; the
        // fallback keeps a starved world honest (MIN_HOME_CELLS). A
        // narrowed good also runs its maxima race within the mask (below),
        // for the same reason the shore goods do (M14.4): a province cell
        // rarely tops its whole-map 5×5 neighborhood.
        let mut homed = false;
        let mask = if let Some(homes) = homes_of(good) {
            // One fused pass: narrow and count together (E10.1).
            let mut narrowed = mask.clone();
            let mut kept = 0usize;
            for (m, &r) in narrowed.iter_mut().zip(rock.iter()) {
                if *m && homes.contains(&r) {
                    kept += 1;
                } else {
                    *m = false;
                }
            }
            if kept >= MIN_HOME_CELLS {
                homed = true;
                narrowed
            } else {
                mask
            }
        } else {
            mask
        };
        if !mask.iter().any(|&m| m) {
            continue;
        }

        // noise evaluated at half resolution, upsampled
        let mut small = Array2::<f64>::zeros((half, half));
        let z = 1.7 + i as f64 * 0.61;
        for y in 0..half {
            for x in 0..half {
                small[[y, x]] = noise.fbm(
                    x as f64 / half as f64 * 11.0,
                    y as f64 / half as f64 * 11.0,
                    z,
                    3,
                );
            }
        }
        let field = Array2::from_shape_fn((size, size), |(y, x)| {
            small[[(y / 2).min(half - 1), (x / 2).min(half - 1)]]
        });

        let vals: Vec<f64> = field
            .iter()
            .zip(mask.iter())
            .filter(|(_, &m)| m)
            .map(|(&v, _)| v)
            .collect();
        let q = good.abundance().quantile();
        let thresh = crate::util::quantile(&vals, q);

        // deterministic jitter breaks ties on the 2x2 upsampling plateaus
        let mut rng = crate::util::rng(seed * 31 + i as i64);
        let mut fj = Array2::<f64>::zeros((size, size));
        for y in 0..size {
            for x in 0..size {
                fj[[y, x]] = field[[y, x]] + rng.gen::<f64>() * 1e-6;
            }
        }
        // M14.4 — shore goods live on one-cell strips: a coastal cell
        // almost never tops its full 5×5 neighborhood (inland and open
        // sea outbid it), so for `Place::Coast` the maxima race runs
        // within the mask. M19 widens the same rule to province-homed
        // minerals: a narrowed mask races within itself, or the shields
        // would never beat the basins that surround them. Every other
        // good keeps the whole-map race, byte-identical to before.
        //
        // E10.1 — both races are decided pointwise at candidate cells
        // (the same reflected 5×5 window `maximum_filter` reads; in-mask
        // rivals only for the masked race) instead of materializing a
        // full-grid filter per good: the budget pays O(candidates ×
        // window), not O(grid × goods).
        let masked_race = homed || matches!(good.spec().place, Place::Coast(_));

        // M14.2 — salt pans: coastal works are plain to see and the sea
        // renews them; buried rock-salt seams roll the dice like any ore.
        let pans = if good == Good::Salt {
            Some(coastal_land(height))
        } else {
            None
        };

        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for &v in field.iter() {
            lo = lo.min(v);
            hi = hi.max(v);
        }
        for y in 0..size {
            for x in 0..size {
                let wins = mask[[y, x]]
                    && field[[y, x]] >= thresh
                    && if masked_race {
                        wins_masked_race(&fj, &mask, y, x)
                    } else {
                        wins_open_race(&fj, y, x)
                    };
                if wins {
                    let rich = (field[[y, x]] - lo) / (hi - lo).max(1e-9);
                    let richv = crate::util::round2(0.35 + 0.65 * rich);
                    let pan = pans.as_ref().map_or(false, |p| p[[y, x]]);
                    deposits.push(Deposit::new(
                        good,
                        x as i64,
                        y as i64,
                        richv,
                        pan || rng.gen::<f64>() < initial_known_p(good),
                        if pan { -1.0 } else { reserve_months(good, richv, rng.gen::<f64>()) },
                    ));
                }
            }
        }

        // M14.4 — the shore keeps what it grows: if a Coast good's mask
        // exists but the threshold left it empty, the single best masked
        // cell gets the ground. A world with a spice coast always has
        // spices somewhere; a world without the biome stays honest.
        if matches!(good.spec().place, Place::Coast(_))
            && !deposits.iter().any(|d| d.r == good)
        {
            let mut best: Option<(f64, usize, usize)> = None;
            for y in 0..size {
                for x in 0..size {
                    if mask[[y, x]] && best.map_or(true, |(bv, _, _)| fj[[y, x]] > bv) {
                        best = Some((fj[[y, x]], y, x));
                    }
                }
            }
            if let Some((_, y, x)) = best {
                let rich = (field[[y, x]] - lo) / (hi - lo).max(1e-9);
                let richv = crate::util::round2(0.35 + 0.65 * rich);
                deposits.push(Deposit::new(
                    good,
                    x as i64,
                    y as i64,
                    richv,
                    rng.gen::<f64>() < initial_known_p(good),
                    reserve_months(good, richv, rng.gen::<f64>()),
                ));
            }
        }

        // M19 — the province keeps what it holds: a homed mineral whose
        // race under-yields its ADR-0013 minimum tops up from the best
        // remaining in-mask cells *now*, so the province-blind floor of
        // fate below almost never has to fire for it. Same rescue shape
        // as M14.4; the floor pass itself stays byte-identical.
        if homed {
            if let Some(&(_, min_n, _)) =
                STRATEGIC_MINIMA.iter().find(|&&(g, _, _)| g == good)
            {
                let mut have = deposits.iter().filter(|d| d.r == good).count();
                if have < min_n {
                    let mut cands: Vec<(f64, usize, usize)> = Vec::new();
                    for y in 0..size {
                        for x in 0..size {
                            if mask[[y, x]] {
                                cands.push((fj[[y, x]], y, x));
                            }
                        }
                    }
                    cands.sort_by(|a, b| {
                        b.0.partial_cmp(&a.0).unwrap().then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2))
                    });
                    for &(_, y, x) in cands.iter() {
                        if have >= min_n {
                            break;
                        }
                        let clear = deposits.iter().filter(|d| d.r == good).all(|d| {
                            let dx = d.x - x as i64;
                            let dy = d.y - y as i64;
                            dx * dx + dy * dy >= 12 * 12
                        });
                        if !clear {
                            continue;
                        }
                        let rich = (field[[y, x]] - lo) / (hi - lo).max(1e-9);
                        let richv = crate::util::round2(0.35 + 0.65 * rich);
                        deposits.push(Deposit::new(
                            good,
                            x as i64,
                            y as i64,
                            richv,
                            rng.gen::<f64>() < initial_known_p(good),
                            reserve_months(good, richv, rng.gen::<f64>()),
                        ));
                        have += 1;
                    }
                }
            }
        }
    }

    // ---- the floor of fate: some seams the world simply must hold.
    // Noise alone can starve a 512-world of gold entirely — and with it
    // coinage, rushes and the whole late-game arc — so every mineral is
    // guaranteed a minimum number of seams, set into the highest fitting
    // ground the map offers. Deterministic in the seed.
    let minima = STRATEGIC_MINIMA;
    let (rows, cols) = height.dim();
    for (mi, &(good, min_n, h_lo)) in minima.iter().enumerate() {
        let have = deposits.iter().filter(|d| d.r == good).count();
        if have >= min_n {
            continue;
        }
        let mut rng = crate::util::rng(seed * 47 + 4700 + mi as i64);
        // highest fitting ground first; lower the floor if the world is flat
        let mut floor = h_lo;
        let mut cands: Vec<(i64, usize, usize)> = Vec::new();
        loop {
            cands.clear();
            for y in 0..rows {
                for x in 0..cols {
                    if height[[y, x]] as f64 >= floor {
                        cands.push(((height[[y, x]] as f64 * 1e6) as i64, y, x));
                    }
                }
            }
            if cands.len() >= 40 || floor <= 0.05 {
                break;
            }
            floor -= 0.08;
        }
        cands.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        let mut need = min_n - have;
        for &(_, y, x) in cands.iter() {
            if need == 0 {
                break;
            }
            let clear = deposits.iter().filter(|d| d.r == good).all(|d| {
                let dx = d.x - x as i64;
                let dy = d.y - y as i64;
                dx * dx + dy * dy >= 12 * 12
            });
            if !clear {
                continue;
            }
            let richv = crate::util::round2(0.55 + 0.40 * rng.gen::<f64>());
            deposits.push(Deposit::new(
                good,
                x as i64,
                y as i64,
                richv,
                rng.gen::<f64>() < initial_known_p(good),
                reserve_months(good, richv, rng.gen::<f64>()),
            ));
            need -= 1;
        }
    }

    // ---- M14.2 — the salting shore. The gate demands salt towns on
    // suitable coasts, so every world holds at least one coastal pan
    // (renewing, plain to see) and at least two salt sources overall.
    // Driest shore first: desert pans before savanna before grass.
    {
        let coast = coastal_land(height);
        let arid = |b: u8| b == gc::DESERT || b == gc::SAVANNA || b == gc::GRASSLAND;
        let clear_of_salt = |deposits: &[Deposit], y: usize, x: usize| {
            deposits.iter().filter(|d| d.r == Good::Salt).all(|d| {
                let dx = d.x - x as i64;
                let dy = d.y - y as i64;
                dx * dx + dy * dy >= 12 * 12
            })
        };
        let mut rng = crate::util::rng(seed * 47 + 4800);
        let have_pan = deposits
            .iter()
            .any(|d| d.r == Good::Salt && d.left < 0.0);
        if !have_pan {
            let mut cands: Vec<(u8, usize, usize)> = Vec::new();
            for y in 0..rows {
                for x in 0..cols {
                    let b = biomes[[y, x]];
                    if coast[[y, x]] && arid(b) {
                        let pri = if b == gc::DESERT { 0 } else if b == gc::SAVANNA { 1 } else { 2 };
                        cands.push((pri, y, x));
                    }
                }
            }
            cands.sort();
            if let Some(&(_, y, x)) = cands.iter().find(|&&(_, y, x)| clear_of_salt(&deposits, y, x)) {
                deposits.push(Deposit::new(
                    Good::Salt,
                    x as i64,
                    y as i64,
                    crate::util::round2(0.55 + 0.40 * rng.gen::<f64>()),
                    true,
                    -1.0,
                ));
            }
        }
        if deposits.iter().filter(|d| d.r == Good::Salt).count() < 2 {
            'bed: for y in 0..rows {
                for x in 0..cols {
                    let b = biomes[[y, x]];
                    let hh = height[[y, x]];
                    if arid(b) && hh > 0.15 && hh <= 0.5 && clear_of_salt(&deposits, y, x) {
                        let richv = crate::util::round2(0.45 + 0.40 * rng.gen::<f64>());
                        deposits.push(Deposit::new(
                            Good::Salt,
                            x as i64,
                            y as i64,
                            richv,
                            rng.gen::<f64>() < initial_known_p(Good::Salt),
                            reserve_months(Good::Salt, richv, rng.gen::<f64>()),
                        ));
                        break 'bed;
                    }
                }
            }
        }
    }
    deposits
}

/// Client vocabulary, derived row-by-row from the GOODS table (M14.1) —
/// engine and client cannot drift because there is nothing else to copy.
/// Goods the map never places (`Place::None`) carry `"virtual": true`.
pub fn resource_meta() -> Value {
    let mut meta = serde_json::Map::new();
    for good in Good::iter() {
        let s = good.spec();
        let mut row = json!({
            "category": s.category,
            "abundance": s.abundance.to_string(),
            "requires": s.requires,
            "isa": s.isa,
            "color": s.color,
            "transport": s.transport.to_string(),
            "perishable": s.perishable,
        });
        if matches!(s.place, Place::None) {
            row["virtual"] = json!(true);
        }
        meta.insert(good.name().to_string(), row);
    }
    Value::Object(meta)
}

/// Inline goods list — settlements carry at most 8 goods (E1.11).
pub type Goods = smallvec::SmallVec<[Good; 8]>;

// ---------------------------------------------------------------- bands

use crate::util::Band;

/// Diagnostics bands (E2.7): what the ground holds.
pub const BANDS: &[Band] = &[
    Band { name: "deposits per 1000 land cells", sweet: (1.0, 6.0), hard: (0.5, 12.0), target: "sweet 1–6 · hard 0.5–12" },
    Band { name: "mineral hidden share at dawn", sweet: (0.45, 0.85), hard: (0.25, 0.95), target: "sweet 45–85% — leave an age of prospectors" },
    Band { name: "known seams worked", sweet: (0.35, 1.0), hard: (0.10, 1.0), target: "found ore must reach the market, not rust in the hills" },
    Band { name: "ore seams in home province", sweet: (0.90, 1.0), hard: (0.80, 1.0), target: "M19 gate: geology says where ore sits — ≥90% pooled" },
];
