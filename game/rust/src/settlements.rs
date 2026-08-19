//! Settlements — port of settlements.py: founding, tiers, colony siting.

use std::collections::HashSet;

use ndarray::Array2;
use rand::Rng;
use rand_pcg::Pcg64Mcg;
use serde::Serialize;

use crate::ids::{PeopleId, RealmId, SettlementId};
use crate::constants as gc;
use crate::naming;
use crate::ndimage;
use crate::resources::{Deposit, Good, Goods};

pub const TIERS: [(i64, &str); 4] = [
    (0, "Camp"),
    (250, "Village"),
    (1000, "Town"),
    (5000, "City"),
];

pub fn tier(pop: i64) -> String {
    let mut name = TIERS[0].1;
    for (threshold, t) in TIERS {
        if pop >= threshold {
            name = t;
        }
    }
    name.to_string()
}

#[derive(Serialize, Clone)]
pub struct Settlement {
    pub id: SettlementId,
    pub name: String,
    pub x: i64,
    pub y: i64,
    pub pop: i64,
    pub tier: String,
    /// Food per head — wire precision 0.1 (E4.2 heartbeat).
    #[serde(serialize_with = "crate::util::ser_f1")]
    pub food: f64,
    /// Carrying capacity, souls — recomputed monthly by `capacity_at`
    /// (crop package + soil + Kaplan arts factor). Shipped to the client
    /// as whole souls (E4.2 heartbeat).
    #[serde(serialize_with = "crate::util::ser_round_i64")]
    pub k: f64,
    pub coastal: bool,
    pub river: bool,
    /// The people who live here (ADR-0018) — moves only by assimilation
    /// or merging (M12), never by conquest.
    pub people: PeopleId,
    /// The crown that rules here (ADR-0018) — moves by conquest,
    /// secession, union and collapse.
    pub realm: RealmId,
    /// People whose tongue coined the name — stable through conquest
    /// (names carry time, M9.2); the M3 label audit classifies against
    /// this, not the current owner.
    pub namer: PeopleId,
    pub connections: i64,
    pub goods: Goods,
    pub exports: Option<Good>,
    /// Whole coin on the wire (E4.2 heartbeat) — the client displays
    /// rounded coin, so finer precision is pure payload noise.
    #[serde(serialize_with = "crate::util::ser_round_i64")]
    pub wealth: f64,
    /// true when this town's trade takes to the sea at its own quays
    pub port: bool,
    /// Reading of the name's parts in its people's tongue (M3.3).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub ety: String,
    /// Fortification level 0–3 (M4.4): palisade, stone walls, towers.
    pub fort: u8,
    /// Older names this place has carried, oldest first (M9.2/M9.3):
    /// conquest lays a new name over the old, wear smooths a long-spoken
    /// one — the strata stay readable in the inspector.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub formerly: Vec<String>,
    /// Highest population this town has ever carried (M9.1: a town that
    /// has fallen far below its peak is dying, not merely small).
    #[serde(skip)]
    pub peak: i64,
    /// Month of founding (0 = the dawn) — young towns get grace (M9.1).
    #[serde(skip)]
    pub born: i64,
    /// M9.1 — the emigration spiral: true once the town has fallen deep
    /// below its own peak and the young are leaving faster than they are
    /// born. Shipped so the inspector can say a place is dying.
    #[serde(default)]
    pub failing: bool,
    /// Months spent pinned below two-fifths of peak (with hysteresis) —
    /// the spiral only opens after years of it, so one plague year or a
    /// Gibrat dip never kills a town that would have recovered.
    #[serde(skip)]
    pub ail: u16,
    /// M12.2 — assimilation drift, 0..1: generations spent leaning toward
    /// the crown people's tongue. Engine-internal; the flip is the event.
    #[serde(skip)]
    pub drift: f64,
    /// The people the drift leans toward — a change of crown resets it.
    #[serde(skip)]
    pub drift_to: Option<PeopleId>,
    /// M12.5 — the crown's word for a minority town; its own folk keep
    /// the map name. Shipped so the inspector can show the doubling.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub exonym: Option<String>,
    /// M20 — the stone under the town: what its quarries cut, read off
    /// the rock province at the site (granite / limestone / marble /
    /// basalt). Set by `trade::goods_for` on every assignment path.
    #[serde(skip_serializing_if = "str::is_empty")]
    pub quarry: &'static str,
    /// M24 — the rebuild arc: while the month sits before this deadline
    /// and pop below `rebuild_peak`, kin return and growth runs hot.
    /// Opened by disaster damage, closed on recovery or when the window
    /// lapses. Engine-internal; never on the wire. 0 = no arc open.
    #[serde(skip)]
    pub rebuild_until: i64,
    /// The population the arc regrows toward: the head-count the moment
    /// before the disaster struck.
    #[serde(skip)]
    pub rebuild_peak: i64,
}

