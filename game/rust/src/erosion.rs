//! Erosion — the carved land, sitting between rock and ice in the
//! fixed pipeline (rock → **soil's carving** → …, ADR-0026): it reads
//! the raw tectonic relief and hands the ice stage mountains with
//! faces. Three processes, run in the order nature runs them: thermal
//! collapse knocks the impossible spikes down to talus slopes, rivers
//! cut valleys with the stream power they actually carry, and soil
//! creep softens what remains. Everything is deterministic — no
//! randomness, pure functions of the heightfield — so the same seed
//! still carves the same world. Flow routing comes from `grid` (M66):
//! the same fill, the same descent, the same drainage sort hydrology
//! uses — one lattice law, not two copies of it.
//!
//! M59 — the sediment budget: what the river detaches it must also
//! carry, and what it cannot carry it must lay down. Every fluvial
//! pass keeps a ledger: detachment recorded cell by cell, load routed
//! down the same drainage tree the incision walked, deposition wherever
//! transport capacity drops — floodplains aggrade, lakes silt from the
//! inflow, and the load that reaches the sea builds delta fans on the
//! shelf. Whatever the shelf cannot hold slides past the fan front and
//! is ledgered abyssal. The books close exactly: detached = settled +
//! delta fill + abyssal, by construction, and `diagnose terrain` audits
//! the grid against the ledger to 1% (the M59 gate).

use ndarray::Array2;

use crate::grid::{accumulate, drainage_order, fill_depressions, flow_directions, DIST, N8};
use crate::util::{fnv1a64, Band};

/// Slopes steeper than this (height units per cell of run) shed rock.
/// One height unit is ~4 km over a ~4 km cell, so 0.05 ≈ 200 m of rise
/// per cell — about the steepest a mean 4 km tile can honestly hold.
const TALUS: f64 = 0.05;
/// Fraction of the excess relief that lets go per pass.
const TALUS_K: f64 = 0.5;
const TALUS_PASSES: usize = 3;

/// Stream-power constant: how hard a river of unit drainage area cuts.
const SPI_K: f64 = 0.014;
/// Implicit incision solves toward the receiver, so any K is stable;
/// two passes let the first valleys steer the second's drainage — and
/// since M59 the second pass also drains across the first pass's delta
/// plain, so the fans prograde instead of merely reappearing.
const SPI_PASSES: usize = 2;

/// Soil-creep diffusion strength and passes (land-only, coast-safe).
const DIFF_D: f64 = 0.12;
const DIFF_PASSES: usize = 2;

// ------------------------------------------------------- M59 constants

/// Transport capacity per cell: `CAP_K · A · S` in the manner of a
/// total-load law (q_s ∝ q·S with q ∝ A on a uniform-rain grid). Where
/// the profile flattens the capacity collapses and the excess drops.
const CAP_K: f64 = 2.0;
/// Fraction of the over-capacity excess that settles per cell — the
/// rest rides on, so floodplains aggrade over tens of cells instead of
/// dumping the whole surplus at the first flat.
const DEP_K: f64 = 0.25;
/// A settling cell may climb at most this fraction of the drop to its
/// receiver: deposition fills valleys, it never reverses a river.
const DEP_HEADROOM: f64 = 0.4;
/// Standing water (a filled depression) takes at most this fraction of
/// its remaining depth per visit — lakes silt from the inflow shoreward,
/// they don't vanish under one flood.
const LAKE_K: f64 = 0.35;
/// Delta plain top: fans fill the shelf to just above the tideline
/// (~12 m at 4 km height units) — low, wet, floodable ground.
const DELTA_TOP: f64 = 0.003;
/// Fans only build where the seabed is shelf, not abyss (~−220 m);
/// anything the fan front cannot hold slides past it and is ledgered.
const FAN_FLOOR: f64 = -0.055;
/// Fan reach in cells (Chebyshev) around the mouth: 5 cells ≈ a 44 km
/// wide delta at full spread — Nile scale, the largest the grid earns.
const FAN_R: isize = 5;
/// Loads below this never ledger a mouth: hillslope rills, not rivers.
const MOUTH_MIN: f64 = 1e-4;
/// Fluvial dominance (Galloway's triangle): a delta only forms where
/// the river beats the waves, and on this grid that is a drainage-area
/// bar in discharge units (which reduce to ~area in cells under
/// uniform rain). Below it the mouth's load disperses along the shore
/// and off the shelf: ledgered abyssal, no fan, no mouth row — the
/// M44 drift already owns shore-parallel sand.
///
/// M64 recalibrated this bar against Earth. The old value (60.0 =
/// RIVER_THRESHOLD — every rendered river a delta) minted 355–381
/// deltas per continent fronting 13–14% of the coast; Earth's whole
/// planet carries ~60–100 deltaic plains resolvable at this grid's
/// 4-km cells, fronting ~1–2% of coastline by length, and their
/// feeding basins run ≥~10⁴ km² (the floor of the global delta
/// censuses — Ericson 2006, Syvitski 2009; wave and tide reworking
/// disperses everything smaller). 10⁴ km² / 16 km² per cell = 625.
const DELTA_AREA_MIN: f64 = 625.0;
/// M59 — how hard fan silt shoals an anchorage: shelter divides by
/// `1 + SILT_SHOAL · depth` read off the deepest silt in the 5×5
/// anchorage window (world.rs applies it right after shelter_score).
pub const SILT_SHOAL: f32 = 40.0;

