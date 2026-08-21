//! Trade — geography-priced commerce. The sea is cheap and the land is
//! dear: a laden cart pays for every climb, every ford, every mile of
//! sand and snow, while a ship on a coastal run carries the same cargo
//! for a fraction. A* works over that truth, so roads thread the
//! passes, lanes hug the shore, barges ride the great rivers, and
//! harbours grow wherever cargo changes from wheel to keel.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap};

use ndarray::Array2;
use serde::Serialize;

use crate::ids::SettlementId;
use crate::state::CellFlags;
use crate::constants as gc;
use crate::hydrology::{DIST, N8};
use crate::ndimage;
use crate::resources::{Abundance, Deposit, Good, Goods};
use crate::settlements::Settlement;

fn rarity_w(ab: Abundance) -> f64 {
    match ab {
        Abundance::Uncommon => 1.6,
        Abundance::Rare => 2.4,
        Abundance::Legendary => 4.0,
        Abundance::Common => 1.0,
    }
}

// Travel modes carried on each route point.
pub const MODE_LAND: u8 = 0;
pub const MODE_SEA: u8 = 1;
pub const MODE_RIVER: u8 = 2;

// The economics of movement (cost per downsampled cell).
pub const COAST_SEA_COST: f64 = 0.35; // cabotage, in sight of land
pub const OPEN_SEA_COST: f64 = 0.55; // blue water: no shelter, no rescue
pub const LAKE_COST: f64 = 0.6; // flat water, short hauls
pub const LAND_BASE: f64 = 3.0; // a cart, an ox, a rutted track
pub const RIVER_BARGE_COST: f64 = 1.1; // a navigable river is a highway
pub const EMBARK_COST: f64 = 10.0; // cranes, porters and harbour dues
pub const NAVIGABLE: f64 = 90.0; // discharge above which barges swim

// M46 — the priced sea: blue water is not one price. A leg that rides
// the gyre or runs downwind sails cheap; the same water fought the
// other way is dear; and the becalmed convergence rows — where the
// zonal wind profile crosses zero and the gyres meet — are slow on
// every heading. Cabotage stays neutral: small craft row, tack and
// anchor at dusk. Only open water feels the ocean move.
/// The per-step cost A*'s straight-line heuristic plans with. No cell
/// may ever price below it, or the search stops being optimal.
pub const PLAN_COST: f64 = 0.3;
/// Cost swing per unit of current alignment (the current index is
/// calibrated so a typical world's p95 open-ocean speed ≈ 1.0; gyre
/// limbs run well past it and saturate the admissibility clamp).
pub const SAIL_CURRENT_GAIN: f64 = 0.55;
/// Cost swing per unit of zonal wind-stress alignment (|τ| ≤ 1.0).
pub const SAIL_WIND_GAIN: f64 = 0.25;

/// M50 — the current-strength ladder the metamorphic harness prices a
/// fixed lane against: 0 = a dead ocean, 1 = this world's own gyres,
/// 2 = twice the flow. A lane's favourable passage must fall and its
/// adverse passage rise, strictly, all the way up the ladder.
pub const META_CURRENT_LADDER: [f64; 5] = [0.0, 0.5, 1.0, 1.5, 2.0];
/// How many sea lanes the harness prices per seed (the longest ones).
pub const META_LANES: usize = 24;
/// The physics floor on the discount side — a following sea can at
/// most double a ship's effective speed. Admissibility is enforced
/// separately and exactly: each cell's multiplier is also clamped to
/// `PLAN_COST / cost`, so the fastest water in the world never prices
/// a step under what the heuristic planned (mixed coast/open blocks
/// can sit near 0.45, where a fixed global floor would undercut it).
pub const SAIL_MULT_FLOOR: f64 = 0.5;
/// The ceiling on the dear side: beating dead upwind against a gyre
/// limb — historically the passage nobody sailed; they waited for the
/// season or went the long way round, which is exactly what a high
/// ceiling makes A* do.
pub const SAIL_MULT_CEIL: f64 = 2.6;
/// |wind stress| below this marks a becalmed row — the doldrum band
/// where the Sverdrup source flips sign and the gyres converge.
pub const DOLDRUM_TAU: f64 = 0.12;
/// Additive surcharge on becalmed open water, both directions: ships
/// drift, whistle for wind, and pay for the waiting days.
pub const DOLDRUM_PENALTY: f64 = 0.3;

// M48 — the sailor's calendar: where the land's rain leans hard into
// one half of the year (the monsoon lands, |pamp| high), the seas they
// breathe over reverse their winds with the season. A lane on such
// water carries a double-frequency year — both monsoon heights move
// cargo, the turns of the wind becalm it — and the wet monsoon's burst
// months shut the water outright, the way pack ice already shuts a
// winter strait: nobody sails into the gale; they wait for the season.
/// Along-coast smoothing of the land's monsoon lean before it walks
/// out to sea (grid cells).
pub const MONSOON_SIGMA: f64 = 4.0;
/// The monsoon wind carries full strength this far offshore (cells)...
pub const MONSOON_NEAR: f64 = 8.0;
/// ...and has died entirely by here: far blue water answers to the
/// gyres and the zonal bands, not to any coast's seasons (cells).
pub const MONSOON_REACH: f64 = 24.0;
/// |sea monsoon| above which the wet season's height is a gale season:
/// the burst months close the water under it.
pub const MONSOON_GALE: f64 = 0.40;
/// COS12 alignment with the wet peak marking a burst month — 0.8 keeps
/// the arc at three months around the height, hemisphere-true.
pub const MONSOON_BURST: f64 = 0.8;
/// Pro-rata annualized surcharge on gale-season water — the ice law's
/// gentler sibling: a burst season is waited out, never iced in.
pub const MONSOON_GALE_SURCHARGE: f64 = 0.5;
/// Throughput swing per unit of lane exposure, double frequency: the
/// two monsoon heights carry, the two turns of the wind lull.
pub const MONSOON_TRADE_GAIN: f64 = 0.55;
/// |route season| above which a lane sails the monsoon calendar.
pub const MONSOON_LANE: f64 = 0.12;

/// M48 — why a lane's water shuts, unified: the ice's winter arc and
/// the monsoon's burst arc are one kind of fact — a month mask with a
/// name — and any future season (storm coasts, flood closures) joins
/// this enum instead of growing another ad-hoc field on `Route`.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SeasonalClosure {
    /// M37 — pack ice: bit m set = frozen shut in calendar month m.
    #[serde(rename = "ice")]
    Ice(u16),
    /// M48 — the wet monsoon's burst months: the gale season.
    #[serde(rename = "monsoon")]
    Monsoon(u16),
}

impl SeasonalClosure {
    pub fn months(&self) -> u16 {
        match self {
            SeasonalClosure::Ice(m) | SeasonalClosure::Monsoon(m) => *m,
        }
    }
}


