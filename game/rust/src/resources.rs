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
    Cattle,
    Coal,
    Copper,
    Deer,
    Elk,
    Fish,
    Gold,
    Grain,
    Horse,
    Iron,
    Jewelry,
    Mithril,
    Pig,
    Sheep,
    Silver,
    Stone,
    Strawberries,
    Timber,
    Tools,
    Weapons,
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

    /// ISA-closure flag: the old `isa_chain(g).contains("food")`.
    /// NOTE: grain is deliberately NOT food here — the legacy ontology gave
    /// grain no parent, and the price math must not change (M8 hash gate).
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

    /// metal ⊂ material, plus timber and stone.
    #[inline]
    pub const fn is_material(self) -> bool {
        self.is_metal() || matches!(self, Good::Timber | Good::Stone)
    }

    #[inline]
    pub const fn is_craft(self) -> bool {
        matches!(self, Good::Tools | Good::Weapons | Good::Jewelry)
    }

    #[inline]
    pub const fn is_fuel(self) -> bool {
        matches!(self, Good::Coal)
    }

    /// A mineral seam that mines work and rushes chase.
    #[inline]
    pub const fn is_mineral(self) -> bool {
        self.is_metal() || matches!(self, Good::Stone | Good::Coal)
    }

    pub const fn abundance(self) -> Abundance {
        match self {
            Good::Silver | Good::Cattle | Good::Horse | Good::Tools => Abundance::Uncommon,
            Good::Gold | Good::Weapons | Good::Jewelry => Abundance::Rare,
            Good::Mithril => Abundance::Legendary,
            _ => Abundance::Common,
        }
    }

    /// First REQUIRES up the old ISA chain.
    pub const fn requires(self) -> Option<&'static str> {
        Some(match self {
            Good::Blackberries | Good::Blueberries | Good::Strawberries => "gathering",
            Good::Fish => "fishing",
            Good::Copper | Good::Silver | Good::Gold => "metal-working",
            Good::Iron => "iron-working",
            Good::Mithril => "mithril-smithing",
            _ => return None,
        })
    }

    /// The ISA chain above the good itself (for client meta).
    pub const fn isa(self) -> &'static [&'static str] {
        match self {
            Good::Bananas => &["fruit", "food"],
            Good::Blackberries | Good::Blueberries | Good::Strawberries => {
                &["berry", "fruit", "food"]
            }
            Good::Cattle | Good::Sheep | Good::Horse | Good::Pig => {
                &["livestock", "animal", "food"]
            }
            Good::Deer | Good::Elk => &["game", "animal", "food"],
            Good::Fish => &["food"],
            Good::Timber | Good::Stone => &["material"],
            Good::Coal => &["fuel"],
            Good::Copper | Good::Silver | Good::Gold | Good::Iron | Good::Mithril => {
                &["metal", "material"]
            }
            Good::Tools | Good::Weapons | Good::Jewelry => &["craft"],
            Good::Grain => &[],
        }
    }

    /// Top-of-chain category, exactly as the old `category()` resolved it.
    pub const fn category(self) -> &'static str {
        if self.is_food() {
            "food"
        } else if self.is_fuel() {
            "fuel"
        } else if self.is_material() {
            "material"
        } else if self.is_craft() {
            "craft"
        } else {
            "misc" // grain: chain of one, no top category
        }
    }

    pub const fn color(self) -> &'static str {
        match self {
            Good::Bananas => "#f5d442",
            Good::Blueberries => "#5b6ee1",
            Good::Strawberries => "#e4485b",
            Good::Blackberries => "#6b3fa0",
            Good::Cattle => "#c98d5a",
            Good::Sheep => "#e8e2d0",
            Good::Horse => "#a9754f",
            Good::Pig => "#e0a3a3",
            Good::Deer => "#b08968",
            Good::Elk => "#8a6f52",
            Good::Fish => "#7fd4e8",
            Good::Timber => "#4f8f3a",
            Good::Stone => "#9aa2ad",
            Good::Coal => "#3a3f46",
            Good::Copper => "#d97742",
            Good::Silver => "#c8d0da",
            Good::Gold => "#f2c14e",
            Good::Iron => "#8f4f38",
            Good::Mithril => "#8ef0e2",
            _ => "#cccccc",
        }
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