/// One ledgered river mouth: the sea cell the load arrived at, the
/// load delivered (height-volume units), the largest upstream drainage
/// area that fed it, and the fan cells its deltas filled.
#[derive(Clone, Debug)]
pub struct Mouth {
    pub y: u32,
    pub x: u32,
    pub load: f64,
    pub area: f64,
    pub fan: u32,
}

/// M59 — the closed books of the fluvial passes. Scalars are the
/// ledger; the grids are its footprint on the map. Frozen at the dawn
/// like the drift ledger (M44): widened, folded into `hash_state`,
/// never ticked.
#[derive(Clone, Debug)]
pub struct Sediment {
    /// Total height-volume the incision detached (Σ over both passes).
    pub detached: f64,
    /// Settled on land along the profile: floodplain + lake fill.
    pub settled: f64,
    /// Poured into delta fans at the sea mouths.
    pub delta_fill: f64,
    /// Carried past the fan front into deep water — leaves the map's
    /// accounting but never the ledger's.
    pub abyssal: f64,
    /// Every sea mouth that received ≥ MOUTH_MIN of load, ledger order
    /// (ascending flat index of the mouth cell at first registration).
    pub mouths: Vec<Mouth>,
    /// Net deposition depth per cell (floodplain + lake + fan fill).
    pub depth: Array2<f32>,
    /// Fan-built cells that ended above the tideline: new delta land.
    pub delta: Array2<bool>,
}

impl Sediment {
    fn empty(dim: (usize, usize)) -> Sediment {
        Sediment {
            detached: 0.0,
            settled: 0.0,
            delta_fill: 0.0,
            abyssal: 0.0,
            mouths: Vec::new(),
            depth: Array2::zeros(dim),
            delta: Array2::from_elem(dim, false),
        }
    }

    /// Ocean margins east and west (cf. `Coast::widen`): grids gain
    /// zeroed columns, mouth coordinates shift east by `pad`.
    pub fn widen(&mut self, pad: usize) {
        if pad == 0 || self.depth.is_empty() {
            return;
        }
        let (h, w) = self.depth.dim();
        let p = pad as isize;
        self.depth = Array2::from_shape_fn((h, w + 2 * pad), |(y, x)| {
            let xi = x as isize - p;
            if xi >= 0 && (xi as usize) < w {
                self.depth[[y, xi as usize]]
            } else {
                0.0
            }
        });
        self.delta = Array2::from_shape_fn((h, w + 2 * pad), |(y, x)| {
            let xi = x as isize - p;
            xi >= 0 && (xi as usize) < w && self.delta[[y, xi as usize]]
        });
        for m in &mut self.mouths {
            m.x += pad as u32;
        }
    }