#[derive(Serialize, Clone)]
pub struct Route {
    pub a: SettlementId,
    pub b: SettlementId,
    pub path: Vec<[i64; 2]>,
    /// travel mode per path point: 0 land, 1 sea, 2 river
    pub m: Vec<u8>,
    pub w: f64,
    /// total terrain cost of the journey
    pub cost: f64,
    /// fraction of the way spent under sail
    pub sea: f64,
    /// signed seasonal swing of the barge legs: high water lifts trade
    pub ramp: f64,
    pub goods: Vec<Option<Good>>,
    /// Disused (M9.4): years without realized flow let the grass grow —
    /// the road stays on the map, drawn faded, a mark history left.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub old: bool,
    /// The union of every seasonal closure below — the one mask the
    /// ledger reads (M37 ice ∪ M48 monsoon burst): bit m set = no
    /// cargo moves in calendar month m.
    #[serde(skip_serializing_if = "u16_is_zero", default)]
    pub closed: u16,
    /// M48 — the closures by name: which season shuts the water, and
    /// when. Empty for lanes that sail all year.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub shut: Vec<SeasonalClosure>,
    /// M48 — signed monsoon exposure of the sailed legs (round2): the
    /// double-frequency throughput season. 0 off the monsoon seas.
    #[serde(skip_serializing_if = "f64_is_zero", default)]
    pub season: f64,
}

fn u16_is_zero(v: &u16) -> bool {
    *v == 0
}

fn f64_is_zero(v: &f64) -> bool {
    *v == 0.0
}


/// Work out what a settlement produces from its hinterland. Also stamps
/// the M20 quarry tag — the stone its province cuts — so every path that
/// assigns goods keeps the tag in step with the rock under the town.
pub fn goods_for(
    s: &mut Settlement,
    deposits: &[Deposit],
    fertility: &Array2<f32>,
    rock: &Array2<u8>,
) {
    s.quarry = crate::rock::quarry(rock[[s.y as usize, s.x as usize]]);
    let r = crate::settlements::work_radius(s.pop);
    let r2 = r * r;
    let mut near: Vec<&Deposit> = deposits
        .iter()
        .filter(|d| {
            if !d.live() {
                return false; // unfound seams, dead pits and stripped woods yield nothing
            }
            let dx = (d.x - s.x) as f64;
            let dy = (d.y - s.y) as f64;
            dx * dx + dy * dy <= r2
        })
        .collect();
    near.sort_by(|a, b| {
        let ka = a.rich * rarity_w(a.r.abundance());
        let kb = b.rich * rarity_w(b.r.abundance());
        kb.partial_cmp(&ka).unwrap()
    });
    let mut goods: Goods = Goods::new();
    for d in near {
        if !goods.contains(&d.r) {
            goods.push(d.r);
        }
    }
    // M14.3 — animal secondaries: the beasts on the hoof carry a second
    // harvest. Sheep country shears wool; cattle and the hunted game
    // yield hides. Derived, never placed — the flock is the deposit — so
    // they ride behind their animals and drop first when the list fills.
    if goods.contains(&Good::Sheep) && !goods.contains(&Good::Wool) {
        goods.push(Good::Wool);
    }
    if (goods.contains(&Good::Cattle)
        || goods.contains(&Good::Deer)
        || goods.contains(&Good::Elk))
        && !goods.contains(&Good::Hides)
    {
        goods.push(Good::Hides);
    }
    let fert = fertility[[s.y as usize, s.x as usize]];
    if fert > 0.45 && !goods.contains(&Good::Grain) {
        let pos = if fert > 0.7 { 0 } else { 1.min(goods.len()) };
        goods.insert(pos, Good::Grain);
    }
    if goods.is_empty() {
        goods.push(if s.coastal { Good::Fish } else { Good::Grain });
    }
    goods.truncate(6);
    s.exports = Some(goods[0]);
    s.goods = goods;
}

pub fn assign_goods(
    settlements: &mut [Settlement],
    deposits: &[Deposit],
    fertility: &Array2<f32>,
    rock: &Array2<u8>,
) {
    for s in settlements.iter_mut() {
        goods_for(s, deposits, fertility, rock);
    }
}

// --- the price of the land ------------------------------------------------