/// M24 — the rebuild window, months: a struck town regrows hot for at
/// most forty years before the arc closes on whatever stands.
pub const REBUILD_WINDOW: i64 = 480;

/// Effective hinterland a town can actually farm, km² — a half-day's
/// cart out and back, shared with its neighbours (M2.5 spacing).
pub const KM2_HINTERLAND: f64 = 110.0;

/// M2.2 — carrying capacity from the crop package around (y,x).
/// Mean package density over the ~12 km disc, tuned by soil, raised by
/// arts via `kaplan` (Kaplan land-per-soul ∝ T^−0.5) and specific arts
/// (`cap_mod`: plough, aqueducts…), plus the site's own larder (delta
/// silt, fisheries, food kernels — the `food_site` term).
///
/// SINGLE SOURCE OF TRUTH: tick_month stores the result on `s.k`,
/// try_colonize and explain.rs read `s.k` — no second copy exists.
pub fn capacity_at(
    crops: &Array2<u8>,
    fert: &Array2<f32>,
    y: usize,
    x: usize,
    coastal: bool,
    food_site: f64,
    kaplan: f64,
    cap_mod: f64,
) -> f64 {
    let (rows, cols) = crops.dim();
    let mut dsum = 0.0;
    let mut n = 0.0;
    for dy in -3i64..=3 {
        for dx in -3i64..=3 {
            if dy * dy + dx * dx > 9 {
                continue;
            }
            let (yy, xx) = (y as i64 + dy, x as i64 + dx);
            if yy < 0 || xx < 0 || yy >= rows as i64 || xx >= cols as i64 {
                continue;
            }
            let (yy, xx) = (yy as usize, xx as usize);
            let d = crate::agriculture::CropPackage::from_code(crops[[yy, xx]]).density()
                * (0.45 + 0.9 * fert[[yy, xx]] as f64);
            dsum += d;
            n += 1.0;
        }
    }
    let mut density = if n > 0.0 { dsum / n } else { 0.0 };
    if coastal {
        density = density.max(6.0); // the sea is a field that never fails
    }
    density = density.max(1.2); // hunting, wells and kitchen gardens
    (KM2_HINTERLAND * density * kaplan * cap_mod + 210.0 * food_site).max(180.0)
}

pub fn territory_radius(pop: i64) -> f64 {
    2.0 + 2.4 * (pop.max(10) as f64).log10()
}

/// The working hinterland: how far a town's carters, herders and mining
/// crews actually range. One shared constant for goods listing, seam
/// claiming, crew counting and the harness — these systems drifted apart
/// once (goods at 1.8r, prospecting at 2.4r) and ore rusted in the hills.
pub fn work_radius(pop: i64) -> f64 {
    territory_radius(pop) * 2.4
}

pub fn site_food(
    food_grid: &Array2<f64>,
    fert: &Array2<f32>,
    near_fresh: &Array2<bool>,
    coast: &Array2<bool>,
    y: usize,
    x: usize,
) -> f64 {
    let v = food_grid[[y, x]]
        + 1.6 * fert[[y, x]] as f64
        + 1.4 * (near_fresh[[y, x]] as u8 as f64)
        + (coast[[y, x]] as u8 as f64);
    crate::util::round2(v.max(0.35))
}

pub struct Founded {
    pub settlements: Vec<Settlement>,
    pub site_score: Array2<f64>,
    pub food_grid: Array2<f64>,
    pub near_fresh: Array2<bool>,
    pub coast: Array2<bool>,
    pub max_settlements: usize,
}