    /// FNV-1a over ledger scalars, mouths and both grids — the raw
    /// books at full bit width. Joins `hash_state`, so native replay
    /// identity covers every f64 the budget wrote. NOT cross-runtime:
    /// the heightfield upstream is transcendental (geo.rs runs on host
    /// libm), and this hash is the first to read raw f64 sums off it —
    /// measured on seed 777 it diverges native↔wasm by ulps while the
    /// wire-precision world (pack bytes, towns, routes, features) is
    /// identical. The identity line carries `footprint_hash` instead.
    pub fn hash(&self) -> u64 {
        let mut b: Vec<u8> = Vec::with_capacity(self.depth.len() * 4 + self.delta.len() + 64);
        for v in [self.detached, self.settled, self.delta_fill, self.abyssal] {
            b.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        for m in &self.mouths {
            b.extend_from_slice(&m.y.to_le_bytes());
            b.extend_from_slice(&m.x.to_le_bytes());
            b.extend_from_slice(&m.load.to_bits().to_le_bytes());
            b.extend_from_slice(&m.area.to_bits().to_le_bytes());
            b.extend_from_slice(&m.fan.to_le_bytes());
        }
        for v in self.depth.iter() {
            b.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        for &v in self.delta.iter() {
            b.push(v as u8);
        }
        fnv1a64(&b)
    }

    /// M59/M27 — the budget's integer-robust face for the cross-runtime
    /// identity line: mouth cells in ledger order and the delta-land
    /// grid. Both survive the heightfield's known ulp-level libm
    /// variance the way the other measured layers (rock, landform,
    /// coast) do — a mouth registers on an integer drainage-area bar
    /// and a load four orders above the noise, and a delta cell is a
    /// filled-to-the-top boolean. The f64 loads, fan counts and depth
    /// grid stay in `hash()` under native determinism, where bit-exact
    /// is actually provable.
    pub fn footprint_hash(&self) -> u64 {
        let mut b: Vec<u8> = Vec::with_capacity(self.delta.len() + self.mouths.len() * 8);
        for m in &self.mouths {
            b.extend_from_slice(&m.y.to_le_bytes());
            b.extend_from_slice(&m.x.to_le_bytes());
        }
        for &v in self.delta.iter() {
            b.push(v as u8);
        }
        fnv1a64(&b)
    }
}

/// Thermal erosion: where a cell towers over a neighbour beyond the
/// angle of repose, the excess lets go and comes to rest below. Moves
/// material (conserves it) rather than just planing peaks off.
fn talus_pass(h: &mut Array2<f64>) {
    let (rows, cols) = h.dim();
    let mut delta = Array2::<f64>::zeros((rows, cols));
    for y in 0..rows {
        for x in 0..cols {
            let hc = h[[y, x]];
            if hc <= 0.0 {
                continue; // the seabed keeps its trenches
            }
            // steepest downhill neighbour
            let mut best = 0.0f64;
            let mut bi: Option<(usize, usize, f64)> = None;
            for (&(dy, dx), &dist) in N8.iter().zip(DIST.iter()) {
                let ny = y as isize + dy;
                let nx = x as isize + dx;
                if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize {
                    continue;
                }
                let (ny, nx) = (ny as usize, nx as usize);
                let s = (hc - h[[ny, nx]]) / dist;
                if s > best {
                    best = s;
                    bi = Some((ny, nx, dist));
                }
            }
            if let Some((ny, nx, dist)) = bi {
                if best > TALUS {
                    let move_amt = TALUS_K * 0.5 * (best - TALUS) * dist;
                    delta[[y, x]] -= move_amt;
                    delta[[ny, nx]] += move_amt;
                }
            }
        }
    }
    *h += &delta;
}

/// Stream-power incision, implicit in the manner of Braun & Willett:
/// walk the drainage tree from mouth to source and relax every cell
/// toward its receiver with strength K·√A. Unconditionally stable —
/// a cell can approach its receiver but never dig below it, so no
/// pass ever creates a pit the next fill has to paper over.
///
/// M59 — the pass now keeps books. Phase 1 (incision, receivers first)
/// records what each cell detached; phase 2 (routing, sources first)
/// carries that load down the same tree, settling the over-capacity
/// excess on the way and ledgering what reaches water; phase 3 builds
/// delta fans at the sea mouths. The pass conserves mass exactly:
/// every unit detached is settled, fan-filled, or ledgered abyssal.
fn fluvial_pass(h: &mut Array2<f64>, sed: &mut Sediment) {
    let size = h.dim().0;
    let cols = h.dim().1;
    let n = size * cols;
    let water = h.mapv(|v| v < 0.0);
    let filled = fill_depressions(h, &water);
    let dirs = flow_directions(&filled, &water);

    // drainage area in cells, accumulated down the tree — the shared
    // lattice law (M66/ADR-0026): same sort, same walk, same bits as
    // the local copy this replaced.
    let order = drainage_order(&filled);
    let area = accumulate(&order, &dirs, |_, _| 1.0, size);

    // ---- phase 1: incision, receivers first (low to high) -------------
    let mut eroded = vec![0.0f64; n];
    for &idx in order.iter().rev() {
        let (y, x) = (idx / cols, idx % cols);
        if water[[y, x]] {
            continue;
        }
        let d = dirs[[y, x]];
        if d < 0 {
            continue; // terminal cells (pit bottoms) keep their floor
        }
        if filled[[y, x]] - h[[y, x]] > 1e-4 {
            continue; // under standing water: deposition country, not erosion
        }
        let (dy, dx) = N8[d as usize];
        let (ny, nx) = ((y as isize + dy) as usize, (x as isize + dx) as usize);
        let dist = DIST[d as usize];
        // rivers never drag the coast below the tideline
        let hr = h[[ny, nx]].max(0.0015);
        let hc = h[[y, x]];
        if hc <= hr {
            continue;
        }
        let f = SPI_K * area[[y, x]].sqrt() / dist;
        let hn = (hc + f * hr) / (1.0 + f);
        h[[y, x]] = hn;
        eroded[idx] = hc - hn;
        sed.detached += hc - hn;
    }

    // ---- phase 2: routing, sources first (high to low) -----------------
    // The load walks the same tree the incision walked. Capacity drops
    // where the profile flattens; the excess settles (floodplain), a
    // standing-water cell takes its share (lake silt), and whatever
    // reaches a sea receiver or a terminal ledgers a mouth.
    let mut load = vec![0.0f64; n];
    // sea mouths this pass: flat index → (load delivered, max drainage area)
    let mut mouths: std::collections::BTreeMap<usize, (f64, f64)> = std::collections::BTreeMap::new();
    for &idx in &order {
        let (y, x) = (idx / cols, idx % cols);
        if water[[y, x]] {
            continue; // sea cells receive via the mouth ledger, not the sweep
        }
        let mut l = load[idx] + eroded[idx];
        if l <= 0.0 {
            continue;
        }
        let standing = filled[[y, x]] - h[[y, x]] > 1e-4;
        if standing {
            // lake / estuary silt: the flooded cell takes a share of its
            // remaining depth, the rest rides the spillway on downstream
            let room = (filled[[y, x]] - h[[y, x]]) * LAKE_K;
            let dep = l.min(room.max(0.0));
            if dep > 0.0 {
                h[[y, x]] += dep;
                sed.depth[[y, x]] += dep as f32;
                sed.settled += dep;
                l -= dep;
            }
        }
        let d = dirs[[y, x]];
        if d < 0 {
            // terminal land cell (a filled pit's floor): whatever the
            // standing water above could not take has nowhere resolvable
            // to go — it leaves the map's accounting, never the ledger's
            sed.abyssal += l;
            continue;
        }
        let (dy, dx) = N8[d as usize];
        let (ny, nx) = ((y as isize + dy) as usize, (x as isize + dx) as usize);
        let dist = DIST[d as usize];
        if !standing && l > 0.0 {
            // transport capacity on the post-incision profile
            let slope = ((h[[y, x]] - h[[ny, nx]]) / dist).max(0.0);
            let cap = CAP_K * area[[y, x]] * slope;
            if l > cap {
                let headroom = ((h[[y, x]] - h[[ny, nx]]) * DEP_HEADROOM).max(0.0);
                let dep = ((l - cap) * DEP_K).min(headroom);
                if dep > 0.0 {
                    h[[y, x]] += dep;
                    sed.depth[[y, x]] += dep as f32;
                    sed.settled += dep;
                    l -= dep;
                }
            }
        }
        if l <= 0.0 {
            continue;
        }
        if water[[ny, nx]] {
            // the river meets the sea: ledger the mouth
            let e = mouths.entry(ny * cols + nx).or_insert((0.0, 0.0));
            e.0 += l;
            e.1 = e.1.max(area[[y, x]]);
        } else {
            load[ny * cols + nx] += l;
        }
    }

    // ---- phase 3: delta fans at the sea mouths -------------------------
    // BFS shoreward from each mouth over shelf water, filling toward the
    // delta-plain top until the load runs out or the fan front hits the
    // reach or the floor. Ascending mouth index: deterministic, and two
    // mouths of one bay share the bay in ledger order.
    let mut stamp = Array2::<u32>::zeros((size, cols));
    let mut fan_id: u32 = 0;
    for (&midx, &(mload, marea)) in &mouths {
        if mload < MOUTH_MIN || marea < DELTA_AREA_MIN {
            // rills and wave-dominated mouths: the sea disperses the
            // load — it leaves the fans' map, never the ledger
            sed.abyssal += mload;
            continue;
        }
        fan_id += 1;
        let (my, mx) = (midx / cols, midx % cols);
        let mut remaining = mload;
        let mut fan_cells: u32 = 0;
        let mut queue = std::collections::VecDeque::new();
        if h[[my, mx]] < DELTA_TOP && h[[my, mx]] > FAN_FLOOR {
            queue.push_back((my, mx));
            stamp[[my, mx]] = fan_id;
        }
        while let Some((y, x)) = queue.pop_front() {
            if remaining <= 0.0 {
                break;
            }
            let fill = (DELTA_TOP - h[[y, x]]).min(remaining);
            if fill > 0.0 {
                h[[y, x]] += fill;
                sed.depth[[y, x]] += fill as f32;
                sed.delta_fill += fill;
                remaining -= fill;
                fan_cells += 1;
                if h[[y, x]] >= 0.0 {
                    sed.delta[[y, x]] = true;
                }
            }
            for &(dy, dx) in N8.iter() {
                let ny = y as isize + dy;
                let nx = x as isize + dx;
                if ny < 0 || nx < 0 || ny >= size as isize || nx >= cols as isize {
                    continue;
                }
                let (ny, nx) = (ny as usize, nx as usize);
                if stamp[[ny, nx]] == fan_id {
                    continue;
                }
                let cy = (ny as isize - my as isize).abs();
                let cx = (nx as isize - mx as isize).abs();
                if cy.max(cx) > FAN_R {
                    continue;
                }
                // only sea at pass start, only shelf, only below the top
                if !water[[ny, nx]] || h[[ny, nx]] <= FAN_FLOOR || h[[ny, nx]] >= DELTA_TOP {
                    continue;
                }
                stamp[[ny, nx]] = fan_id;
                queue.push_back((ny, nx));
            }
        }
        // past the fan front: off the shelf and out of the books' map
        sed.abyssal += remaining;
        // one ledger row per mouth cell across passes: load sums, the
        // drainage area takes the max, fan cells accumulate
        if let Some(m) = sed
            .mouths
            .iter_mut()
            .find(|m| (m.y as usize, m.x as usize) == (my, mx))
        {
            m.load += mload;
            m.area = m.area.max(marea);
            m.fan += fan_cells;
        } else {
            sed.mouths.push(Mouth {
                y: my as u32,
                x: mx as u32,
                load: mload,
                area: marea,
                fan: fan_cells,
            });
        }
    }
}

/// Soil creep: a gentle land-only diffusion. Cells average with their
/// land neighbours; the coastline itself never moves, so beaches stay
/// where the tectonics put them. E5.11 — the pre-pass snapshot lands in
/// a caller-owned scratch grid instead of a fresh clone per pass.
fn diffuse_pass(h: &mut Array2<f64>, src: &mut Array2<f64>) {
    let (rows, cols) = h.dim();
    src.assign(h);
    for y in 0..rows {
        for x in 0..cols {
            let hc = src[[y, x]];
            if hc <= 0.0 {
                continue;
            }
            let mut sum = 0.0;
            let mut n = 0usize;
            for (dy, dx) in crate::grid::N4 {
                let ny = y as isize + dy;
                let nx = x as isize + dx;
                if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize {
                    continue;
                }
                let hn = src[[ny as usize, nx as usize]];
                if hn > 0.0 {
                    sum += hn;
                    n += 1;
                }
            }
            if n > 0 {
                let mean = sum / n as f64;
                h[[y, x]] = hc + DIFF_D * (mean - hc);
            }
        }
    }
}

/// The full carving sequence, applied to the raw tectonic heightmap
/// before climate ever sees it. Returns the M59 sediment ledger the
/// fluvial passes kept while they carved.
pub fn erode(h: &mut Array2<f64>) -> Sediment {
    for _ in 0..TALUS_PASSES {
        talus_pass(h);
    }
    let mut sed = Sediment::empty(h.dim());
    for _ in 0..SPI_PASSES {
        fluvial_pass(h, &mut sed);
    }
    let mut src = Array2::<f64>::zeros(h.dim());
    for _ in 0..DIFF_PASSES {
        diffuse_pass(h, &mut src);
    }
    sed
}

// ---------------------------------------------------------------- bands
// M59 sweet/hard set from the measured landing across the standing
// report seeds (12345 · 777 · 90210), per E2.7, remeasured after M64's
// fluvial-dominance recalibration (DELTA_AREA_MIN 60 → 625): delta
// land ~1.0% of land, 78–83 fans, settled ~41–42%, abyssal ~32% (the
// dispersed small-mouth loads now rightly leave the shelf). The bands
// bracket that spread with room for seed families, then hold the
// mechanism there.

pub const BANDS: &[Band] = &[
    Band { name: "delta land share of land %", sweet: (0.3, 3.0), hard: (0.05, 6.0), target: "M59/M64: deltas on every coast, never swallowing one (measured 1.0% post-recalibration; Earth's deltaic plains ≈0.5–1% of land)" },
    Band { name: "river deltas per seed", sweet: (20.0, 200.0), hard: (5.0, 800.0), target: "M59/M64: mouths that built fans — a family of tens, as Earth carries at 4-km resolution (measured 78–83)" },
    Band { name: "sediment settled share %", sweet: (5.0, 75.0), hard: (1.0, 92.0), target: "M59: floodplains and lakes take a real cut of the load" },
    Band { name: "sediment abyssal share %", sweet: (2.0, 80.0), hard: (0.5, 95.0), target: "M59: the deep sea takes what the shelf cannot hold" },
    // M64 — calibration vs Earth: floodplain extent. Mapped alluvial
    // floodplains cover ≈2% of Earth's land (Tockner & Stanford 2002,
    // 2.1 M km²); the broader 100-yr fluvial belts reach ~7% (GFPLAIN).
    // The plain is silt ≥10 m of fill — an alluvial body, not a 1 m
    // overbank veneer. And a floodplain is river-built ground: the law
    // is per body, since plains run wider than one cell — every silt
    // body must hold (or border) the channel that laid it.
    Band { name: "floodplain share of land %", sweet: (0.3, 9.0), hard: (0.05, 18.0), target: "M64: Earth's mapped alluvium ≈2% of land; 100-yr belts ~7% (silt ≥10 m)" },
    Band { name: "floodplain river adjacency %", sweet: (55.0, 100.0), hard: (30.0, 100.0), target: "M64: % of plain area in river-borne bodies — silt bodies hold the water that laid them" },
];