fn smoothstep(x: f64, lo: f64, hi: f64) -> f64 {
    let t = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The downsampled world the caravans plan over: what each step costs,
/// which cells are open water (so embarking can be charged), and the
/// months that water is icebound (M37).
///
/// Clone so the M50 metamorphic harness can price the same lane over a
/// grid whose currents have been scaled — the perturbation never
/// touches the world's own grid.
#[derive(Clone)]
pub struct TradeGrid {
    pub cost: Array2<f64>,
    pub sea: Array2<bool>,
    /// M37 — full-resolution sea-ice month mask (`seaice::frozen_months`):
    /// routes read their icebound season off the same grid A* priced.
    pub frozen: Array2<u16>,
    /// M48 — full-resolution signed sea-monsoon lean: the coastal
    /// land's `pamp`, smoothed along the shore and carried offshore on
    /// a window that dies by `MONSOON_REACH`. Routes read their
    /// sailing calendar off the same grid A* priced.
    pub mons: Array2<f32>,
    /// M45 — best harbor shelter per coarse cell (block max of the
    /// settlements::shelter_score field): a harbor is a point asset, so
    /// the mean would wash it out. Discounts the embarkation toll.
    pub shelter: Array2<f64>,
    /// M46 — mean surface current per coarse cell (u grid-eastward,
    /// v grid-southward), solved by `currents::compute` on this grid's
    /// own sea mask: the routes sail the very ocean that was priced.
    pub cu: Array2<f64>,
    pub cv: Array2<f64>,
    /// M46 — coarse blue-water mask (majority of the block is open sea
    /// beyond cabotage's sight of land): only these feel the sail law.
    pub open: Array2<bool>,
    /// M46 — signed zonal wind stress per coarse row (+ = eastward).
    pub wind: Vec<f64>,
    /// M46 — becalmed convergence rows: |wind| under `DOLDRUM_TAU`.
    pub becalmed: Vec<bool>,
    pub f: usize,
}

impl TradeGrid {
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        height: &Array2<f32>,
        flags: &Array2<u8>,
        biomes: &Array2<u8>,
        discharge: &Array2<f32>,
        tmean: &Array2<f32>,
        tamp: &Array2<f32>,
        shelter: &Array2<f32>,
        pamp: &Array2<f32>,
        f: usize,
    ) -> TradeGrid {
        let (rows, cols) = height.dim();
        let sea_mask = height.mapv(|h| h < 0.0);
        // distance (on water) to the nearest shore — cabotage vs blue water
        let shore_d = ndimage::distance_transform_edt(&sea_mask);
        let hpos = height.mapv(|h| h.max(0.0) as f64);
        let (gy, gx) = ndimage::gradient(&hpos);
        // M37 — where and when the sea freezes over
        let frozen = crate::seaice::frozen_months(height, tmean, tamp);

        // M48 — the sea the monsoon land breathes over: the land's
        // signed rain-lean, smoothed along the coast, walked offshore
        // on a window that dies by MONSOON_REACH. Normalizing by the
        // smoothed land mask keeps coastal amplitude the land's own;
        // the window keeps far blue water out of a calendar it never
        // feels.
        let mut pw = Array2::<f64>::zeros((rows, cols));
        let mut lw = Array2::<f64>::zeros((rows, cols));
        for y in 0..rows {
            for x in 0..cols {
                if !sea_mask[[y, x]] {
                    pw[[y, x]] = pamp[[y, x]] as f64;
                    lw[[y, x]] = 1.0;
                }
            }
        }
        let num = ndimage::gaussian_filter(&pw, MONSOON_SIGMA);
        let den = ndimage::gaussian_filter(&lw, MONSOON_SIGMA);
        let mons = Array2::from_shape_fn((rows, cols), |(y, x)| {
            if !sea_mask[[y, x]] || den[[y, x]] < 1e-6 {
                return 0.0f32;
            }
            let fade = 1.0 - smoothstep(shore_d[[y, x]], MONSOON_NEAR, MONSOON_REACH);
            ((num[[y, x]] / den[[y, x]]) * fade) as f32
        });

        let full = Array2::from_shape_fn((rows, cols), |(y, x)| {
            let h = height[[y, x]] as f64;
            if h < 0.0 {
                // M37 — perennial pack is no lane at all; a strait that
                // freezes part of the year charges its closed season up
                // front, pro rata — the annualized price of sometimes.
                let iced = frozen[[y, x]].count_ones();
                if iced >= 12 {
                    return crate::seaice::PACK_SEA_COST;
                }
                let base = if shore_d[[y, x]] <= 6.0 {
                    COAST_SEA_COST
                } else {
                    OPEN_SEA_COST
                };
                // M48 — gale-season water charges its burst months the
                // same pro-rata way, at a gentler rate: the season is
                // waited out in harbour, never iced in.
                let burst = monsoon_burst_mask(mons[[y, x]]).count_ones();
                return base
                    * (1.0 + crate::seaice::ICE_LANE_SURCHARGE * iced as f64 / 12.0)
                    * (1.0 + MONSOON_GALE_SURCHARGE * burst as f64 / 12.0);
            }
            if flags[[y, x]] & CellFlags::LAKE.bits() != 0 {
                return LAKE_COST;
            }
            let slope = gy[[y, x]].hypot(gx[[y, x]]) * rows as f64 / 8.0;
            let mut cost = LAND_BASE + 14.0 * slope.clamp(0.0, 1.2);
            // thin air, scree and snowline above the tree country
            cost += 4.0 * smoothstep(h, 0.42, 0.8);
            let b = biomes[[y, x]];
            cost += 3.0 * ((b == gc::DESERT) as u8 as f64)
                + 9.0 * ((b == gc::ICE) as u8 as f64)
                + 1.5 * ((b == gc::TUNDRA) as u8 as f64)
                // M38 — summer mire: the wet tundra walks worse than dry heath
                + 2.0 * ((b == gc::WET_TUNDRA) as u8 as f64)
                + 2.5 * ((b == gc::TROPICAL_RAIN_FOREST) as u8 as f64)
                + 0.8 * ((b == gc::WOODLAND
                    || b == gc::SEASONAL_RAIN_FOREST
                    || b == gc::TEMPERATE_RAIN_FOREST
                    || b == gc::BOREAL_FOREST) as u8 as f64);
            if flags[[y, x]] & CellFlags::RIVER.bits() != 0 {
                if discharge[[y, x]] as f64 > NAVIGABLE {
                    // a broad river carries barges: the cheapest road inland
                    cost = cost.min(RIVER_BARGE_COST);
                } else {
                    cost += 1.8; // wet boots and lost cargo at the ford
                }
            }
            cost
        });

        let cost = downsample(&full, f);
        let s = rows / f;
        let sc = cols / f;
        let sea = Array2::from_shape_fn((s, sc), |(y, x)| {
            let mut n = 0usize;
            for dy in 0..f {
                for dx in 0..f {
                    if sea_mask[[y * f + dy, x * f + dx]] {
                        n += 1;
                    }
                }
            }
            2 * n > f * f
        });
        // M45 — the block's best anchorage, not its average shore
        let shel = Array2::from_shape_fn((s, sc), |(y, x)| {
            let mut m = 0.0f64;
            for dy in 0..f {
                for dx in 0..f {
                    let v = shelter[[y * f + dy, x * f + dx]] as f64;
                    if v > m {
                        m = v;
                    }
                }
            }
            m
        });
        // M46 — the ocean that carries the ships: gyres solved on the
        // same sea mask the costs were priced from, then read per
        // coarse cell as the block-mean surface current (land cells
        // hold zero current, so shore blocks dilute honestly).
        let cur = crate::currents::Currents::compute(&sea_mask);
        let cu = downsample(&cur.u.mapv(|v| v as f64), f);
        let cv = downsample(&cur.v.mapv(|v| v as f64), f);
        let open = Array2::from_shape_fn((s, sc), |(y, x)| {
            let mut n = 0usize;
            for dy in 0..f {
                for dx in 0..f {
                    let (yy, xx) = (y * f + dy, x * f + dx);
                    if sea_mask[[yy, xx]] && shore_d[[yy, xx]] > 6.0 {
                        n += 1;
                    }
                }
            }
            2 * n > f * f
        });
        let mut wind = Vec::with_capacity(s);
        let mut becalmed = Vec::with_capacity(s);
        for y in 0..s {
            let lat = -90.0 + (y * f + f / 2) as f64 * 180.0 / (rows as f64 - 1.0);
            let w = crate::currents::wind_stress(lat.abs());
            wind.push(w);
            becalmed.push(w.abs() < DOLDRUM_TAU);
        }
        TradeGrid { cost, sea, frozen, mons, shelter: shel, cu, cv, open, wind, becalmed, f }
    }
}

pub fn downsample(a: &Array2<f64>, f: usize) -> Array2<f64> {
    let (rows, cols) = a.dim();
    let s = rows / f;
    let sc = cols / f;
    Array2::from_shape_fn((s, sc), |(y, x)| {
        let mut sum = 0.0;
        for dy in 0..f {
            for dx in 0..f {
                sum += a[[y * f + dy, x * f + dx]];
            }
        }
        sum / (f * f) as f64
    })
}

// ------------------------------------------------ the directed sea (M46)