/// Score cells and greedily found settlements with min spacing.
#[allow(clippy::too_many_arguments)]
pub fn found_settlements(
    height: &Array2<f32>,
    biomes: &Array2<u8>,
    tmean: &Array2<f32>,
    rivers: &Array2<bool>,
    lakes: &Array2<bool>,
    discharge: &Array2<f32>,
    deposits: &[Deposit],
    fert: &Array2<f32>,
    rng: &mut Pcg64Mcg,
    taken: &mut HashSet<String>,
) -> Founded {
    let size = height.dim().0;
    let land = height.mapv(|h| h >= 0.0);
    let sea = height.mapv(|h| h < 0.0);

    let sea_adj = ndimage::binary_dilation(&sea, 2);
    // fresh water counts within one cell (4 km) — a real riverside claim,
    // not the whole floodplain, so dry-coast harbours stay in the running.
    let riv_adj = ndimage::binary_dilation(rivers, 1);
    let lake_adj = ndimage::binary_dilation(lakes, 1);
    let coast = Array2::from_shape_fn((size, size), |(y, x)| land[[y, x]] && sea_adj[[y, x]]);
    let near_fresh = Array2::from_shape_fn((size, size), |(y, x)| {
        land[[y, x]] && (riv_adj[[y, x]] || lake_adj[[y, x]])
    });

    // River deltas: where a great river meets the tide, the silt piles
    // deep and every keel and cart must meet. The bigger the river, the
    // harder its mouth pulls settlement toward it.
    let mut delta = Array2::<f64>::zeros((size, size));
    for y in 0..size {
        for x in 0..size {
            if !rivers[[y, x]] || discharge[[y, x]] < 60.0 {
                continue;
            }
            let mut mouth = false;
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (ny, nx) = (y as i64 + dy, x as i64 + dx);
                    if ny < 0 || nx < 0 || ny >= size as i64 || nx >= size as i64 {
                        continue;
                    }
                    if height[[ny as usize, nx as usize]] < 0.0 {
                        mouth = true;
                    }
                }
            }
            if !mouth {
                continue;
            }
            let w = (discharge[[y, x]] as f64 / 300.0).sqrt().clamp(0.6, 1.8);
            let rr = 6i64;
            for dy in -rr..=rr {
                for dx in -rr..=rr {
                    let (ny, nx) = (y as i64 + dy, x as i64 + dx);
                    if ny < 0 || nx < 0 || ny >= size as i64 || nx >= size as i64 {
                        continue;
                    }
                    let d = ((dy * dy + dx * dx) as f64).sqrt();
                    if d > rr as f64 {
                        continue;
                    }
                    let v = w * (1.0 - d / (rr as f64 + 1.0));
                    let cell = &mut delta[[ny as usize, nx as usize]];
                    if v > *cell {
                        *cell = v;
                    }
                }
            }
        }
    }

    // food kernel from deposits whose ISA chain reaches "food"
    let mut food = Array2::<f64>::zeros((size, size));
    for d in deposits {
        if d.r.is_food() {
            food[[d.y as usize, d.x as usize]] += d.rich;
        }
    }
    let mut food = ndimage::gaussian_filter(&food, 5.0).mapv(|v| (v * 60.0).clamp(0.0, 3.0));
    // delta silt and estuary fisheries feed towns for free
    food.zip_mut_with(&delta, |f, &d| *f += 0.9 * d);

    let mut score = Array2::<f64>::zeros((size, size));
    for y in 0..size {
        for x in 0..size {
            if !land[[y, x]] {
                score[[y, x]] = -1e9;
                continue;
            }
            let comfort = (-(((tmean[[y, x]] as f64 - 12.0) / 14.0).powi(2))).exp();
            let b = biomes[[y, x]];
            // fresh water pulls hard but no longer vetoes: a sheltered
            // coast with good soil can found on wells and cisterns.
            score[[y, x]] = 1.5 * (near_fresh[[y, x]] as u8 as f64)
                + 1.8 * (coast[[y, x]] as u8 as f64)
                + 2.8 * delta[[y, x]]
                + food[[y, x]]
                + 2.0 * comfort
                + 2.6 * fert[[y, x]] as f64
                - 2.5 * ((b == gc::DESERT) as u8 as f64)
                - 3.5 * ((b == gc::ICE) as u8 as f64)
                - 1.5 * ((b == gc::TUNDRA) as u8 as f64)
                - 2.0 * (height[[y, x]] as f64 - 0.5).clamp(0.0, 1.0) * 4.0;
        }
    }

    let mut settlements: Vec<Settlement> = Vec::new();
    let mut working = score.clone();
    let n_target = (size / 32).max(6);
    let min_dist = size as f64 / 18.0;
    let min_d2 = min_dist * min_dist;

    for _ in 0..n_target * 3 {
        if settlements.len() >= n_target {
            break;
        }
        // row-major first maximum, like np.argmax
        let mut best = f64::NEG_INFINITY;
        let (mut by, mut bx) = (0usize, 0usize);
        for y in 0..size {
            for x in 0..size {
                if working[[y, x]] > best {
                    best = working[[y, x]];
                    by = y;
                    bx = x;
                }
            }
        }
        if best < 2.0 {
            break;
        }
        let pop = rng.gen_range(40..140) as i64;
        settlements.push(Settlement {
            id: SettlementId(settlements.len() as i64),
            name: naming::make_word(rng, "hellenic", taken),
            x: bx as i64,
            y: by as i64,
            pop,
            tier: tier(pop),
            food: site_food(&food, fert, &near_fresh, &coast, by, bx),
            k: 0.0, // set by World::generate once the crop grid exists
            coastal: coast[[by, bx]],
            river: near_fresh[[by, bx]],
            people: PeopleId(0),
            realm: RealmId(0),
            namer: PeopleId(0),
            connections: 0,
            goods: Goods::new(),
            exports: None,
            wealth: crate::util::round2(pop as f64 * 0.2),
            port: false,
            ety: String::new(), // filled when cultures re-name in their tongue
            fort: 0,
            formerly: Vec::new(),
            peak: pop,
            born: 0,
            failing: false,
            ail: 0,
            drift: 0.0,
            drift_to: None,
            exonym: None,
            quarry: "",
            rebuild_until: 0,
            rebuild_peak: 0,
        });
        for y in 0..size {
            for x in 0..size {
                let dy = y as f64 - by as f64;
                let dx = x as f64 - bx as f64;
                if dy * dy + dx * dx < min_d2 {
                    working[[y, x]] = -1e9;
                }
            }
        }
    }

    Founded {
        settlements,
        site_score: score,
        food_grid: food,
        near_fresh,
        coast,
        // 6× the dawn towns: with market-town spacing (M2.5) the land can
        // hold a dense web of villages between the regional capitals.
        max_settlements: n_target * 6,
    }
}

