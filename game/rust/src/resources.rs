//! Resources — the triples ontology from resources.py, ported.
//!
//! The ISA / REQUIRES / ABUNDANCE / FOUNDIN relations are kept as explicit
//! relation lookups (the (rel, :right, a) -> b dict of the original), with
//! the ISA transitive closure walked exactly like apply-kleene*.

use ndarray::Array2;
use serde::Serialize;
use serde_json::{json, Value};

use crate::constants as gc;
use crate::ndimage;
use crate::noisegen::Perlin3;
use rand::Rng;

pub const ALL_PLACEABLE: [&str; 19] = [
    "bananas",
    "blueberries",
    "strawberries",
    "blackberries",
    "cattle",
    "sheep",
    "horse",
    "pig",
    "deer",
    "elk",
    "fish",
    "timber",
    "stone",
    "coal",
    "copper",
    "iron",
    "silver",
    "gold",
    "mithril",
];

/// (ISA, :right, a) -> b — one parent per subject, exactly as the dict ends up.
fn isa_parent(name: &str) -> Option<&'static str> {
    Some(match name {
        "bananas" => "fruit",
        "blueberries" | "strawberries" | "blackberries" => "berry",
        "berry" => "fruit",
        "fruit" => "food",
        "cattle" | "sheep" | "horse" | "pig" => "livestock",
        "deer" | "elk" => "game",
        "livestock" | "game" => "animal",
        "animal" => "food",
        "fish" => "food",
        "timber" | "stone" => "material",
        "coal" => "fuel",
        "copper" | "silver" | "gold" | "iron" | "mithril" => "metal",
        "metal" => "material",
        _ => return None,
    })
}

fn requires_direct(name: &str) -> Option<&'static str> {
    Some(match name {
        "berry" => "gathering",
        "fish" => "fishing",
        "metal" => "metal-working",
        "iron" => "iron-working",
        "mithril" => "mithril-smithing",
        _ => return None,
    })
}

fn abundance_direct(name: &str) -> Option<&'static str> {
    Some(match name {
        "coal" | "copper" | "iron" | "stone" | "timber" => "common",
        "silver" | "cattle" | "horse" => "uncommon",
        "gold" => "rare",
        "mithril" => "legendary",
        _ => return None,
    })
}

/// (FOUNDIN, :right, a) -> b — kept for ontology completeness.
pub fn foundin(name: &str) -> Option<&'static str> {
    if isa_chain(name).iter().any(|s| s == "metal") {
        Some("mountain")
    } else {
        None
    }
}

/// Transitive ISA closure, origin first — the original apply-kleene*.
pub fn isa_chain(name: &str) -> Vec<String> {
    let mut chain = vec![name.to_string()];
    let mut cur = name.to_string();
    while let Some(next) = isa_parent(&cur) {
        if chain.contains(&next.to_string()) {
            break;
        }
        chain.push(next.to_string());
        cur = next.to_string();
    }
    chain
}

/// First REQUIRES found walking up the ISA chain.
pub fn requires(name: &str) -> Option<&'static str> {
    for step in isa_chain(name) {
        if let Some(r) = requires_direct(&step) {
            return Some(r);
        }
    }
    None
}

pub fn abundance(name: &str) -> &'static str {
    for step in isa_chain(name) {
        if let Some(a) = abundance_direct(&step) {
            return a;
        }
    }
    "common"
}

pub fn category(name: &str) -> String {
    let chain = isa_chain(name);
    for top in ["food", "material", "fuel"] {
        if chain.iter().any(|s| s == top) {
            return top.to_string();
        }
    }
    if chain.len() > 1 {
        chain[chain.len() - 1].clone()
    } else {
        "misc".to_string()
    }
}

fn abundance_quantile(ab: &str) -> f64 {
    match ab {
        "common" => 0.945,
        "uncommon" => 0.975,
        "rare" => 0.988,
        "legendary" => 0.9965,
        _ => 0.945,
    }
}