/// M48 — the burst months of one sea cell: the wet monsoon's height,
/// three months around the peak through the exact COS12 table,
/// hemisphere-true via the sign of the lean; 0 below the gale
/// threshold.
pub fn monsoon_burst_mask(mons: f32) -> u16 {
    if (mons as f64).abs() < MONSOON_GALE {
        return 0;
    }
    let mut m = 0u16;
    for (i, c) in crate::climate::COS12.iter().enumerate() {
        let aligned = if mons > 0.0 { *c } else { -*c };
        if aligned >= MONSOON_BURST {
            m |= 1 << i;
        }
    }
    m
}

/// M48 — a monsoon lane's monthly throughput multiplier: double
/// frequency through the exact COS12 table — both monsoon heights
/// carry cargo, the two turns of the wind becalm it. One law, two
/// readers: the economy's ledger and the diagnostics that grade it.
pub fn season_mult(season: f64, month: i64) -> f64 {
    if season == 0.0 {
        return 1.0;
    }
    let m2 = (2 * month.rem_euclid(12)) as usize % 12;
    (1.0 + MONSOON_TRADE_GAIN * season.abs() * crate::climate::COS12[m2]).max(0.4)
}



/// The directional price of one coarse cell for a step in unit
/// direction (dxu, dyu): 1.0 everywhere except open water, where the
/// current's alignment and the zonal wind buy or cost passage, the
/// becalmed convergence rows surcharge every heading, and the clamp
/// keeps the heuristic admissible and the routing graph conditioned.
pub fn sail_mult(grid: &TradeGrid, y: usize, x: usize, dxu: f64, dyu: f64) -> f64 {
    if !grid.open[[y, x]] {
        return 1.0;
    }
    let align_c = grid.cu[[y, x]] * dxu + grid.cv[[y, x]] * dyu;
    let align_w = grid.wind[y] * dxu;
    let mut m = 1.0 - SAIL_CURRENT_GAIN * align_c - SAIL_WIND_GAIN * align_w;
    if grid.becalmed[y] {
        // becalmed water never discounts: drift days come off no ledger
        m = m.max(1.0) + DOLDRUM_PENALTY;
    }
    // The exact admissibility clamp: this cell, under this multiplier,
    // must never price a step below the heuristic's plan.
    let floor = SAIL_MULT_FLOOR.max(PLAN_COST / grid.cost[[y, x]]);
    m.clamp(floor, SAIL_MULT_CEIL)
}

/// Price one directed step between neighbouring coarse cells — the one
/// law A* searches with and `path_cost` re-walks: half of each cell's
/// terrain cost under its own sail multiplier, plus the harbour toll
/// where cargo changes from wheel to keel.
pub fn edge_cost(
    grid: &TradeGrid,
    (y, x): (usize, usize),
    (ny, nx): (usize, usize),
    dist: f64,
) -> f64 {
    let dxu = (nx as f64 - x as f64) / dist;
    let dyu = (ny as f64 - y as f64) / dist;
    let mut c = dist
        * 0.5
        * (grid.cost[[y, x]] * sail_mult(grid, y, x, dxu, dyu)
            + grid.cost[[ny, nx]] * sail_mult(grid, ny, nx, dxu, dyu));
    if grid.sea[[y, x]] != grid.sea[[ny, nx]] {
        // cargo changes from wheel to keel — and the toll answers
        // the anchorage (M45): a sheltered harbour pays half dues;
        // an open beach has no quay at all — cargo lighters out
        // through the surf on weather windows, and historically
        // that cost as much as hundreds of kilometres of sea
        // carriage, so the toll grows quadratically with exposure
        // until a long cart road to a real harbour is the cheaper
        // voyage and mid-range beach-to-beach hauls go overland.
        let (ly, lx) = if grid.sea[[y, x]] { (ny, nx) } else { (y, x) };
        let open = 1.0 - grid.shelter[[ly, lx]];
        c += EMBARK_COST * (0.5 + 12.0 * open * open);
    }
    c
}

/// Walk a finished path and price it in one direction (M46). `rev`
/// prices the homeward passage — the same water, the currents now
/// against the keel. Forward equals the cost A* returned, exactly.
pub fn path_cost(grid: &TradeGrid, path: &[(usize, usize)], rev: bool) -> f64 {
    let n = path.len();
    let mut total = 0.0;
    for i in 1..n {
        let (a, b) = if rev {
            (path[n - i], path[n - 1 - i])
        } else {
            (path[i - 1], path[i])
        };
        let diag = a.0 != b.0 && a.1 != b.1;
        let dist = if diag { 1.4142135 } else { 1.0 };
        total += edge_cost(grid, a, b, dist);
    }
    total
}

/// A route is priced at the round trip (M46): the ship that rides the
/// gyre out fights it home, so the ledger charges the mean of the two
/// passages. Land routes are unchanged to the bit — carts feel no
/// current — while a lane through the doldrums pays its waiting days
/// on both legs.
pub fn round_trip(grid: &TradeGrid, path: &[(usize, usize)], fwd: f64) -> f64 {
    0.5 * (fwd + path_cost(grid, path, true))
}

/// Min-heap item ordered like Python's heapq tuples (f, g, y, x).
struct AItem(f64, f64, usize, usize);

impl PartialEq for AItem {
    fn eq(&self, o: &Self) -> bool {
        self.0 == o.0 && self.1 == o.1 && self.2 == o.2 && self.3 == o.3
    }
}
impl Eq for AItem {}
impl PartialOrd for AItem {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for AItem {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0
            .partial_cmp(&self.0)
            .unwrap()
            .then_with(|| o.1.partial_cmp(&self.1).unwrap())
            .then_with(|| o.2.cmp(&self.2))
            .then_with(|| o.3.cmp(&self.3))
    }
}

/// A* scratch space, pooled across calls (E5.4). `seen` carries a
/// generation stamp per cell: bumping `stamp` invalidates the whole grid
/// in O(1), so the O(N²) route builds stop allocating a fresh `Array2` +
/// `HashMap` per pair. Contents never outlive their stamp, so pooling is
/// invisible to the search — same expansions, same paths.
#[derive(Default)]
struct AstarScratch {
    stamp: u32,
    seen: Vec<u32>,
    best: Vec<f64>,
    /// packed predecessor cell index; u32::MAX = path start
    came: Vec<u32>,
    heap: BinaryHeap<AItem>,
}

impl AstarScratch {
    fn reset(&mut self, n: usize) {
        if self.seen.len() != n {
            self.seen = vec![0; n];
            self.best = vec![f64::INFINITY; n];
            self.came = vec![u32::MAX; n];
            self.stamp = 1;
        } else {
            self.stamp = self.stamp.wrapping_add(1);
            if self.stamp == 0 {
                self.seen.fill(0);
                self.stamp = 1;
            }
        }
        self.heap.clear();
    }

    #[inline]
    fn best(&self, i: usize) -> f64 {
        if self.seen[i] == self.stamp {
            self.best[i]
        } else {
            f64::INFINITY
        }
    }