/// Minimum spacing between any two towns, in cells. Calibrated against
/// the 15–30 km market-town band (M2.5): 6 cells = 24 km at 4 km/cell,
/// so settled cores tighten toward real market-shed distances while the
/// frontier stays sparse.
pub const MIN_TOWN_SPACING_CELLS: f64 = 6.0;

/// Best colony site in the ring around a parent, clear of others. The outer
/// edge of the ring widens as a people masters sail and star-charts.
///
/// `pull` is the market's voice: unworked seams project a price-weighted
/// attraction, so a rich vein can carry a hungry site past the food gate —
/// the colony goes for the ore, not the soil.
pub fn colony_site(
    site_score: &Array2<f64>,
    pull: &Array2<f64>,
    settlements: &[Settlement],
    parent: &Settlement,
    max_d2: f64,
) -> Option<(usize, usize)> {
    let (rows, cols) = site_score.dim();
    let min_d2 = MIN_TOWN_SPACING_CELLS * MIN_TOWN_SPACING_CELLS;
    let mut best = f64::NEG_INFINITY;
    let mut found: Option<(usize, usize)> = None;
    for y in 0..rows {
        for x in 0..cols {
            let s = site_score[[y, x]] + pull[[y, x]];
            if s <= 2.2 || s <= best {
                continue;
            }
            let dyp = y as f64 - parent.y as f64;
            let dxp = x as f64 - parent.x as f64;
            let d2p = dyp * dyp + dxp * dxp;
            if d2p < 64.0 || d2p > max_d2 {
                continue;
            }
            let mut clear = true;
            for o in settlements {
                let dy = y as f64 - o.y as f64;
                let dx = x as f64 - o.x as f64;
                if dy * dy + dx * dx < min_d2 {
                    clear = false;
                    break;
                }
            }
            if clear {
                best = s;
                found = Some((y, x));
            }
        }
    }
    found
}

// ---------------------------------------------------------------- bands

use crate::util::Band;

/// Diagnostics bands (E2.7): how the towns grow.
pub const BANDS: &[Band] = &[
    Band { name: "century growth", sweet: (2.0, 1200.0), hard: (1.05, 3000.0), target: "M2 crop-package K: sweet 2–1200×" },
    Band { name: "rank-size slope (Zipf)", sweet: (-1.3, -0.8), hard: (-1.75, -0.5), target: "M2.3 gate: −1.3…−0.8" },
    Band { name: "median town spacing", sweet: (14.0, 48.0), hard: (8.0, 120.0), target: "M2.5: market-town band ~15–30 km in settled cores" },
    Band { name: "mean rank-size slope", sweet: (-1.3, -0.8), hard: (-1.9, -0.45), target: "M2.3 gate: Zipf holds across the sweep" },
];
