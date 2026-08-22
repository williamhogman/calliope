//! Hydrology — the *water* stage of the fixed pipeline (rock → ice →
//! **water** → soil → landform, ADR-0026): rivers, lakes, salt basins,
//! seasonal regimes and the Darcy water table, all read off the relief
//! the ice stage finished. Ported from hydrology.py; the shared lattice
//! law it grew (priority-flood fill, D8 routing, drainage-order
//! accumulation) now lives in `grid` and is re-exported here so the
//! historical `hydrology::` paths keep working.

use ndarray::Array2;

// M66/ADR-0026 — the lattice law: N8 order, first-wins descent,
// high-to-low index-tied drainage sort. Moved verbatim to `grid`;
// the bits of every accumulated sum depend on that order.
pub use crate::grid::{
    accumulate, drainage_order, fill_depressions, flow_accumulation, flow_directions, DIST, N8,
};

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
                for (dy, dx) in crate::grid::N4 {
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
    //
    // K is frozen for the whole solve, so each level's face
    // transmissivities (harmonic means) are computed once here and
    // reused by every sweep of that level — the same expression on the
    // same operands as the in-loop version, so the head is bit-identical
    // while the sweep sheds four divisions per cell (E10.1).
    let ks = k.as_slice().expect("k is standard layout");
    let mut t_h = vec![0.0f64; rows * cols];
    let mut t_v = vec![0.0f64; rows * cols];
    for (level, &sweeps) in AQ_SWEEPS.iter().enumerate() {
        let step = 1usize << (AQ_SWEEPS.len() - 1 - level); // 4, 2, 1
        let mut y = 0usize;
        while y < rows {
            let row = y * cols;
            let mut x = 0usize;
            while x < cols {
                let i = row + x;
                let kc = ks[i];
                if x + step < cols {
                    let kn = ks[i + step];
                    t_h[i] = if kc + kn > 0.0 { 2.0 * kc * kn / (kc + kn) } else { 0.0 };
                }
                if y + step < rows {
                    let kn = ks[i + step * cols];
                    t_v[i] = if kc + kn > 0.0 { 2.0 * kc * kn / (kc + kn) } else { 0.0 };
                }
                x += step;
            }
            y += step;
        }
        for _ in 0..sweeps {
            sor_sweep(&mut head, &surf, &rech, &pinned, step, &t_h, &t_v);
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
///
/// E10.1 — the sweep runs on the flat row-major slices with the four
/// neighbor branches unrolled, and reads its face transmissivities from
/// the per-level tables built in `water_table`: K never changes between
/// sweeps, so the harmonic means are computed once per level instead of
/// four divisions per cell per sweep. The arithmetic (accumulation
/// order N, S, W, E; over-relaxed update) is byte-for-byte the naive
/// loop's. This kernel is the deep half of the fertility stage's
/// budget, swept 136 times per world.
///
/// `t_h[i]` is the face between cell `i` and `i + step` (east); `t_v[i]`
/// the face between `i` and `i + step*cols` (south).
fn sor_sweep(
    head: &mut Array2<f64>,
    surf: &Array2<f64>,
    rech: &Array2<f64>,
    pinned: &Array2<bool>,
    step: usize,
    t_h: &[f64],
    t_v: &[f64],
) {
    let (rows, cols) = head.dim();
    let h2 = (step * step) as f64;
    // Standard-layout grids: the flat slices exist by construction.
    let hs = head.as_slice_mut().expect("head is standard layout");
    let ss = surf.as_slice().expect("surf is standard layout");
    let rs = rech.as_slice().expect("rech is standard layout");
    let ps = pinned.as_slice().expect("pinned is standard layout");
    let vstride = step * cols;
    let mut y = 0usize;
    while y < rows {
        let row = y * cols;
        let up = y >= step;
        let down = y + step < rows;
        let mut x = 0usize;
        while x < cols {
            let i = row + x;
            if ps[i] {
                x += step;
                continue;
            }
            let mut num = 0.0;
            let mut den = 0.0;
            // The four faces in the fixed order (-1,0) (1,0) (0,-1)
            // (0,1); harmonic mean — the tighter rock throttles the face.
            if up {
                let j = i - vstride;
                let t = t_v[j];
                num += t * hs[j];
                den += t;
            }
            if down {
                let t = t_v[i];
                num += t * hs[i + vstride];
                den += t;
            }
            if x >= step {
                let j = i - step;
                let t = t_h[j];
                num += t * hs[j];
                den += t;
            }
            if x + step < cols {
                let t = t_h[i];
                num += t * hs[i + step];
                den += t;
            }
            if den <= 0.0 {
                x += step;
                continue;
            }
            let target = (num + AQUIFER_MOUND * rs[i] * h2) / den;
            let relaxed = hs[i] + AQ_OMEGA * (target - hs[i]);
            hs[i] = relaxed.min(ss[i]).max(ss[i] - AQUIFER_FLOOR_M);
            x += step;
        }
        y += step;
    }
}

// --------------------------------------------- M55 springs and oases

/// M55 — where the buried water reaches the day.
///
/// Two ways the table becomes drinkable without a shaft. A **spring**
/// is the head surface cutting the ground: on a hillside the table is a
/// subdued replica of the terrain, so where the slope suddenly steepens
/// the ground falls away faster than the water does and the water comes
/// out. That is a break in slope, and it is measured as one — the drop
/// below the cell steeper than the rise above it, with the table already
/// within a couple of metres of the surface. An **oasis** is the arid
/// case with no break at all: desert ground where the table sits inside
/// the reach of phreatophyte roots, so the vegetation itself tells you
/// the water is there.
///
/// Both are derived from the frozen `aquifer` grid and the frozen
/// relief, so they inherit its determinism; neither rides the wire.
pub struct DryWater {
    /// The table daylights here — a running spring.
    pub springs: Array2<bool>,
    /// Arid ground standing over water shallow enough to root in.
    pub oases: Array2<bool>,
}

/// A spring only counts where the table is this close to the surface.
pub const SPRING_DAYLIGHT_M: f64 = 2.0;
/// Minimum steepening across the cell (m of fall per km, downslope
/// minus upslope) for the ground to outrun the water table.
pub const SPRING_BREAK_M_PER_KM: f64 = 6.0;
/// Below this downslope gradient the hillside is too gentle to open a
/// seep however the curvature reads.
pub const SPRING_MIN_SLOPE_M_PER_KM: f64 = 4.0;
/// Phreatophytes (date palm, tamarisk, mesquite) root about this deep;
/// past it the desert stays desert.
pub const OASIS_DEPTH_M: f64 = 8.0;
/// Arid by rainfall: below this the year cannot carry a farm on rain.
pub const ARID_PRECIP_MM: f64 = 300.0;

/// True where the year is too dry to settle on rainfall alone — the
/// desert biome, or any ground under `ARID_PRECIP_MM`. One definition,
/// shared by the siting veto and the diagnostics gate so the check and
/// the world cannot drift apart.
pub fn arid(biome: u8, precip_mm: f64) -> bool {
    biome == crate::constants::DESERT || precip_mm < ARID_PRECIP_MM
}

/// Mark springs and oases over the solved table.
pub fn springs_and_oases(
    height: &Array2<f32>,
    water: &Array2<bool>,
    rivers: &Array2<bool>,
    lakes: &Array2<bool>,
    aquifer: &Array2<f32>,
    biomes: &Array2<u8>,
    precip: &Array2<f32>,
) -> DryWater {
    let (rows, cols) = height.dim();
    let m_per_unit = crate::constants::METRES_PER_UNIT;
    let km = crate::constants::KM_PER_CELL;
    let mut springs = Array2::from_elem((rows, cols), false);
    let mut oases = Array2::from_elem((rows, cols), false);
    for y in 0..rows {
        for x in 0..cols {
            if water[[y, x]] || rivers[[y, x]] || lakes[[y, x]] {
                continue;
            }
            let d = aquifer[[y, x]] as f64;
            let s = height[[y, x]] as f64 * m_per_unit;
            // Steepest fall below and steepest rise above, in m/km.
            let mut down = 0.0f64;
            let mut up = 0.0f64;
            for (dy, dx) in crate::grid::N4 {
                let ny = y as isize + dy;
                let nx = x as isize + dx;
                if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize {
                    continue;
                }
                let n = height[[ny as usize, nx as usize]] as f64 * m_per_unit;
                let g = (s - n) / km;
                if g > down {
                    down = g;
                }
                if -g > up {
                    up = -g;
                }
            }
            if d <= SPRING_DAYLIGHT_M
                && down >= SPRING_MIN_SLOPE_M_PER_KM
                && down - up >= SPRING_BREAK_M_PER_KM
            {
                springs[[y, x]] = true;
            }
            if d <= OASIS_DEPTH_M && arid(biomes[[y, x]], precip[[y, x]] as f64) {
                oases[[y, x]] = true;
            }
        }
    }
    DryWater { springs, oases }
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
    Band { name: "spring share of land %", sweet: (0.05, 4.0), hard: (0.005, 12.0), target: "M55: land cells where the table daylights at a break in slope · sweet 0.05–4% · hard 0.005–12%" },
    Band { name: "oasis share of arid land %", sweet: (0.2, 25.0), hard: (0.0, 60.0), target: "M55: arid cells standing over a table within root reach (8 m) · sweet 0.2–25% · hard 0–60%" },
    Band { name: "glacier-fed river share %", sweet: (0.5, 15.0), hard: (0.05, 40.0), target: "sweet 0.5–15 · hard 0.05–40 (M35: % of river cells carrying ≥25% accumulated melt — the ice keeps its rivers; measured 1.3–1.5 on three seeds)" },
    // M64 — calibration vs Earth: the shape of the river net. Horton's
    // bifurcation ratio is near-universal on Earth (Rb 3–5; Horton 1945,
    // Strahler 1957), measured on the full drainage tree at a fixed
    // support area — never on the render-pruned river mask, whose
    // missing headwaters read Rb≈1 off pure artifact. Drainage density
    // is scale-bound, so it is gated as a ratio against Hack-pruned
    // expectation over humid land only (≥400 mm — the 1.4 constant is a
    // humid-terrain figure): D_ref = 1.4/√A₅₀ km/km², with channel
    // length walked along true D8 steps (diagonals 4√2 km).
    Band { name: "horton bifurcation ratio", sweet: (2.8, 5.5), hard: (2.0, 8.0), target: "M64: Earth networks run Rb 3–5 (Horton 1945; Strahler 1957) — full tree at A_c 80 km²" },
    Band { name: "hack density ratio", sweet: (0.4, 2.5), hard: (0.15, 6.0), target: "M64: Dd·√A₅₀/1.4 ≈ 1 over humid land — density obeys Hack pruning at the map's channel threshold" },
];