    #[inline]
    fn relax(&mut self, i: usize, g: f64, from: u32) {
        self.seen[i] = self.stamp;
        self.best[i] = g;
        self.came[i] = from;
    }
}

thread_local! {
    static ASTAR_SCRATCH: std::cell::RefCell<AstarScratch> =
        std::cell::RefCell::new(AstarScratch::default());
}

/// A* over the trade grid. Crossing the shoreline pays the harbour fee,
/// so short hops stay ashore and long hauls take ship. Returns the path
/// and its total cost.
pub fn astar(
    grid: &TradeGrid,
    start: (usize, usize),
    goal: (usize, usize),
) -> Option<(Vec<(usize, usize)>, f64)> {
    ASTAR_SCRATCH.with(|sc| astar_with(&mut sc.borrow_mut(), grid, start, goal))
}

fn astar_with(
    sc: &mut AstarScratch,
    grid: &TradeGrid,
    start: (usize, usize),
    goal: (usize, usize),
) -> Option<(Vec<(usize, usize)>, f64)> {
    let cost = &grid.cost;
    let (hh, ww) = cost.dim();
    let (sy, sx) = start;
    let (gy, gx) = goal;
    let min_cost = PLAN_COST;
    let max_expand = 200_000usize;
    sc.reset(hh * ww);
    sc.relax(sy * ww + sx, 0.0, u32::MAX);
    let h0 = min_cost * ((gy as f64 - sy as f64).hypot(gx as f64 - sx as f64));
    sc.heap.push(AItem(h0, 0.0, sy, sx));
    let mut expanded = 0usize;

    while let Some(AItem(_, g, y, x)) = sc.heap.pop() {
        if g > sc.best(y * ww + x) {
            continue;
        }
        if y == gy && x == gx {
            let mut path = vec![(y, x)];
            let mut ci = y * ww + x;
            while sc.came[ci] != u32::MAX {
                ci = sc.came[ci] as usize;
                path.push((ci / ww, ci % ww));
            }
            path.reverse();
            return Some((path, g));
        }
        expanded += 1;
        if expanded > max_expand {
            return None;
        }
        for (&(dy, dx), &dist) in N8.iter().zip(DIST.iter()) {
            let ny = y as isize + dy;
            let nx = x as isize + dx;
            if ny < 0 || nx < 0 || ny >= hh as isize || nx >= ww as isize {
                continue;
            }
            let (ny, nx) = (ny as usize, nx as usize);
            // one law for search and re-walk alike (M46): terrain cost
            // under the sail multiplier, plus the M45 harbour toll.
            let ng = g + edge_cost(grid, (y, x), (ny, nx), dist);
            let ni = ny * ww + nx;
            if ng < sc.best(ni) {
                sc.relax(ni, ng, (y * ww + x) as u32);
                let f = ng + min_cost * ((gy as f64 - ny as f64).hypot(gx as f64 - nx as f64));
                sc.heap.push(AItem(f, ng, ny, nx));
            }
        }
    }
    None
}

/// Classify one full-resolution point: under sail, on a barge, or afoot.
fn point_mode(
    x: i64,
    y: i64,
    height: &Array2<f32>,
    flags: &Array2<u8>,
    discharge: &Array2<f32>,
) -> u8 {
    let (hh, ww) = height.dim();
    if x < 0 || y < 0 || x >= ww as i64 || y >= hh as i64 {
        return MODE_LAND;
    }
    if height[[y as usize, x as usize]] < 0.0 {
        return MODE_SEA;
    }
    for dy in -1i64..=1 {
        for dx in -1i64..=1 {
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0 || ny < 0 || nx >= ww as i64 || ny >= hh as i64 {
                continue;
            }
            let (nxu, nyu) = (nx as usize, ny as usize);
            if flags[[nyu, nxu]] & CellFlags::RIVER.bits() != 0 && discharge[[nyu, nxu]] as f64 > NAVIGABLE {
                return MODE_RIVER;
            }
        }
    }
    MODE_LAND
}

#[allow(clippy::too_many_arguments)]
pub fn route_entry(
    sa: &Settlement,
    sb: &Settlement,
    path: &[(usize, usize)],
    f: usize,
    total_cost: f64,
    height: &Array2<f32>,
    flags: &Array2<u8>,
    discharge: &Array2<f32>,
    flow_amp: &Array2<f32>,
    frozen: &Array2<u16>,
    mons: &Array2<f32>,
) -> Route {
    let mut pts: Vec<[i64; 2]> = path
        .iter()
        .map(|&(y, x)| [(x * f + f / 2) as i64, (y * f + f / 2) as i64])
        .collect();
    pts[0] = [sa.x, sa.y];
    let last = pts.len() - 1;
    pts[last] = [sb.x, sb.y];
    if pts.len() > 3 {
        let mut thinned = vec![pts[0]];
        let mut i = 1;
        while i < pts.len() - 1 {
            thinned.push(pts[i]);
            i += 2;
        }
        thinned.push(pts[pts.len() - 1]);
        pts = thinned;
    }
    let n = pts.len();
    let mut modes: Vec<u8> = pts
        .iter()
        .map(|p| point_mode(p[0], p[1], height, flags, discharge))
        .collect();
    modes[0] = MODE_LAND; // journeys begin and end at the town gate
    modes[n - 1] = MODE_LAND;
    let sea_frac = modes.iter().filter(|&&m| m == MODE_SEA).count() as f64 / n as f64;

    // The barge legs remember their river's seasons: a route that rides
    // monsoon water swells and shrinks with it.
    let (hh, ww) = flow_amp.dim();
    let mut amp_sum = 0.0f64;
    let mut nriv = 0usize;
    for (p, &m) in pts.iter().zip(modes.iter()) {
        if m == MODE_RIVER
            && p[0] >= 0
            && p[1] >= 0
            && (p[0] as usize) < ww
            && (p[1] as usize) < hh
        {
            amp_sum += flow_amp[[p[1] as usize, p[0] as usize]] as f64;
            nriv += 1;
        }
    }
    let ramp = if nriv > 0 {
        crate::util::round2(amp_sum / nriv as f64 * (nriv as f64 / n as f64))
    } else {
        0.0
    };

    // M37 — the ice writes the schedule: any frozen water on the sailed
    // leg closes the whole journey for those months.
    let mut ice: u16 = 0;
    for (p, &m) in pts.iter().zip(modes.iter()) {
        if m == MODE_SEA
            && p[0] >= 0
            && p[1] >= 0
            && (p[0] as usize) < ww
            && (p[1] as usize) < hh
        {
            ice |= frozen[[p[1] as usize, p[0] as usize]];
        }
    }
    ice &= crate::seaice::MONTHS_MASK;

    // M48 — the monsoon writes the rest: the sailed legs' mean lean
    // sets the lane's double-frequency season (diluted by the share of
    // the journey under sail, exactly the barge ramp's law), and any
    // gale-season water on the way closes the whole journey for its
    // burst months, exactly the ice's law.
    let mut mn_sum = 0.0f64;
    let mut nsea = 0usize;
    let mut monsoon: u16 = 0;
    for (p, &m) in pts.iter().zip(modes.iter()) {
        if m == MODE_SEA
            && p[0] >= 0
            && p[1] >= 0
            && (p[0] as usize) < ww
            && (p[1] as usize) < hh
        {
            let v = mons[[p[1] as usize, p[0] as usize]];
            mn_sum += v as f64;
            nsea += 1;
            monsoon |= monsoon_burst_mask(v);
        }
    }
    monsoon &= crate::seaice::MONTHS_MASK;
    let season = if nsea > 0 {
        crate::util::round2(mn_sum / nsea as f64 * (nsea as f64 / n as f64))
    } else {
        0.0
    };
    let mut shut = Vec::new();
    if ice != 0 {
        shut.push(SeasonalClosure::Ice(ice));
    }
    if monsoon != 0 {
        shut.push(SeasonalClosure::Monsoon(monsoon));
    }

    let base_w = (0.5 + (((sa.pop + sb.pop) as f64).log10() - 2.0) * 0.6).clamp(0.5, 2.0);
    // the sea multiplies what a route can carry
    let w = crate::util::round2((base_w * (0.8 + 0.55 * sea_frac)).clamp(0.4, 2.4));
    Route {
        a: sa.id,
        b: sb.id,
        path: pts,
        m: modes,
        w,
        cost: crate::util::round2(total_cost),
        sea: crate::util::round2(sea_frac),
        ramp,
        goods: vec![sa.exports.clone(), sb.exports.clone()],
        old: false,
        closed: ice | monsoon,
        shut,
        season,
    }
}

