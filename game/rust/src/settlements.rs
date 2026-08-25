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
    /// M79 — the harbour's wound, 0..1: the share of the town's seaborne
    /// trade its quays cannot handle while the moles are broken and the
    /// fleet is scattered. Opened by a storm landfall, repaired month by
    /// month. Engine-internal; folded into `hash_state`, never on the wire.
    #[serde(skip)]
    pub harbor_dmg: f64,
    /// The month the harbour is whole again come what may — the repair
    /// window's far edge, so no wound can outlive its arc.
    #[serde(skip)]
    pub harbor_until: i64,
}

/// M24 — the rebuild window, months: a struck town regrows hot for at
/// most forty years before the arc closes on whatever stands.
pub const REBUILD_WINDOW: i64 = 480;

// -------------------------------------------------- M79 · the harbour's wound
//
// A wrecked harbour is not a wrecked town: the quays, moles and boats go
// first and come back first. The numbers say a bad strike takes most of
// a port's water trade for a season and is essentially forgotten inside
// three years — the shape of a real rebuild, front-loaded and total.

/// The worst a single coast can be left: even a direct hit leaves some
/// beach to land a boat on.
pub const HARBOR_DMG_MAX: f64 = 0.85;
/// The share of the wound still standing a month later — 0.80 halves the
/// damage in ~3 months and clears a full hit inside ~2 years.
pub const HARBOR_REPAIR: f64 = 0.80;
/// Below this the harbour is called whole again.
pub const HARBOR_CLEAR: f64 = 0.02;
/// The far edge of any repair arc, months: three years and the coast has
/// its harbour back whatever else happened.
pub const HARBOR_WINDOW: i64 = 36;
/// A strike this weak is not worth a ledger row.
pub const HARBOR_MARK_MIN: f64 = 0.05;
/// A wound this deep gets its own chronicle beat.
pub const HARBOR_TELL_MIN: f64 = 0.35;


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
    /// M55 — arid ground with no water a people can reach without a
    /// shaft: no surface water, no spring, no oasis. Siting refuses
    /// these cells until the well tech reaches the table beneath.
    pub arid_dry: Array2<bool>,
    /// M55 — the site score those arid-dry cells would carry if their
    /// water were reachable; -1e9 everywhere else.
    pub dry_site_score: Array2<f64>,
    pub max_settlements: usize,
}

// ------------------------------------------------------ M55 dry-land water

/// M55 — how deep a people can sink a well, in metres of table.
///
/// A dry site is only habitable if someone can reach the water under it,
/// and reach is a matter of craft. Hand-dug pits in loose ground go a
/// few metres; a lined and cased shaft (masonry) holds open far deeper;
/// the qanat, the horizontal gallery that made the Persian desert
/// habitable, comes with the same water-engineering that raises the
/// aqueduct; and full engineering drives shafts to the regional table.
/// With no craft at all there are no wells — only springs, oases and
/// running water.
pub fn well_reach_m(soc: &crate::society::Society) -> f64 {
    use crate::society::TechId as T;
    if soc.knows(T::Engineering) {
        90.0
    } else if soc.knows(T::Aqueduct) {
        60.0
    } else if soc.knows(T::Masonry) {
        30.0
    } else if soc.knows(T::Pottery) || soc.knows(T::Stonecraft) {
        12.0
    } else {
        0.0
    }
}