// ---------------------------------------------------------------- GoodSet

/// A set of goods as one u32 — the ISA-closure bitmask idea (E1.3) applied
/// to every "collection of goods" in the engine. Iteration order is variant
/// (= alphabetical) order, matching the old `BTreeSet<String>` everywhere.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct GoodSet(u32);

impl GoodSet {
    pub const EMPTY: GoodSet = GoodSet(0);

    #[inline]
    pub fn insert(&mut self, g: Good) {
        self.0 |= 1 << g as u32;
    }

    #[inline]
    pub fn contains(self, g: Good) -> bool {
        self.0 & (1 << g as u32) != 0
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
pub const ALL_PLACEABLE: [Good; 19] = [
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
];

/// (FOUNDIN, :right, a) -> b — kept for ontology completeness.
pub fn foundin(g: Good) -> Option<&'static str> {
    if g.is_metal() {
        Some("mountain")
    } else {
        None
    }
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
}

/// Chance a deposit starts the story already found. Everything that walks,
/// grows or swims is plain to see; buried seams mostly are not — the age of
/// prospectors has to actually happen on stage.
fn initial_known_p(g: Good) -> f64 {
    match g {
        Good::Stone => 0.60,
        Good::Coal | Good::Copper => 0.25,
        Good::Iron => 0.20,
        Good::Silver => 0.06,
        Good::Gold => 0.03,
        Good::Mithril => 0.0,
        _ => 1.0,
    }
}

/// Reserve in months-of-working; -1 for goods the land renews.
fn reserve_months(g: Good, rich: f64, roll: f64) -> f64 {
    let (base, spread) = match g {
        Good::Coal => (480.0, 960.0),
        Good::Copper | Good::Iron => (520.0, 980.0),
        Good::Silver => (320.0, 640.0),
        Good::Gold => (260.0, 520.0),
        Good::Mithril => (1400.0, 1200.0),
        _ => return -1.0,
    };
    ((base + spread * rich) * (0.85 + 0.30 * roll)).round()
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

fn biome_mask(biomes: &Array2<u8>, ids: &[u8]) -> Array2<bool> {
    biomes.mapv(|b| ids.contains(&b))
}

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
    match g {
        Good::Bananas => biome_mask(biomes, &[gc::TROPICAL_RAIN_FOREST]),
        Good::Blueberries => biome_mask(biomes, &[gc::BOREAL_FOREST, gc::TUNDRA]),
        Good::Strawberries => biome_mask(biomes, &[gc::GRASSLAND, gc::WOODLAND]),
        Good::Blackberries => biome_mask(biomes, &[gc::WOODLAND, gc::SEASONAL_RAIN_FOREST]),
        Good::Cattle => biome_mask(biomes, &[gc::GRASSLAND]),
        Good::Sheep => Array2::from_shape_fn(dim, |(y, x)| {
            let g = biomes[[y, x]] == gc::GRASSLAND || biomes[[y, x]] == gc::TUNDRA;
            let hills = land(y, x) && h[[y, x]] > 0.3 && h[[y, x]] <= 0.6;
            g || hills
        }),
        Good::Horse => biome_mask(biomes, &[gc::GRASSLAND, gc::SAVANNA]),
        Good::Pig => biome_mask(biomes, &[gc::WOODLAND, gc::SEASONAL_RAIN_FOREST]),
        Good::Deer => biome_mask(
            biomes,
            &[gc::WOODLAND, gc::SEASONAL_RAIN_FOREST, gc::TEMPERATE_RAIN_FOREST],
        ),
        Good::Elk => biome_mask(biomes, &[gc::BOREAL_FOREST, gc::TUNDRA]),
        Good::Fish => {
            let coast = coastal_water(height);
            Array2::from_shape_fn(dim, |(y, x)| {
                coast[[y, x]] || rivers[[y, x]] || lakes[[y, x]]
            })
        }
        Good::Timber => biome_mask(
            biomes,
            &[
                gc::WOODLAND,
                gc::SEASONAL_RAIN_FOREST,
                gc::TEMPERATE_RAIN_FOREST,
                gc::BOREAL_FOREST,
                gc::TROPICAL_RAIN_FOREST,
            ],
        ),
        Good::Stone => Array2::from_shape_fn(dim, |(y, x)| land(y, x) && h[[y, x]] > 0.5),
        Good::Coal => Array2::from_shape_fn(dim, |(y, x)| {
            land(y, x) && h[[y, x]] > 0.3 && h[[y, x]] <= 0.6
        }),
        Good::Copper | Good::Iron => {
            Array2::from_shape_fn(dim, |(y, x)| land(y, x) && h[[y, x]] > 0.45)
        }
        Good::Silver => Array2::from_shape_fn(dim, |(y, x)| land(y, x) && h[[y, x]] > 0.55),
        Good::Gold => Array2::from_shape_fn(dim, |(y, x)| {
            (land(y, x) && h[[y, x]] > 0.6) || (rivers[[y, x]] && h[[y, x]] > 0.35)
        }),
        Good::Mithril => Array2::from_shape_fn(dim, |(y, x)| land(y, x) && h[[y, x]] > 0.8),
        _ => Array2::from_elem(dim, false),
    }
}

/// Deposits thinned to local maxima — port of place_resources.
pub fn place_resources(
    biomes: &Array2<u8>,
    height: &Array2<f32>,
    rivers: &Array2<bool>,
    lakes: &Array2<bool>,
    seed: i64,
) -> Vec<Deposit> {
    let size = height.dim().0;
    let half = size / 2;
    let noise = Perlin3::new(seed + 5000);
    let mut deposits = Vec::new();

    for (i, &good) in ALL_PLACEABLE.iter().enumerate() {
        let mask = suitability(good, biomes, height, rivers, lakes);
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
        let maxima = ndimage::maximum_filter(&fj, 5);

        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for &v in field.iter() {
            lo = lo.min(v);
            hi = hi.max(v);
        }
        for y in 0..size {
            for x in 0..size {
                if mask[[y, x]] && field[[y, x]] >= thresh && fj[[y, x]] == maxima[[y, x]] {
                    let rich = (field[[y, x]] - lo) / (hi - lo).max(1e-9);
                    let richv = crate::util::round2(0.35 + 0.65 * rich);
                    deposits.push(Deposit {
                        r: good,
                        x: x as i64,
                        y: y as i64,
                        rich: richv,
                        known: rng.gen::<f64>() < initial_known_p(good),
                        left: reserve_months(good, richv, rng.gen::<f64>()),
                    });
                }
            }
        }
    }

    // ---- the floor of fate: some seams the world simply must hold.
    // Noise alone can starve a 512-world of gold entirely — and with it
    // coinage, rushes and the whole late-game arc — so every mineral is
    // guaranteed a minimum number of seams, set into the highest fitting
    // ground the map offers. Deterministic in the seed.
    let minima: [(Good, usize, f64); 7] = [
        (Good::Stone, 4, 0.45),
        (Good::Coal, 4, 0.28),
        (Good::Copper, 4, 0.40),
        (Good::Iron, 4, 0.40),
        (Good::Silver, 2, 0.48),
        (Good::Gold, 2, 0.42),
        (Good::Mithril, 1, 0.55),
    ];
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
            deposits.push(Deposit {
                r: good,
                x: x as i64,
                y: y as i64,
                rich: richv,
                known: rng.gen::<f64>() < initial_known_p(good),
                left: reserve_months(good, richv, rng.gen::<f64>()),
            });
            need -= 1;
        }
    }
    deposits
}

pub fn resource_meta() -> Value {
    let mut meta = serde_json::Map::new();
    for good in ALL_PLACEABLE {
        meta.insert(
            good.name().to_string(),
            json!({
                "category": good.category(),
                "abundance": good.abundance().to_string(),
                "requires": good.requires(),
                "isa": good.isa(),
                "color": good.color(),
            }),
        );
    }
    // grain is a produced good (fertile farmland), not a map deposit
    meta.insert(
        "grain".to_string(),
        json!({
            "category": "food", "abundance": "common", "requires": "farming",
            "isa": ["food"], "color": "#e3c96b", "virtual": true,
        }),
    );
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
];