pub fn recount_connections(settlements: &mut [Settlement], routes: &[Route]) {
    let mut conn: HashMap<SettlementId, i64> = settlements.iter().map(|s| (s.id, 0)).collect();
    for r in routes {
        *conn.entry(r.a).or_insert(0) += 1;
        *conn.entry(r.b).or_insert(0) += 1;
    }
    for s in settlements.iter_mut() {
        s.connections = *conn.get(&s.id).unwrap_or(&0);
    }
}

/// A harbour is where a settlement's trade takes to the water: coastal,
/// with at least one route whose *first or last step* goes under sail —
/// embarkation inside the town's own coarse cell, its literal gates
/// (M45). A town whose cargo carts up the coast to someone else's quay
/// is a customer, not a port; the old blanket "a mostly-sea route makes
/// harbours of both ends" was adjacency-alone in disguise — it flagged
/// beach towns whose lighters worked someone else's roadstead — and the
/// pathfinder's lighterage pricing now decides where sail begins.
pub fn mark_ports(settlements: &mut [Settlement], routes: &[Route]) {
    let mut ports: BTreeSet<SettlementId> = BTreeSet::new();
    for r in routes {
        if r.sea < 0.05 {
            continue;
        }
        // m[0] and m[n-1] are pinned to LAND (the gate itself), so the
        // first honest reading is m[1]: the first coarse step out.
        let n = r.m.len();
        if n < 3 {
            continue;
        }
        if r.m[1] == MODE_SEA {
            ports.insert(r.a);
        }
        if r.m[n - 2] == MODE_SEA {
            ports.insert(r.b);
        }
    }
    for s in settlements.iter_mut() {
        s.port = s.coastal && ports.contains(&s.id);
    }
}

/// Is a journey worth making at all? Impassable country strands towns.
fn viable(total_cost: f64, start: (usize, usize), goal: (usize, usize)) -> bool {
    let d = (goal.0 as f64 - start.0 as f64).hypot(goal.1 as f64 - start.1 as f64);
    total_cost <= 8.0 * d + 40.0
}

/// Candidate pairs: each settlement courts its 2 nearest neighbours, and
/// every *harbour* town also seeks a far harbour to trade with under
/// sail (M45): a shipping line runs roadstead to roadstead. A coastal
/// town below `SHELTER_ROADSTEAD` is a fishing village — it keeps its
/// two neighbour routes (which may still cross a strait and earn the
/// port flag honestly), but it does not found a blue-water line from an
/// open beach, and no far line terminates on one.
pub fn build_routes(
    grid: &TradeGrid,
    settlements: &mut [Settlement],
    height: &Array2<f32>,
    flags: &Array2<u8>,
    discharge: &Array2<f32>,
    flow_amp: &Array2<f32>,
    shelter: &Array2<f32>,
) -> Vec<Route> {
    let f = grid.f;
    let rows = grid.cost.dim().0 * f;
    let far2 = ((rows as f64) / 5.0).powi(2);
    let roadstead = |s: &Settlement| {
        s.coastal
            && shelter[[s.y as usize, s.x as usize]]
                >= crate::settlements::SHELTER_ROADSTEAD
    };
    let mut pairs: BTreeSet<(SettlementId, SettlementId)> = BTreeSet::new();
    for s in settlements.iter() {
        let mut others: Vec<&Settlement> =
            settlements.iter().filter(|o| o.id != s.id).collect();
        others.sort_by(|a, b| {
            let da = (a.x - s.x).pow(2) + (a.y - s.y).pow(2);
            let db = (b.x - s.x).pow(2) + (b.y - s.y).pow(2);
            da.cmp(&db)
        });
        for o in others.iter().take(2) {
            pairs.insert((s.id.min(o.id), s.id.max(o.id)));
        }
        // the sea link: the nearest fellow harbour town beyond the horizon
        if roadstead(s) {
            if let Some(o) = others.iter().find(|o| {
                roadstead(o)
                    && ((o.x - s.x).pow(2) + (o.y - s.y).pow(2)) as f64 > far2
            }) {
                pairs.insert((s.id.min(o.id), s.id.max(o.id)));
            }
        }
    }

    let by_id: HashMap<SettlementId, usize> = settlements
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id, i))
        .collect();
    let mut routes: Vec<Route> = Vec::new();
    for &(a, b) in pairs.iter() {
        let sa = &settlements[by_id[&a]];
        let sb = &settlements[by_id[&b]];
        let start = (sa.y as usize / f, sa.x as usize / f);
        let goal = (sb.y as usize / f, sb.x as usize / f);
        if let Some((path, cost)) = astar(grid, start, goal) {
            let cost = round_trip(grid, &path, cost);
            if viable(cost, start, goal) {
                routes.push(route_entry(sa, sb, &path, f, cost, height, flags, discharge, flow_amp, &grid.frozen, &grid.mons));
            }
        }
    }
    recount_connections(settlements, &routes);
    rescue_unconnected(settlements, &mut routes, grid, height, flags, discharge, flow_amp);
    bridge_components(settlements, &mut routes, grid, height, flags, discharge, flow_amp);
    routes
}