fn display_color(name: &str) -> &'static str {
    match name {
        "bananas" => "#f5d442",
        "blueberries" => "#5b6ee1",
        "strawberries" => "#e4485b",
        "blackberries" => "#6b3fa0",
        "cattle" => "#c98d5a",
        "sheep" => "#e8e2d0",
        "horse" => "#a9754f",
        "pig" => "#e0a3a3",
        "deer" => "#b08968",
        "elk" => "#8a6f52",
        "fish" => "#7fd4e8",
        "timber" => "#4f8f3a",
        "stone" => "#9aa2ad",
        "coal" => "#3a3f46",
        "copper" => "#d97742",
        "silver" => "#c8d0da",
        "gold" => "#f2c14e",
        "iron" => "#8f4f38",
        "mithril" => "#8ef0e2",
        _ => "#cccccc",
    }
}

#[derive(Serialize, Clone)]
pub struct Deposit {
    pub r: String,
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
fn initial_known_p(name: &str) -> f64 {
    match name {
        "stone" => 0.60,
        "coal" | "copper" => 0.25,
        "iron" => 0.20,
        "silver" => 0.06,
        "gold" => 0.03,
        "mithril" => 0.0,
        _ => 1.0,
    }
}

/// Reserve in months-of-working; -1 for goods the land renews.
fn reserve_months(name: &str, rich: f64, roll: f64) -> f64 {
    let (base, spread) = match name {
        "coal" => (480.0, 960.0),
        "copper" | "iron" => (520.0, 980.0),
        "silver" => (320.0, 640.0),
        "gold" => (260.0, 520.0),
        "mithril" => (1400.0, 1200.0),
        _ => return -1.0,
    };
    ((base + spread * rich) * (0.85 + 0.30 * roll)).round()
}

/// Coastal water cells (sea adjacent to land).
fn coastal_water(height: &Array2<f64>) -> Array2<bool> {
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
    name: &str,
    biomes: &Array2<u8>,
    height: &Array2<f64>,
    rivers: &Array2<bool>,
    lakes: &Array2<bool>,
) -> Array2<bool> {
    let land = |y: usize, x: usize| height[[y, x]] >= 0.0;
    let h = height;
    let dim = height.dim();
    match name {
        "bananas" => biome_mask(biomes, &[gc::TROPICAL_RAIN_FOREST]),
        "blueberries" => biome_mask(biomes, &[gc::BOREAL_FOREST, gc::TUNDRA]),
        "strawberries" => biome_mask(biomes, &[gc::GRASSLAND, gc::WOODLAND]),
        "blackberries" => biome_mask(biomes, &[gc::WOODLAND, gc::SEASONAL_RAIN_FOREST]),
        "cattle" => biome_mask(biomes, &[gc::GRASSLAND]),
        "sheep" => Array2::from_shape_fn(dim, |(y, x)| {
            let g = biomes[[y, x]] == gc::GRASSLAND || biomes[[y, x]] == gc::TUNDRA;
            let hills = land(y, x) && h[[y, x]] > 0.3 && h[[y, x]] <= 0.6;
            g || hills
        }),
        "horse" => biome_mask(biomes, &[gc::GRASSLAND, gc::SAVANNA]),
        "pig" => biome_mask(biomes, &[gc::WOODLAND, gc::SEASONAL_RAIN_FOREST]),
        "deer" => biome_mask(
            biomes,
            &[gc::WOODLAND, gc::SEASONAL_RAIN_FOREST, gc::TEMPERATE_RAIN_FOREST],
        ),
        "elk" => biome_mask(biomes, &[gc::BOREAL_FOREST, gc::TUNDRA]),
        "fish" => {
            let coast = coastal_water(height);
            Array2::from_shape_fn(dim, |(y, x)| {
                coast[[y, x]] || rivers[[y, x]] || lakes[[y, x]]
            })
        }
        "timber" => biome_mask(
            biomes,
            &[
                gc::WOODLAND,
                gc::SEASONAL_RAIN_FOREST,
                gc::TEMPERATE_RAIN_FOREST,
                gc::BOREAL_FOREST,
                gc::TROPICAL_RAIN_FOREST,
            ],
        ),
        "stone" => Array2::from_shape_fn(dim, |(y, x)| land(y, x) && h[[y, x]] > 0.5),
        "coal" => Array2::from_shape_fn(dim, |(y, x)| {
            land(y, x) && h[[y, x]] > 0.3 && h[[y, x]] <= 0.6
        }),
        "copper" | "iron" => {
            Array2::from_shape_fn(dim, |(y, x)| land(y, x) && h[[y, x]] > 0.45)
        }
        "silver" => Array2::from_shape_fn(dim, |(y, x)| land(y, x) && h[[y, x]] > 0.55),
        "gold" => Array2::from_shape_fn(dim, |(y, x)| {
            (land(y, x) && h[[y, x]] > 0.6) || (rivers[[y, x]] && h[[y, x]] > 0.35)
        }),
        "mithril" => Array2::from_shape_fn(dim, |(y, x)| land(y, x) && h[[y, x]] > 0.8),
        _ => Array2::from_elem(dim, false),
    }
}

/// Deposits thinned to local maxima — port of place_resources.
pub fn place_resources(
    biomes: &Array2<u8>,
    height: &Array2<f64>,
    rivers: &Array2<bool>,
    lakes: &Array2<bool>,
    seed: i64,
) -> Vec<Deposit> {
    let size = height.dim().0;
    let half = size / 2;
    let noise = Perlin3::new(seed + 5000);
    let mut deposits = Vec::new();

    for (i, name) in ALL_PLACEABLE.iter().enumerate() {
        let mask = suitability(name, biomes, height, rivers, lakes);
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
        let q = abundance_quantile(abundance(name));
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
                        r: name.to_string(),
                        x: x as i64,
                        y: y as i64,
                        rich: richv,
                        known: rng.gen::<f64>() < initial_known_p(name),
                        left: reserve_months(name, richv, rng.gen::<f64>()),
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
    let minima: [(&str, usize, f64); 7] = [
        ("stone", 4, 0.45),
        ("coal", 4, 0.28),
        ("copper", 4, 0.40),
        ("iron", 4, 0.40),
        ("silver", 2, 0.48),
        ("gold", 2, 0.42),
        ("mithril", 1, 0.55),
    ];
    let (rows, cols) = height.dim();
    for (mi, &(name, min_n, h_lo)) in minima.iter().enumerate() {
        let have = deposits.iter().filter(|d| d.r == name).count();
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
                    if height[[y, x]] >= floor {
                        cands.push(((height[[y, x]] * 1e6) as i64, y, x));
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
            let clear = deposits.iter().filter(|d| d.r == name).all(|d| {
                let dx = d.x - x as i64;
                let dy = d.y - y as i64;
                dx * dx + dy * dy >= 12 * 12
            });
            if !clear {
                continue;
            }
            let richv = crate::util::round2(0.55 + 0.40 * rng.gen::<f64>());
            deposits.push(Deposit {
                r: name.to_string(),
                x: x as i64,
                y: y as i64,
                rich: richv,
                known: rng.gen::<f64>() < initial_known_p(name),
                left: reserve_months(name, richv, rng.gen::<f64>()),
            });
            need -= 1;
        }
    }
    deposits
}

pub fn resource_meta() -> Value {
    let mut meta = serde_json::Map::new();
    for name in ALL_PLACEABLE {
        let chain = isa_chain(name);
        meta.insert(
            name.to_string(),
            json!({
                "category": category(name),
                "abundance": abundance(name),
                "requires": requires(name),
                "isa": &chain[1..],
                "color": display_color(name),
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
