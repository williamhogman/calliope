//! Hydrology — port of hydrology.py: priority-flood fill, D8 routing, rivers.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use ndarray::Array2;

pub const N8: [(isize, isize); 8] = [
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, -1),
    (0, 1),
    (1, -1),
    (1, 0),
    (1, 1),
];
pub const DIST: [f64; 8] = [
    1.4142135, 1.0, 1.4142135, 1.0, 1.0, 1.4142135, 1.0, 1.4142135,
];

/// Min-heap item ordered like Python's heapq tuples (h, y, x).
struct HeapItem(f64, usize, usize);

impl PartialEq for HeapItem {
    fn eq(&self, o: &Self) -> bool {
        self.0 == o.0 && self.1 == o.1 && self.2 == o.2
    }
}
impl Eq for HeapItem {}
impl PartialOrd for HeapItem {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for HeapItem {
    fn cmp(&self, o: &Self) -> Ordering {
        // reversed: BinaryHeap is a max-heap, we want the smallest first
        o.0
            .partial_cmp(&self.0)
            .unwrap()
            .then_with(|| o.1.cmp(&self.1))
            .then_with(|| o.2.cmp(&self.2))
    }
}

/// Priority-flood fill so every land cell drains to the ocean or map edge.
pub fn fill_depressions(height: &Array2<f64>, water: &Array2<bool>) -> Array2<f64> {
    let eps = 1e-5;
    let size = height.dim().0;
    let mut filled = height.clone();
    let mut visited = water.clone();
    let mut heap: BinaryHeap<HeapItem> = BinaryHeap::new();

    // seeds: land on the border, and land adjacent to water
    for y in 0..size {
        for x in 0..size {
            if water[[y, x]] {
                continue;
            }
            let border = y == 0 || y == size - 1 || x == 0 || x == size - 1;
            let mut adj = false;
            if !border {
                adj = water[[y - 1, x]] || water[[y + 1, x]] || water[[y, x - 1]]
                    || water[[y, x + 1]];
            } else {
                for (dy, dx) in [(-1isize, 0isize), (1, 0), (0, -1), (0, 1)] {
                    let ny = y as isize + dy;
                    let nx = x as isize + dx;
                    if ny >= 0 && nx >= 0 && ny < size as isize && nx < size as isize {
                        adj |= water[[ny as usize, nx as usize]];
                    }
                }
            }
            if border || adj {
                heap.push(HeapItem(filled[[y, x]], y, x));
                visited[[y, x]] = true;
            }
        }
    }

    while let Some(HeapItem(hcur, y, x)) = heap.pop() {
        for &(dy, dx) in N8.iter() {
            let ny = y as isize + dy;
            let nx = x as isize + dx;
            if ny < 0 || nx < 0 || ny >= size as isize || nx >= size as isize {
                continue;
            }
            let (ny, nx) = (ny as usize, nx as usize);
            if visited[[ny, nx]] {
                continue;
            }
            visited[[ny, nx]] = true;
            let mut nh = filled[[ny, nx]];
            if nh <= hcur {
                nh = hcur + eps;
                filled[[ny, nx]] = nh;
            }
            heap.push(HeapItem(nh, ny, nx));
        }
    }
    filled
}

/// D8: index 0..7 into N8 of the steepest downslope neighbour, -1 = terminal.
pub fn flow_directions(filled: &Array2<f64>, water: &Array2<bool>) -> Array2<i8> {
    let size = filled.dim().0;
    Array2::from_shape_fn((size, size), |(y, x)| {
        if water[[y, x]] {
            return -1i8;
        }
        let mut best_drop = 0.0f64;
        let mut best_dir = -1i8;
        for (i, (&(dy, dx), &dist)) in N8.iter().zip(DIST.iter()).enumerate() {
            let ny = y as isize + dy;
            let nx = x as isize + dx;
            if ny < 0 || nx < 0 || ny >= size as isize || nx >= size as isize {
                continue;
            }
            let drop = (filled[[y, x]] - filled[[ny as usize, nx as usize]]) / dist;
            if drop > best_drop {
                best_drop = drop;
                best_dir = i as i8;
            }
        }
        best_dir
    })
}

/// Cells sorted by filled height, high to low — donors before receivers.
pub fn drainage_order(filled: &Array2<f64>) -> Vec<usize> {
    let size = filled.dim().0;
    let mut order: Vec<usize> = (0..size * size).collect();
    order.sort_by(|&a, &b| {
        let fa = filled[[a / size, a % size]];
        let fb = filled[[b / size, b % size]];
        fb.partial_cmp(&fa).unwrap().then(a.cmp(&b)) // high to low
    });
    order
}

/// Accumulate a per-cell weight down the drainage tree.
fn accumulate(
    order: &[usize],
    dirs: &Array2<i8>,
    weight: impl Fn(usize, usize) -> f64,
    size: usize,
) -> Array2<f64> {
    let mut acc = Array2::from_shape_fn((size, size), |(y, x)| weight(y, x));
    for &idx in order {
        let (y, x) = (idx / size, idx % size);
        let d = dirs[[y, x]];
        if d >= 0 {
            let (dy, dx) = N8[d as usize];
            let ny = y as isize + dy;
            let nx = x as isize + dx;
            if ny >= 0 && nx >= 0 && ny < size as isize && nx < size as isize {
                let v = acc[[y, x]];
                acc[[ny as usize, nx as usize]] += v;
            }
        }
    }
    acc
}

/// Accumulate precip downstream; returns discharge (precip-weighted area).
pub fn flow_accumulation(
    filled: &Array2<f64>,
    dirs: &Array2<i8>,
    precip: &Array2<f64>,
    water: &Array2<bool>,
) -> Array2<f64> {
    let size = filled.dim().0;
    let order = drainage_order(filled);
    accumulate(
        &order,
        dirs,
        |y, x| if water[[y, x]] { 0.0 } else { precip[[y, x]] / 1000.0 },
        size,
    )
}

pub struct Hydrology {
    pub filled: Array2<f64>,
    pub dirs: Array2<i8>,
    pub discharge: Array2<f64>,
    pub rivers: Array2<bool>,
    pub lakes: Array2<bool>,
    /// Endorheic basins: lakes with no road to the sea, crusted white.
    pub salt: Array2<bool>,
    /// Rivers that fail their threshold in the dry season — wadis.
    pub seasonal: Array2<bool>,
    /// Strahler stream order, 0 for non-river cells.
    pub strahler: Array2<u8>,
    /// Signed seasonal discharge swing, -1..1 (positive peaks month 0).
    pub flow_amp: Array2<f64>,
    /// M32 — braided reaches: river cells running over an outwash
    /// corridor, wandering in gravel sheets instead of one channel.
    pub braided: Array2<bool>,
    /// M35 — accumulated glacial meltwater discharge per cell, same
    /// units as `discharge` (of which it is a component).
    pub melt: Array2<f64>,
    /// M35 — signed month-0 harmonic of the melt lane, −1..1, same
    /// convention as `flow_amp`; 0 wherever no melt flows.
    pub melt_amp: Array2<f64>,
}

// 60.0 keeps the great rivers and their major tributaries (~4% of land)
// and prunes the minor-stream fuzz the 4 km cells can't honestly carry.
pub const RIVER_THRESHOLD: f64 = 60.0;

/// M35 — a river cell is glacier-fed when at least this share of its
/// discharge is accumulated melt. Glacier-fed rivers keep a reliable
/// warm-season flow, so the wadi stamp yields to them.
pub const GLACIAL_MIN: f64 = 0.25;

pub fn hydrology(
    height: &Array2<f64>,
    water: &Array2<bool>,
    precip: &Array2<f64>,
    pamp: &Array2<f64>,
    tmean: &Array2<f64>,
    tamp: &Array2<f64>,
    outwash: &Array2<f32>,
    modern: &Array2<f32>,
) -> Hydrology {
    let size = height.dim().0;
    let filled = fill_depressions(height, water);
    let mut dirs = flow_directions(&filled, water);
    let order = drainage_order(&filled);

    let mut lakes = Array2::<bool>::from_elem((size, size), false);
    for y in 0..size {
        for x in 0..size {
            lakes[[y, x]] = !water[[y, x]] && (filled[[y, x]] - height[[y, x]] > 0.004);
        }
    }

    // --- endorheic basins: in dry warm country, a lake evaporates what
    // its rivers bring and never reaches the sea. Flow terminates in the
    // basin floor; downstream of the ghost-spill the channel runs dry.
    let mut salt = Array2::<bool>::from_elem((size, size), false);
    let mut seen = Array2::<bool>::from_elem((size, size), false);
    let mut any_terminal = false;
    for y in 0..size {
        for x in 0..size {
            if !lakes[[y, x]] || seen[[y, x]] {
                continue;
            }
            // flood-fill this lake component (4-connectivity, scan order)
            let mut comp = vec![(y, x)];
            seen[[y, x]] = true;
            let mut qi = 0usize;
            while qi < comp.len() {
                let (cy, cx) = comp[qi];
                qi += 1;
                for (dy, dx) in [(-1isize, 0isize), (1, 0), (0, -1), (0, 1)] {
                    let ny = cy as isize + dy;
                    let nx = cx as isize + dx;
                    if ny < 0 || nx < 0 || ny >= size as isize || nx >= size as isize {
                        continue;
                    }
                    let (ny, nx) = (ny as usize, nx as usize);
                    if lakes[[ny, nx]] && !seen[[ny, nx]] {
                        seen[[ny, nx]] = true;
                        comp.push((ny, nx));
                    }
                }
            }
            let m = comp.len() as f64;
            let p_mean: f64 = comp.iter().map(|&(a, b)| precip[[a, b]]).sum::<f64>() / m;
            let t_mean: f64 = comp.iter().map(|&(a, b)| tmean[[a, b]]).sum::<f64>() / m;
            let d_mean: f64 =
                comp.iter().map(|&(a, b)| filled[[a, b]] - height[[a, b]]).sum::<f64>() / m;
            if comp.len() >= 2 && p_mean < 520.0 && t_mean > 8.0 && d_mean > 0.006 {
                any_terminal = true;
                for &(a, b) in &comp {
                    salt[[a, b]] = true;
                    dirs[[a, b]] = -1; // the water stops here and rises as haze
                }
            }
        }
    }

    // --- M35: the glacier partition. On a glacier cell the year's
    // snow is banked, not run off — `climate::melt_throughput` splits
    // the cell into a melt lane (the bank, released by positive-degree
    // months, summer-phased) and a rain lane (the warm months' rain,
    // which runs off immediately with its true harmonic instead of
    // pretending the banked snow fell as winter rain). Off-glacier
    // cells keep the classic precip/pamp sources. Mass is conserved:
    // melt + rain on a glacier cell sums to its runoff-eligible
    // precipitation (a cap with no melt months banks its snow forever).
    let mut melt_src = Array2::<f64>::zeros((size, size));
    let mut melt_amp_src = Array2::<f64>::zeros((size, size));
    let mut rain_src = Array2::<f64>::zeros((size, size));
    let mut rain_harm_src = Array2::<f64>::zeros((size, size));
    let glaciated = modern.dim() == (size, size);
    for y in 0..size {
        for x in 0..size {
            if water[[y, x]] {
                continue;
            }
            if glaciated && modern[[y, x]] > 0.0 {
                let (melt, amp, rain, rharm) = crate::climate::melt_throughput(
                    tmean[[y, x]],
                    tamp[[y, x]],
                    precip[[y, x]],
                    pamp[[y, x]],
                );
                melt_src[[y, x]] = melt;
                melt_amp_src[[y, x]] = melt * amp;
                rain_src[[y, x]] = rain;
                rain_harm_src[[y, x]] = rharm;
            } else {
                rain_src[[y, x]] = precip[[y, x]] / 1000.0;
                rain_harm_src[[y, x]] = precip[[y, x]] * pamp[[y, x]] / 1000.0;
            }
        }
    }
    let rain_acc = accumulate(&order, &dirs, |y, x| rain_src[[y, x]], size);
    // second accumulation, weighted by the signed seasonal share: the
    // ratio to total discharge says how hard each river breathes.
    let acc_season = accumulate(&order, &dirs, |y, x| rain_harm_src[[y, x]], size);
    // M35 — the melt lane, flow-routed down the same tree, plus its
    // signed harmonic mass for the combined seasonal swing.
    let melt = accumulate(&order, &dirs, |y, x| melt_src[[y, x]], size);
    let melt_harm = accumulate(&order, &dirs, |y, x| melt_amp_src[[y, x]], size);
    let discharge = &rain_acc + &melt;
    let mut melt_amp = Array2::<f64>::zeros((size, size));
    for y in 0..size {
        for x in 0..size {
            if melt[[y, x]] > 1e-12 {
                melt_amp[[y, x]] = (melt_harm[[y, x]] / melt[[y, x]]).clamp(-1.0, 1.0);
            }
        }
    }
    let _ = any_terminal;

    let mut rivers = Array2::<bool>::from_elem((size, size), false);
    for y in 0..size {
        for x in 0..size {
            rivers[[y, x]] = !water[[y, x]]
                && !lakes[[y, x]]
                && discharge[[y, x]] > RIVER_THRESHOLD;
        }
    }

    // --- Strahler order over the whole drainage net: every land cell
    // carries a rill of order 1, and a stream steps up only where two
    // branches of its own order meet. The visible rivers then wear the
    // true orders of their basins — mainstems come out 6th, 7th order.
    let mut strahler = Array2::<u8>::from_elem((size, size), 0);
    for &idx in &order {
        let (y, x) = (idx / size, idx % size);
        if water[[y, x]] {
            continue;
        }
        let mut top = 0u8;
        let mut top_n = 0usize;
        for (i, &(dy, dx)) in N8.iter().enumerate() {
            let ny = y as isize + dy;
            let nx = x as isize + dx;
            if ny < 0 || nx < 0 || ny >= size as isize || nx >= size as isize {
                continue;
            }
            let (ny, nx) = (ny as usize, nx as usize);
            if water[[ny, nx]] {
                continue;
            }
            // does this neighbour flow into us?
            let d = dirs[[ny, nx]];
            if d < 0 {
                continue;
            }
            let (ddy, ddx) = N8[d as usize];
            if (ny as isize + ddy, nx as isize + ddx) != (y as isize, x as isize) {
                continue;
            }
            let o = strahler[[ny, nx]];
            if o > top {
                top = o;
                top_n = 1;
            } else if o == top && o > 0 {
                top_n += 1;
            }
            let _ = i;
        }
        strahler[[y, x]] = if top == 0 {
            1
        } else if top_n >= 2 {
            (top + 1).min(12)
        } else {
            top
        };
    }

    // --- seasonal swing and wadis
    let mut flow_amp = Array2::<f64>::zeros((size, size));
    let mut seasonal = Array2::<bool>::from_elem((size, size), false);
    for y in 0..size {
        for x in 0..size {
            if discharge[[y, x]] > 1e-9 {
                // M35 — both lanes breathe into one swing: the rain
                // harmonic plus the summer-phased melt harmonic.
                flow_amp[[y, x]] = ((acc_season[[y, x]] + melt_harm[[y, x]])
                    / discharge[[y, x]])
                    .clamp(-1.0, 1.0);
            }
            if rivers[[y, x]] {
                // at low water the year's rain leans away: a channel that
                // no longer clears the river bar is a wadi, full half the
                // year and a ribbon of cracked mud the other half. M35:
                // glacier-fed rivers are exempt — the melt returns every
                // summer, however hard the swing reads.
                let dry = discharge[[y, x]] * (1.0 - flow_amp[[y, x]].abs());
                seasonal[[y, x]] = dry < RIVER_THRESHOLD
                    && melt[[y, x]] / discharge[[y, x]] < GLACIAL_MIN;
            }
        }
    }

    // --- M32: braided reaches — a river crossing an outwash corridor
    // wanders in gravel sheets instead of a single channel. Corridors
    // are flat by construction (ice::OUT_SLOPE_MAX), so the low-slope
    // test is already priced into the mask.
    let mut braided = Array2::<bool>::from_elem((size, size), false);
    if outwash.dim() == (size, size) {
        for y in 0..size {
            for x in 0..size {
                braided[[y, x]] =
                    rivers[[y, x]] && outwash[[y, x]] >= crate::ice::OUT_BRAID_MIN;
            }
        }
    }

    Hydrology {
        filled,
        dirs,
        discharge,
        rivers,
        lakes,
        salt,
        seasonal,
        strahler,
        flow_amp,
        braided,
        melt,
        melt_amp,
    }
}

// ------------------------------------------------------- M54 aquifers

/// M54 — the water table beneath the map.
///
/// Rain that neither runs off nor evaporates soaks in, and the ground
/// carries it sideways toward whatever the land has already opened: a
/// river, a lake, the sea. The steady state of that slow sideways
/// travel is Darcy's law with a recharge source,
///
/// ```text
///     ∇·(K ∇h) + R = 0
/// ```
///
/// solved here for hydraulic head `h` (metres above sea level) on the
/// same grid the rivers were routed over. `K` is the rock province's
/// conductivity (M18), `R` the share of the year's rain that infiltrates,
/// and the boundary conditions are the surface waters themselves —
/// where a river, a lake or the ocean already stands, the table is *at*
/// that water and cannot rise past it. Everywhere else the head is free,
/// clamped only by the ground above it (a table cannot daylight where no
/// spring was drawn) and by a regional floor far below.
///
/// The output grid is **depth to water**: `surface − head`, in metres.
/// Zero on open water and where the table reaches the surface; deep
/// under dry uplands of permeable rock.
///
/// Frozen at genesis like every other physical field (ADR-0005), CRC-
/// stable, and hashed.

/// Relative hydraulic conductivity by rock province (M18). Crystalline
/// shield rock passes water only through its fractures; a sedimentary
/// basin is the classic aquifer; folded strata sit between; young
/// volcanics are cracked and thirsty but shallow-bedded.
pub fn conductivity(province: u8) -> f64 {
    match province {
        crate::rock::SHIELD => 0.10,
        crate::rock::BASIN => 1.00,
        crate::rock::FOLD_BELT => 0.32,
        crate::rock::VOLCANIC => 0.55,
        _ => 0.50,
    }
}

/// Share of the year's rain that reaches the table rather than running
/// off or returning to the sky — modulated by the same rock.
fn infiltration(province: u8) -> f64 {
    match province {
        crate::rock::SHIELD => 0.06,
        crate::rock::BASIN => 0.20,
        crate::rock::FOLD_BELT => 0.11,
        crate::rock::VOLCANIC => 0.16,
        _ => 0.12,
    }
}

/// The regional floor: no cell reports a table deeper than this. Below
/// it the rock is dry enough that "how deep" stops meaning anything a
/// well could act on.
pub const AQUIFER_FLOOR_M: f64 = 150.0;

/// Metres of head the unit recharge buys against unit conductivity —
/// the one dial that sets how high the table mounds between drains.
const AQUIFER_MOUND: f64 = 60.0;

/// Subgrid drainage: at 4 km cells the routed river network is only the
/// trunk of the real drainage. Every valley that gathers even a little
/// flow carries an unmapped stream, and that stream drains the table
/// beside it. Cells above this accumulation are treated as drains —
/// pinned to their own surface — so the table is a *subdued replica* of
/// the terrain rather than a single dome under the whole upland.
pub const SUBGRID_DRAIN_Q: f64 = 6.0;

/// Successive over-relaxation factor and sweep counts, coarse to fine.
const AQ_OMEGA: f64 = 1.82;
const AQ_SWEEPS: [usize; 3] = [90, 34, 12];

/// Solve the steady-state water table; returns depth to water in metres.
pub fn water_table(
    height: &Array2<f64>,
    water: &Array2<bool>,
    rivers: &Array2<bool>,
    lakes: &Array2<bool>,
    discharge: &Array2<f64>,
    precip: &Array2<f64>,
    rock: &Array2<u8>,
) -> Array2<f32> {
    let (rows, cols) = height.dim();
    let m_per_unit = crate::constants::METRES_PER_UNIT;

    // Per-cell surface, conductivity, recharge and pinning.
    let mut surf = Array2::<f64>::zeros((rows, cols));
    let mut k = Array2::<f64>::zeros((rows, cols));
    let mut rech = Array2::<f64>::zeros((rows, cols));
    let mut pinned = Array2::<bool>::from_elem((rows, cols), false);
    for y in 0..rows {
        for x in 0..cols {
            let s = height[[y, x]] * m_per_unit;
            surf[[y, x]] = s;
            let p = rock[[y, x]];
            k[[y, x]] = conductivity(p);
            // mm/yr -> m/yr, times the province's infiltration share
            rech[[y, x]] = (precip[[y, x]].max(0.0) / 1000.0) * infiltration(p);
            pinned[[y, x]] = water[[y, x]]
                || rivers[[y, x]]
                || lakes[[y, x]]
                || discharge[[y, x]] >= SUBGRID_DRAIN_Q;
        }
    }

    // Head starts at the drains and relaxes upward. Ocean pins at sea
    // level (0 m); fresh water pins at its own surface.
    let mut head = Array2::<f64>::zeros((rows, cols));
    for y in 0..rows {
        for x in 0..cols {
            head[[y, x]] = if water[[y, x]] {
                0.0
            } else if pinned[[y, x]] {
                surf[[y, x]]
            } else {
                (surf[[y, x]] - AQUIFER_FLOOR_M).max(0.0)
            };
        }
    }

    // Coarse-to-fine: the table is a long-wavelength surface, so the
    // low frequencies are settled on a cheap grid first and the fine
    // sweeps only clean up the detail. Deterministic: fixed sweep
    // counts, fixed scan order, no convergence test on wall clock.
    for (level, &sweeps) in AQ_SWEEPS.iter().enumerate() {
        let step = 1usize << (AQ_SWEEPS.len() - 1 - level); // 4, 2, 1
        for _ in 0..sweeps {
            sor_sweep(&mut head, &surf, &k, &rech, &pinned, step);
        }
    }

    // Depth to water, clamped to the reportable window.
    let mut depth = Array2::<f32>::zeros((rows, cols));
    for y in 0..rows {
        for x in 0..cols {
            depth[[y, x]] = if water[[y, x]] {
                0.0
            } else {
                ((surf[[y, x]] - head[[y, x]]).clamp(0.0, AQUIFER_FLOOR_M)) as f32
            };
        }
    }
    depth
}

/// One over-relaxed Gauss-Seidel sweep of ∇·(K∇h) + R = 0 over the
/// sub-lattice of stride `step`, in fixed scan order.
fn sor_sweep(
    head: &mut Array2<f64>,
    surf: &Array2<f64>,
    k: &Array2<f64>,
    rech: &Array2<f64>,
    pinned: &Array2<bool>,
    step: usize,
) {
    let (rows, cols) = head.dim();
    let h2 = (step * step) as f64;
    let mut y = 0usize;
    while y < rows {
        let mut x = 0usize;
        while x < cols {
            if pinned[[y, x]] {
                x += step;
                continue;
            }
            let kc = k[[y, x]];
            let mut num = 0.0;
            let mut den = 0.0;
            for (dy, dx) in [(-1isize, 0isize), (1, 0), (0, -1), (0, 1)] {
                let ny = y as isize + dy * step as isize;
                let nx = x as isize + dx * step as isize;
                if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize {
                    continue;
                }
                let (ny, nx) = (ny as usize, nx as usize);
                let kn = k[[ny, nx]];
                // harmonic mean: the tighter rock throttles the face
                let t = if kc + kn > 0.0 { 2.0 * kc * kn / (kc + kn) } else { 0.0 };
                num += t * head[[ny, nx]];
                den += t;
            }
            if den <= 0.0 {
                x += step;
                continue;
            }
            let target = (num + AQUIFER_MOUND * rech[[y, x]] * h2) / den;
            let relaxed = head[[y, x]] + AQ_OMEGA * (target - head[[y, x]]);
            head[[y, x]] = relaxed
                .min(surf[[y, x]])
                .max(surf[[y, x]] - AQUIFER_FLOOR_M);
            x += step;
        }
        y += step;
    }
}

// ---------------------------------------------------------------- bands

use crate::util::Band;

/// Diagnostics bands (E2.7): rivers, lakes and their power.
pub const BANDS: &[Band] = &[
    Band { name: "river share of land", sweet: (0.008, 0.05), hard: (0.003, 0.10), target: "sweet 0.8–5% · hard 0.3–10%" },
    Band { name: "lake share of land", sweet: (0.0, 0.03), hard: (0.0, 0.08), target: "sweet 0–3% · hard 0–8%" },
    Band { name: "river systems", sweet: (8.0, 400.0), hard: (3.0, 2000.0), target: "sweet 8–400 · hard 3–2000" },
    Band { name: "strahler top order", sweet: (4.0, 9.0), hard: (3.0, 12.0), target: "sweet 4–9 · hard 3–12" },
    Band { name: "river seasonal swing", sweet: (0.05, 0.50), hard: (0.01, 0.90), target: "mean |amp| · sweet .05–.50 · hard .01–.90" },
    Band { name: "aquifer median depth m", sweet: (4.0, 60.0), hard: (1.0, 120.0), target: "M54: median depth to water on unpinned land · sweet 4–60 m · hard 1–120" },
    Band { name: "glacier-fed river share %", sweet: (0.5, 15.0), hard: (0.05, 40.0), target: "sweet 0.5–15 · hard 0.05–40 (M35: % of river cells carrying ≥25% accumulated melt — the ice keeps its rivers; measured 1.3–1.5 on three seeds)" },
];