/// Can this people put a town on that cell? Everything not arid-dry is
/// open; arid-dry ground opens only when the well reach clears the depth
/// to water there (M55 gate).
pub fn dry_site_ok(
    arid_dry: &Array2<bool>,
    aquifer: &Array2<f32>,
    y: usize,
    x: usize,
    well_reach: f64,
) -> bool {
    if !arid_dry[[y, x]] {
        return true;
    }
    well_reach > 0.0 && (aquifer[[y, x]] as f64) <= well_reach
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
    shelter: &Array2<f32>,
    // M55 — the dry-land water: the day-lit table and the desert's own
    // shallow reserves, plus the rainfall the ground actually gets.
    springs: &Array2<bool>,
    oases: &Array2<bool>,
    precip: &Array2<f32>,
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

    // M55 — arid ground with no drinkable water at the surface. The sea
    // is not water access: a desert harbour drinks from a well or a
    // spring or it does not drink. Springs and oases count within one
    // cell, the same 4 km claim fresh water gets — a town stands beside
    // its water, it does not commute to it.
    let spring_adj = ndimage::binary_dilation(springs, 1);
    let oasis_adj = ndimage::binary_dilation(oases, 1);
    let arid_dry = Array2::from_shape_fn((size, size), |(y, x)| {
        land[[y, x]]
            && crate::hydrology::arid(biomes[[y, x]], precip[[y, x]] as f64)
            && !near_fresh[[y, x]]
            && !spring_adj[[y, x]]
            && !oasis_adj[[y, x]]
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
    // M55 — what an arid-dry cell would be worth if its water could be
    // reached. Held aside so the dawn cannot found there, while a later
    // people with wells can bid for the same ground.
    let mut dry_score = Array2::<f64>::from_elem((size, size), -1e9);
    for y in 0..size {
        for x in 0..size {
            if !land[[y, x]] {
                score[[y, x]] = -1e9;
                continue;
            }
            // M55 — the dawn peoples have no shafts. Arid ground with no
            // surface water, spring or oasis is unfoundable until the
            // well craft arrives; colonisation re-tests it against reach.
            let comfort = (-(((tmean[[y, x]] as f64 - 12.0) / 14.0).powi(2))).exp();
            let b = biomes[[y, x]];
            // fresh water pulls hard but no longer vetoes: a sheltered
            // coast with good soil can found on wells and cisterns.
            // M45 — the coast is no longer one flat bonus: mere adjacency
            // pays a keep-alive 0.3 (was 1.8) and the sailor's reading of
            // the anchorage pays on top — and the term is CONVEX
            // (4.5·sh + 4.5·sh³), because the sailor's eye is: a great
            // harbour (0.8) is worth far more than twice a passable
            // roadstead (0.4). Measured, not guessed: with a linear term
            // (tried at 4.5 and 6.5) the soil terms kept outbidding the
            // cove and towns founded on the bluff beside 0.7+ water (the
            // "missed" cohort of the M45 dump); the cubic tail is what
            // makes Genoa take the rocky cove over the fertile strand.
            //
            // The delta's draw splits in two (Alexandria's law): half is
            // soil — silt feeds a town wherever it stands — and half is
            // gateway, worth carrying only where ships can actually lie,
            // so the gateway half scales with the anchorage under it.
            let sh = shelter[[y, x]] as f64;
            let dv = delta[[y, x]];
            score[[y, x]] = 1.5 * (near_fresh[[y, x]] as u8 as f64)
                + 0.3 * (coast[[y, x]] as u8 as f64)
                + 4.5 * sh
                + 4.5 * sh * sh * sh
                + 1.4 * dv
                + 1.4 * dv * (0.2 + 1.6 * sh).min(1.6)
                + food[[y, x]]
                + 2.0 * comfort
                + 2.6 * fert[[y, x]] as f64
                - 2.5 * ((b == gc::DESERT) as u8 as f64)
                - 3.5 * ((b == gc::ICE) as u8 as f64)
                - 1.5 * ((b == gc::TUNDRA || b == gc::WET_TUNDRA) as u8 as f64)
                - 2.0 * (height[[y, x]] as f64 - 0.5).clamp(0.0, 1.0) * 4.0;

            // M55 — the dawn peoples have no shafts. Arid ground with no
            // surface water, spring or oasis is unfoundable; its score is
            // set aside for colonists who can sink a well to the table.
            if arid_dry[[y, x]] {
                dry_score[[y, x]] = score[[y, x]];
                score[[y, x]] = -1e9;
            }
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
            harbor_dmg: 0.0,
            harbor_until: 0,
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
        arid_dry,
        dry_site_score: dry_score,

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

/// M55 — a founding party's reach into dry country: the arid-dry mask,
/// the table depth under it, the score that ground would carry, and how
/// deep this people can sink a shaft.
pub struct DryFrontier<'a> {
    pub arid_dry: &'a Array2<bool>,
    pub aquifer: &'a Array2<f32>,
    pub dry_site_score: &'a Array2<f64>,
    pub well_reach_m: f64,
    /// M56 — the caravan's provisioning field (`trade::caravan_provision`):
    /// 1.0 on a watered market's own ground, falling to 0.0 where no
    /// caravan out of a market can still be victualled.
    pub provision: &'a Array2<f32>,
}

// ------------------------------------------------------ M56 the caravan frontier

/// M56 — how much a mining camp is worth *as a mine*. The ordinary site
/// score prices a cell as a farm; a camp on the dry frontier is priced
/// by the seam under it, victualled by caravan. Potosi stood at 4000 m
/// on ground that grew nothing because the silver paid for every sack
/// of maize that climbed to it; the gain is what turns a market signal
/// into a reason to leave the plough.
pub const EXTRACTIVE_GAIN: f64 = 2.6;

/// M56 — what a fully provisioned caravan lane is worth as subsistence
/// to a camp that grows nothing: exactly the founding bar (2.2). A
/// supplied camp with no ore under it is precisely marginal — it is the
/// seam, not the caravan, that makes the desert worth taking.
pub const CARAVAN_SUBSISTENCE: f64 = 2.2;

/// M56 — the shaft's standing cost, in score units per metre of lift.
/// A well is not a one-off: it is rope, leather, beasts and hands every
/// day the town drinks. At 0.06 a hand-dug 12 m pit costs 0.7 — a
/// nuisance — while an engineered 90 m shaft costs 5.4, which only a
/// rich seam under a well-provisioned lane can carry. This is what
/// makes the M55 ladder load-bearing rather than a pass/fail switch.
pub const WELL_UPKEEP_PER_M: f64 = 0.06;

/// M56 — the standing cost of drinking from a table `depth` metres down.
pub fn well_upkeep(depth_m: f64) -> f64 {
    WELL_UPKEEP_PER_M * depth_m.max(0.0)
}

impl DryFrontier<'_> {
    /// The site's worth to *this* people: the ordinary score, unless the
    /// cell is arid-dry, in which case the held-aside score is unlocked
    /// exactly when the well reaches the water.
    pub fn score_at(&self, site_score: &Array2<f64>, y: usize, x: usize) -> f64 {
        if !self.arid_dry[[y, x]] {
            return site_score[[y, x]];
        }
        if self.well_reach_m > 0.0 && (self.aquifer[[y, x]] as f64) <= self.well_reach_m {
            self.dry_site_score[[y, x]]
        } else {
            -1e9
        }
    }

    /// M56 — what a site is actually *offered* for, market included.
    ///
    /// Ordinary ground is priced as it always was: the farm score plus
    /// whatever the market's unworked seams add to it. Dry ground is a
    /// different proposition and is priced as one — an extractive camp,
    /// not a farm:
    ///
    ///   · the seam's pull dominates, at `EXTRACTIVE_GAIN`, because the
    ///     camp lives on what it digs, not on what it grows;
    ///   · that pull is conditioned on the caravan that must victual it
    ///     — a seam nobody can supply is worth nothing on the ground;
    ///   · and the well is a standing cost against the yield, deeper
    ///     tables charging more, so 12 m ground is marginal and 60 m
    ///     ground pays only where the seam is rich.
    ///
    /// The M55 veto still runs first: no reach, no town, at any price.
    pub fn offer(&self, site_score: &Array2<f64>, pull: &Array2<f64>, y: usize, x: usize) -> f64 {
        if !self.arid_dry[[y, x]] {
            return site_score[[y, x]] + pull[[y, x]];
        }
        let held = self.score_at(site_score, y, x);
        if held < -1e8 {
            return held; // the well does not reach: no price opens this ground
        }
        let prov = self.provision[[y, x]] as f64;
        if prov <= 0.0 {
            return -1e9; // no caravan reaches it; a camp there starves
        }
        // A camp is not a farm. `held` prices this ground as a farm and
        // charges it the desert's full agricultural penalty; a mining
        // camp does not grow its bread, it buys it off the caravan, so
        // its floor is the victualled subsistence the lane can carry —
        // never below what the ground itself would have offered.
        let floor = held.max(CARAVAN_SUBSISTENCE * prov);
        floor + EXTRACTIVE_GAIN * pull[[y, x]] * prov
            - well_upkeep(self.aquifer[[y, x]] as f64)
    }
}

/// M45 — the harbour eye: what an anchorage is *worth* to the people
/// looking at it. The dawn site score prices the cove at dawn rates
/// (`4.5·sh + 4.5·sh³`), and that price is trade income — the gateway
/// half of a coastal site, not its soil. A people that has mastered
/// sail, the wheel, script and coin earns strictly more from the same
/// water (`society::mods_for(..).trade` is that same multiplier, the
/// one the economy already pays out on), so a colony sent out by a
/// maritime power must read the cove at *its* rates, not at the dawn's.
///
/// The correction is the trade multiplier itself, not a new constant:
/// the offer adds `(trade − 1)·(4.5·sh + 4.5·sh³)`, which is exactly
/// zero for a dawn people (`trade = 1`) — dawn siting is untouched by
/// construction — and grows only as the founder's own mastery grows.
pub struct HarbourEye<'a> {
    pub shelter: &'a Array2<f32>,
    /// The founder's realized trade multiplier (1.0 at the dawn).
    pub trade: f64,
}

impl HarbourEye<'_> {
    fn premium(&self, y: usize, x: usize) -> f64 {
        let sh = self.shelter[[y, x]] as f64;
        if sh <= 0.0 {
            return 0.0;
        }
        (self.trade - 1.0).max(0.0) * (4.5 * sh + 4.5 * sh * sh * sh)
    }
}

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
    // M55 — the dry frontier: arid ground held back from the dawn, its
    // score restored for a people whose wells reach the table beneath.
    dry: &DryFrontier<'_>,
    // M45 — the anchorage priced at the founder's own trade rates.
    sea: &HarbourEye<'_>,
) -> Option<(usize, usize)> {
    let (rows, cols) = site_score.dim();
    let min_d2 = MIN_TOWN_SPACING_CELLS * MIN_TOWN_SPACING_CELLS;
    let mut best = f64::NEG_INFINITY;
    let mut found: Option<(usize, usize)> = None;
    // E10.2 — the ring bound, hoisted out of the cell body. Every cell the
    // old full-map scan visited outside `max_d2` was scored and then thrown
    // away by the `d2p > max_d2` test below, which still stands unchanged:
    // this only refuses to walk rows and columns that cannot satisfy it.
    // `ceil` keeps the box a superset of the ring, so the exact test inside
    // remains the only thing that decides membership — same accepted cells,
    // same row-major order, same choice.
    let reach = max_d2.max(0.0).sqrt().ceil() as isize;
    let y0 = (parent.y as isize - reach).max(0) as usize;
    let y1 = ((parent.y as isize + reach + 1).max(0) as usize).min(rows);
    let x0 = (parent.x as isize - reach).max(0) as usize;
    let x1 = ((parent.x as isize + reach + 1).max(0) as usize).min(cols);
    for y in y0..y1 {
        for x in x0..x1 {
            let base = dry.offer(site_score, pull, y, x);
            if base < -1e8 {
                continue;
            }
            let s = base + sea.premium(y, x);
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

// --------------------------------------------------------------- shelter

/// M45 — enclosure disc radius around the anchorage: 6 cells = 24 km,
/// the water a harbor actually answers to.
pub const SHELTER_R: i64 = 6;
/// M45 — a working roadstead: the absolute shelter a town needs before
/// it founds a blue-water shipping line (seed-independent — the bar is
/// physics, not a quantile of this world's coast). Below it a coastal
/// town is a fishing village: it feeds the coastal web by cart and
/// lighters the odd cargo, but the far sea lanes are not its trade.
pub const SHELTER_ROADSTEAD: f32 = 0.5;
/// M45 — fetch ray cap: 48 cells = 192 km. Wave height grows with the
/// wind's runway far past 96 km (a gale over open ocean builds seas no
/// 40-km basin ever sees), so the cap must sit high enough that an
/// inland sea reads calm *relative to* the strand facing true ocean —
/// at 24 the two were nearly alike and the field flattened. Measured on
/// the seed sweep: 48 spreads basin coasts cleanly above the open coast
/// while pocket bays stay on top.
pub const SHELTER_FETCH_CAP: i64 = 48;
/// M45 — a lagoon anchorage is a harbor the coast built itself (M44).
pub const SHELTER_LAGOON: f32 = 0.35;
/// M45 — the classic hook: a spit at the shoulder breaks the swell.
pub const SHELTER_SPIT: f32 = 0.15;

/// M45 — harbor shelter: the coast read the way sailors read it.
///
/// Three voices, all pure arithmetic on the f32 height grid — no libm,
/// because the score joins `hash_state` and must replay bit-identically
/// across runtimes (the coast ledger's f32 law, M44):
///
///   · **enclosure** — land fraction in the R-disc around the anchorage:
///     a straight shore reads ~0.5, a bay-head ~0.7, a headland ~0.3;
///   · **fetch** — the *mean* open-water ray over 8 bearings (capped):
///     the sea a storm can cross before it lands, averaged the way a
///     sailor weighs an anchorage — one open bearing is a manageable
///     risk, exposure on most bearings is a roadstead in name only
///     (the max-ray form proved near-binary: any single open bearing
///     zeroed the calm, and the field collapsed onto enclosure alone);
///   · **the drift's own forms** — a LAGOON anchorage scores the bonus
///     of a coast-built harbor, a SPIT shoulder the classic hook.
///
/// Every land cell in the coastal band gets a score in [0, 1]; open sea
/// and inland cells are exactly 0.0, so inland site scoring is untouched
/// to the bit (the M45 determinism leg). The anchorage is the *best* sea
/// cell of the 5×5 — a harbor is built on the sheltered side of its
/// headland, so a town reads the best water within reach of its quay,
/// not the first cell a row-major scan happens upon. The reach stays
/// tight (±2, 8 km): widening it smears the field, and a smeared field
/// prices every beach like a harbour — the toll stops diverting cargo
/// and the port flag stops meaning anything. The max over pure f32
/// values is order-independent, so determinism holds.
pub fn shelter_score(height: &Array2<f32>, form: &Array2<u8>) -> Array2<f32> {
    let (rows, cols) = height.dim();
    let sea = height.mapv(|h| h < 0.0);
    let sea_adj = ndimage::binary_dilation(&sea, 2);
    let mut out = Array2::<f32>::zeros((rows, cols));
    let dirs: [(i64, i64); 8] =
        [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (-1, 1), (1, -1), (1, 1)];
    for y in 0..rows {
        for x in 0..cols {
            if sea[[y, x]] || !sea_adj[[y, x]] {
                continue; // coastal-band land only
            }
            // every sea cell in the 5×5 is a candidate anchorage; the
            // quay goes where the water is calmest
            let mut water = 0.0f32;
            for ady in -2i64..=2 {
                for adx in -2i64..=2 {
                    let (ay, ax) = (y as i64 + ady, x as i64 + adx);
                    if ay < 0 || ax < 0 || ay >= rows as i64 || ax >= cols as i64 {
                        continue;
                    }
                    let (ay, ax) = (ay as usize, ax as usize);
                    if !sea[[ay, ax]] {
                        continue;
                    }
                    // enclosure: land fraction in the disc at the anchorage —
                    // off-map counts as open sea (the map edge shelters no one)
                    let (mut land_n, mut tot) = (0i64, 0i64);
                    for dy in -SHELTER_R..=SHELTER_R {
                        for dx in -SHELTER_R..=SHELTER_R {
                            if dy * dy + dx * dx > SHELTER_R * SHELTER_R {
                                continue;
                            }
                            tot += 1;
                            let (ny, nx) = (ay as i64 + dy, ax as i64 + dx);
                            if ny >= 0
                                && nx >= 0
                                && ny < rows as i64
                                && nx < cols as i64
                                && !sea[[ny as usize, nx as usize]]
                            {
                                land_n += 1;
                            }
                        }
                    }
                    let landfrac = land_n as f32 / tot as f32;
                    let enclose = ((landfrac - 0.35) / 0.40).clamp(0.0, 1.0);
                    // fetch: mean open-water ray from the anchorage, 8 bearings
                    let mut fetch_sum = 0i64;
                    for (dy, dx) in dirs {
                        let (mut cy, mut cx) = (ay as i64, ax as i64);
                        let mut d = 0i64;
                        while d < SHELTER_FETCH_CAP {
                            cy += dy;
                            cx += dx;
                            if cy < 0 || cx < 0 || cy >= rows as i64 || cx >= cols as i64 {
                                d = SHELTER_FETCH_CAP; // the edge is open ocean
                                break;
                            }
                            if !sea[[cy as usize, cx as usize]] {
                                break;
                            }
                            d += 1;
                        }
                        fetch_sum += d;
                    }
                    let calm = 1.0 - fetch_sum as f32 / (8.0 * SHELTER_FETCH_CAP as f32);
                    water = water.max(0.45 * enclose + 0.45 * calm);
                }
            }
            // the drift's forms within reach of the quay
            let mut bonus = 0.0f32;
            for dy in -2i64..=2 {
                for dx in -2i64..=2 {
                    let (ny, nx) = (y as i64 + dy, x as i64 + dx);
                    if ny < 0 || nx < 0 || ny >= rows as i64 || nx >= cols as i64 {
                        continue;
                    }
                    let f = form[[ny as usize, nx as usize]];
                    if f == crate::coast::LAGOON {
                        bonus = bonus.max(SHELTER_LAGOON);
                    } else if f == crate::coast::SPIT {
                        bonus = bonus.max(SHELTER_SPIT);
                    }
                }
            }
            out[[y, x]] = (water + bonus).clamp(0.0, 1.0);
        }
    }
    out
}

// ---------------------------------------------------------------- bands

use crate::util::Band;

/// Diagnostics bands (E2.7): how the towns grow.
pub const BANDS: &[Band] = &[
    Band { name: "century growth", sweet: (2.0, 1200.0), hard: (1.05, 3000.0), target: "M2 crop-package K: sweet 2–1200×" },
    Band { name: "rank-size slope (Zipf)", sweet: (-1.3, -0.8), hard: (-1.75, -0.5), target: "M2.3 gate: −1.3…−0.8" },
    Band { name: "median town spacing", sweet: (14.0, 48.0), hard: (8.0, 120.0), target: "M2.5: market-town band ~15–30 km in settled cores" },
    Band { name: "mean rank-size slope", sweet: (-1.3, -0.8), hard: (-1.9, -0.45), target: "M2.3 gate: Zipf holds across the sweep" },
    Band { name: "port shelter concentration", sweet: (0.70, 1.0), hard: (0.55, 1.0), target: "M45 gate: ≥70% of ports well-sited — top-quartile water, or the best their coast offers" },
    Band { name: "coastal shelter p90", sweet: (0.30, 0.95), hard: (0.15, 1.0), target: "M45: the field discriminates — bays and lagoons stand above the open strand" },
    Band { name: "coastal town shelter lift", sweet: (1.2, 10.0), hard: (1.0, 25.0), target: "M45: founded coastal towns sit on better-than-average anchorages" },
];