/// Any town the viability cap left stranded still gets one lifeline: the
/// cheapest path to a near neighbour, taken at whatever price the terrain
/// asks. Frontier mining camps in hungry country stay on the map of trade.
pub fn rescue_unconnected(
    settlements: &mut [Settlement],
    routes: &mut Vec<Route>,
    grid: &TradeGrid,
    height: &Array2<f32>,
    flags: &Array2<u8>,
    discharge: &Array2<f32>,
    flow_amp: &Array2<f32>,
) {
    let f = grid.f;
    let lonely: Vec<usize> = settlements
        .iter()
        .enumerate()
        .filter(|(_, s)| s.connections == 0)
        .map(|(i, _)| i)
        .collect();
    if lonely.is_empty() {
        return;
    }
    for idx in lonely {
        let s = settlements[idx].clone();
        let mut others: Vec<&Settlement> =
            settlements.iter().filter(|o| o.id != s.id).collect();
        others.sort_by(|a, b| {
            let da = (a.x - s.x).pow(2) + (a.y - s.y).pow(2);
            let db = (b.x - s.x).pow(2) + (b.y - s.y).pow(2);
            da.cmp(&db)
        });
        let mut best: Option<(Route, f64)> = None;
        for o in others.iter().take(6) {
            let start = (s.y as usize / f, s.x as usize / f);
            let goal = (o.y as usize / f, o.x as usize / f);
            if let Some((path, cost)) = astar(grid, start, goal) {
                let cost = round_trip(grid, &path, cost);
                if best.as_ref().map_or(true, |(_, c)| cost < *c) {
                    best = Some((
                        route_entry(&s, o, &path, f, cost, height, flags, discharge, flow_amp, &grid.frozen, &grid.mons),
                        cost,
                    ));
                }
            }
        }
        if let Some((r, _)) = best {
            routes.push(r);
        }
    }
    recount_connections(settlements, routes);
}

/// Link a newly founded settlement (at `idx`) into the route network.
#[allow(clippy::too_many_arguments)]
pub fn connect_settlement(
    idx: usize,
    settlements: &mut [Settlement],
    routes: &mut Vec<Route>,
    grid: &TradeGrid,
    height: &Array2<f32>,
    flags: &Array2<u8>,
    discharge: &Array2<f32>,
    flow_amp: &Array2<f32>,
) {
    let f = grid.f;
    let s = settlements[idx].clone();
    let mut others: Vec<&Settlement> = settlements.iter().filter(|o| o.id != s.id).collect();
    others.sort_by(|a, b| {
        let da = (a.x - s.x).pow(2) + (a.y - s.y).pow(2);
        let db = (b.x - s.x).pow(2) + (b.y - s.y).pow(2);
        da.cmp(&db)
    });
    let mut new_routes: Vec<Route> = Vec::new();
    for o in others.iter().take(2) {
        let start = (s.y as usize / f, s.x as usize / f);
        let goal = (o.y as usize / f, o.x as usize / f);
        if let Some((path, cost)) = astar(grid, start, goal) {
            let cost = round_trip(grid, &path, cost);
            if viable(cost, start, goal) {
                new_routes.push(route_entry(&s, o, &path, f, cost, height, flags, discharge, flow_amp, &grid.frozen, &grid.mons));
            }
        }
    }
    routes.extend(new_routes);
    recount_connections(settlements, routes);
    rescue_unconnected(settlements, routes, grid, height, flags, discharge, flow_amp);
    bridge_components(settlements, routes, grid, height, flags, discharge, flow_amp);
    mark_ports(settlements, routes);
}

/// One world, one web (M8.1). The loneliness rescue works at the level of
/// a single town's degree — it cannot see an archipelago pair that trades
/// only with itself, a component of two invisible to every degree count.
/// Union the route graph and, while more than one component stands, bridge
/// the smallest one to the largest by the cheapest lifeline crossing,
/// taken at whatever price the terrain asks.
#[allow(clippy::too_many_arguments)]
pub fn bridge_components(
    settlements: &mut [Settlement],
    routes: &mut Vec<Route>,
    grid: &TradeGrid,
    height: &Array2<f32>,
    flags: &Array2<u8>,
    discharge: &Array2<f32>,
    flow_amp: &Array2<f32>,
) {
    let n = settlements.len();
    if n < 2 {
        return;
    }
    let f = grid.f;
    let idx_of: HashMap<SettlementId, usize> = settlements
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id, i))
        .collect();

    fn find(uf: &mut Vec<usize>, mut i: usize) -> usize {
        while uf[i] != i {
            uf[i] = uf[uf[i]];
            i = uf[i];
        }
        i
    }

    let mut bridged = false;
    // Each pass joins two components; n passes always suffice.
    for _ in 0..n {
        let mut uf: Vec<usize> = (0..n).collect();
        for r in routes.iter() {
            if let (Some(&ia), Some(&ib)) = (idx_of.get(&r.a), idx_of.get(&r.b)) {
                let (ra, rb) = (find(&mut uf, ia), find(&mut uf, ib));
                if ra != rb {
                    uf[ra] = rb;
                }
            }
        }
        let mut comps: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for i in 0..n {
            let r = find(&mut uf, i);
            comps.entry(r).or_default().push(i);
        }
        if comps.len() <= 1 {
            break;
        }
        // Largest component is the mainland web; smallest is next to join.
        // BTreeMap order + (len, root) keys keep the choice deterministic.
        let main_root = *comps
            .iter()
            .max_by_key(|(root, m)| (m.len(), usize::MAX - **root))
            .map(|(root, _)| root)
            .unwrap();
        let minor_root = *comps
            .iter()
            .filter(|(root, _)| **root != main_root)
            .min_by_key(|(root, m)| (m.len(), **root))
            .map(|(root, _)| root)
            .unwrap();
        let main = &comps[&main_root];
        let minor = &comps[&minor_root];

        // For each stranded town, its nearest mainland partner; then court
        // the closest few crossings and keep the cheapest path that answers.
        let mut cands: Vec<(i64, usize, usize)> = minor
            .iter()
            .map(|&mi| {
                let s = &settlements[mi];
                let (bj, d2) = main
                    .iter()
                    .map(|&mj| {
                        let o = &settlements[mj];
                        (mj, (o.x - s.x).pow(2) + (o.y - s.y).pow(2))
                    })
                    .min_by_key(|&(mj, d2)| (d2, settlements[mj].id))
                    .unwrap();
                (d2, mi, bj)
            })
            .collect();
        cands.sort_by_key(|&(d2, a, b)| (d2, settlements[a].id, settlements[b].id));
        cands.truncate(6);

        let mut best: Option<(Route, f64)> = None;
        for &(_, mi, mj) in &cands {
            let s = &settlements[mi];
            let o = &settlements[mj];
            let start = (s.y as usize / f, s.x as usize / f);
            let goal = (o.y as usize / f, o.x as usize / f);
            if let Some((path, cost)) = astar(grid, start, goal) {
                let cost = round_trip(grid, &path, cost);
                if best.as_ref().map_or(true, |(_, c)| cost < *c) {
                    best = Some((
                        route_entry(s, o, &path, f, cost, height, flags, discharge, flow_amp, &grid.frozen, &grid.mons),
                        cost,
                    ));
                }
            }
        }
        match best {
            Some((r, _)) => {
                routes.push(r);
                bridged = true;
            }
            // No crossing answered at all — impassable by every measure;
            // leave the world as it is rather than loop forever.
            None => break,
        }
    }
    if bridged {
        recount_connections(settlements, routes);
    }
}

// ---------------------------------------------------------------- bands

/// Diagnostics bands (E11.6): the gravity of big close pairs, and the
/// directed sea (M46).
pub const BANDS: &[crate::util::Band] = &[
    crate::util::Band { name: "gravity-model correlation", sweet: (0.30, 1.0), hard: (0.10, 1.0), target: "M5.4 gate: big close pairs carry the trade" },
    // M46 edges are measured, not guessed: at gains 0.55/0.25 the three
    // probe seeds ran best mirror advantage 23.0/12.4/10.1% and alive
    // share 33/19/7% (12345/90210/777). Sweet's floor is the spec's own
    // 15% and the gate seed (12345) clears it with slack; hard admits an
    // honestly symmetric sea (777 has one far-sea hub and crossings
    // near-perpendicular to the gyres) at WARN, never FAIL — the open-
    // water fraction of a lane bounds its achievable asymmetry, and
    // cabotage plus harbour tolls are symmetric by design.
    crate::util::Band { name: "sea-lane mirror advantage (best)", sweet: (15.0, 60.0), hard: (8.0, 100.0), target: "M46 gate: the with-current passage sails ≥15% faster than its against-current mirror" },
    crate::util::Band { name: "directional lanes alive", sweet: (15.0, 100.0), hard: (5.0, 100.0), target: "M46: share of blue-water lanes (%) sailing ≥2% faster one way than the other" },
    // M48 bands are measured, not guessed: at gale 0.40 / gain 0.55 the
    // five probe seeds ran lane share 34/28/24/30/39% and swing mean
    // 74/82/92/81/89% (12345/777/90210/31337/555). Sweet brackets that
    // envelope with slack both ways; the swing floor stays the spec's
    // own 30% — closures dominate the mean (a shut lane swings 100%),
    // and a world whose monsoon lanes all stayed open would honestly
    // WARN here at gain 0.55 (open-water swing tops out near 20%).
    crate::util::Band { name: "seasonal sea-lane share", sweet: (10.0, 80.0), hard: (2.0, 100.0), target: "M49: share of sea-touching lanes (%) whose year is not flat — ice-shut, gale-shut or monsoon-leaning" },
    crate::util::Band { name: "sea-lane seasonality spread", sweet: (5.0, 100.0), hard: (1.0, 100.0), target: "M49: p90−p10 of per-lane throughput swing (pp) — the fleet's calendar must differ lane to lane, not move as one" },
    crate::util::Band { name: "monsoon lane share", sweet: (15.0, 55.0), hard: (5.0, 80.0), target: "M48: share of sea-touching lanes (%) sailing the monsoon calendar" },
    crate::util::Band { name: "monsoon throughput swing", sweet: (30.0, 100.0), hard: (15.0, 100.0), target: "M48 gate: monsoon-lane throughput swings ≥30% between the year's peak and its floor" },
];

// ------------------------------------------------------- M56 the caravan

/// M56 — how far a caravan can be victualled from a watered market, in
/// the same cost units A* prices land travel with. A desert crossing is
/// bounded by water and fodder, not by will: at `LAND_BASE` 3.0 plus the
/// desert's own 3.0 surcharge, 120 cost units buy roughly twenty coarse
/// cells of sand — about 320 km at 16 km per coarse cell, the working
/// range of a caravan that must reach a well or a market before its
/// water runs out. Beyond it the deep interior stays empty, which is why
/// the Sahara has a rim of oasis towns and a hollow middle.
pub const CARAVAN_BUDGET: f64 = 120.0;

/// M56 — the provisioning field: for every land cell, how well a caravan
/// out of the nearest watered market can supply it, 1.0 at the market's
/// own ground falling linearly to 0.0 at `CARAVAN_BUDGET` of land travel.
///
/// Dijkstra over the coarse trade cost grid, land only — a caravan does
/// not sail, and a dry site on the far side of a strait is not reachable
/// by camel. `markets` are fine-grid coordinates; the field is returned
/// at fine resolution so siting reads it per cell.
pub fn caravan_provision(
    grid: &TradeGrid,
    markets: &[(usize, usize)],
    fine: (usize, usize),
) -> Array2<f32> {
    let (hh, ww) = grid.cost.dim();
    let mut dist = vec![f64::INFINITY; hh * ww];
    let mut heap: BinaryHeap<AItem> = BinaryHeap::new();
    for &(fy, fx) in markets {
        let (y, x) = (fy / grid.f, fx / grid.f);
        if y >= hh || x >= ww || grid.sea[[y, x]] {
            continue;
        }
        if dist[y * ww + x] > 0.0 {
            dist[y * ww + x] = 0.0;
            heap.push(AItem(0.0, 0.0, y, x));
        }
    }
    while let Some(AItem(g, _, y, x)) = heap.pop() {
        if g > dist[y * ww + x] {
            continue;
        }
        if g >= CARAVAN_BUDGET {
            continue;
        }
        for (k, &(dy, dx)) in N8.iter().enumerate() {
            let (ny, nx) = (y as isize + dy, x as isize + dx);
            if ny < 0 || nx < 0 || ny >= hh as isize || nx >= ww as isize {
                continue;
            }
            let (ny, nx) = (ny as usize, nx as usize);
            if grid.sea[[ny, nx]] {
                continue; // the caravan walks; it does not swim
            }
            let step = DIST[k] * 0.5 * (grid.cost[[y, x]] + grid.cost[[ny, nx]]);
            let ng = g + step;
            if ng < dist[ny * ww + nx] && ng < CARAVAN_BUDGET {
                dist[ny * ww + nx] = ng;
                heap.push(AItem(ng, 0.0, ny, nx));
            }
        }
    }
    let (frows, fcols) = fine;
    Array2::from_shape_fn((frows, fcols), |(y, x)| {
        let (cy, cx) = ((y / grid.f).min(hh - 1), (x / grid.f).min(ww - 1));
        let d = dist[cy * ww + cx];
        if !d.is_finite() {
            0.0
        } else {
            (1.0 - d / CARAVAN_BUDGET).clamp(0.0, 1.0) as f32
        }
    })
}
