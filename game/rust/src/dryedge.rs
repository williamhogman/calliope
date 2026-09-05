//! M94 — the dry edge: steppe encroachment and oasis failure as dated,
//! extent-mapped events.
//!
//! The desert margin is not a line on the dawn map; it is a verdict the
//! sky renews every year. Two threshold laws live here, both read off
//! the same pointwise sky the harvests and the rivers already read
//! (`climate::year_anomaly_at`), both pure in seed × cell × year, both
//! with the hysteresis that separates a bad year from a changed country:
//!
//! * **The edge.** Every rain-fed land cell whose dawn rainfall stands at
//!   or above the pastoral aridity line (`hydrology::ARID_PRECIP_MM`,
//!   300 mm — research/08: below it a year cannot carry grazing without
//!   irrigation) but within reach of it (`EDGE_CEIL_MM`) is an *edge
//!   cell*. Each year its realized rain `p0 · (1 + dp)` is struck against
//!   the line. [`ENCROACH_YEARS`] consecutive years under it and the
//!   steppe takes the cell; [`RECOVER_YEARS`] consecutive years at or
//!   over it and the grass returns. The Sahel's own clock: the 1968–73
//!   run of failed rains turned pasture to dust in five years; the
//!   regreening of the 1990s came back in three.
//! * **The oasis.** Every grove the landform vocabulary names (`OASIS`:
//!   the strict low point of the M55 well-reach mask — the shallowest
//!   well) stands over a table `d0` metres down. A recharge mound of
//!   [`OASIS_MOUND_M`] rides on the regional base beneath it, and the
//!   mound is linear in recharge (Darcy): a sustained fractional deficit
//!   `δ` in the rain lowers the table by `OASIS_MOUND_M · δ`. The aquifer
//!   integrates — its `δ` is the mean over the last [`OASIS_MEMORY_YEARS`]
//!   years — so one dry year cannot kill a grove, a dry decade can. When
//!   the table sinks past the phreatophyte root reach
//!   (`hydrology::OASIS_DEPTH_M`, 8 m) plus a hysteresis margin the oasis
//!   fails; when it rises back within reach minus the margin, water
//!   stands again.
//!
//! Edge cells are grouped at the dawn into 8-connected *reaches* — the
//! named ground a chronicle entry speaks of. A reach speaks when the
//! standing extent the steppe holds first reaches [`EVENT_MIN_CELLS`]
//! (onset), when it doubles past the last spoken extent (widening), and
//! when it falls back to a quarter of the episode's peak (return). The
//! ledger keeps every year's change; the chronicle speaks only the turns.
//!
//! Determinism: the ledger is founded from the dawn grids in row-major
//! order, advanced once per year in the year's last month, and every
//! number in it is re-derivable from the seed — the harness does exactly
//! that re-derivation (`diagnose civ`). The path-dependent part (what is
//! taken, when, what was spoken) rides `hash()` into the replay identity
//! line; `law_hash()` leaves the names out so an independently driven
//! twin ledger can be compared to the live one.

use std::collections::HashSet;

use ndarray::Array2;

use crate::constants::{self as gc, KM_PER_CELL};
use crate::util::Band;

/// The pastoral aridity line, mm/y — one constant, shared with M55's
/// siting veto so the edge and the founding law cannot drift apart.
pub const PASTORAL_MM: f64 = crate::hydrology::ARID_PRECIP_MM;
/// Dawn rainfall above which a cell is out of the edge's reach: it would
/// take five consecutive years each ≥ 35 % short to bring 460 mm under
/// the line — three σ of the interannual law at the latitudes the steppe
/// lives at, five times running. Membership is a bound on work, not a
/// law of the world: nothing above it can be taken in practice.
pub const EDGE_CEIL_MM: f64 = 460.0;
/// Consecutive years under the line before the steppe takes a cell.
pub const ENCROACH_YEARS: u8 = 5;
/// Consecutive years at or over `RECOVER_MM` before the grass returns.
pub const RECOVER_YEARS: u8 = 3;
/// The rain a taken cell needs, each of `RECOVER_YEARS` years running,
/// before the grass comes back: a tenth over the line. Degraded pasture
/// does not return the year the rains do — the crust, the seed bank and
/// the grazing that stayed all lag it (the Sahel's cover trailed the
/// 1990s rains by years). Between the line and this the ground holds
/// whatever it holds: no run advances.
pub const RECOVER_MM: f64 = 330.0;
/// Standing extent (cells) a reach must hold before the chronicle
/// speaks its onset; 12 cells is 192 km² — a district, not a field.
/// Smaller takings are ledgered and mapped, never announced.
pub const EVENT_MIN_CELLS: u32 = 12;
/// A widening is spoken when the standing extent reaches this multiple
/// of the last spoken extent — log-spaced, so a slow creep speaks a
/// handful of times, never every year.
pub const WIDEN_FACTOR: f64 = 2.0;
/// The return is spoken when the standing extent falls to this share of
/// the episode's peak.
pub const RETURN_SHARE: f64 = 0.25;
/// km² per cell — the extent a spoken event carries.
pub const CELL_KM2: f64 = KM_PER_CELL * KM_PER_CELL;

/// Years the aquifer integrates: the mean fractional rain anomaly over
/// this window is the recharge deficit the table answers to.
pub const OASIS_MEMORY_YEARS: usize = 8;
/// Height of the recharge mound above the regional base beneath a
/// desert-margin grove, m. Mounds under wadis and dune fields run
/// 10–20 m (research/03); linear in recharge, so a sustained 30 %
/// deficit lowers the table 4.5 m.
pub const OASIS_MOUND_M: f64 = 15.0;
/// Hysteresis around the root reach, m: fails past `OASIS_DEPTH_M +`,
/// waters again within `OASIS_DEPTH_M −`.
pub const OASIS_HYSTERESIS_M: f64 = 1.0;
/// A grove fails only on a *run*: at least this many of the remembered
/// years must themselves have been under normal rain. The mound's mean
/// is the depth law; this is the sustained-dryness law beside it, so a
/// marginal grove one bad year deep never dies of that year alone.
pub const OASIS_DRY_MAJORITY: usize = 5;

/// One rain-fed cell within reach of the pastoral line.
#[derive(Clone, Debug)]
pub struct EdgeCell {
    pub x: u16,
    pub y: u16,
    /// Dawn rainfall, mm/y — the `p0` the year's fraction multiplies.
    pub p0: f32,
    /// The reach this cell belongs to (index into `DryEdge::reaches`).
    pub reach: u32,
    /// Consecutive years under the line, so far.
    pub run_dry: u8,
    /// Consecutive years at or over the line, so far.
    pub run_wet: u8,
    /// Whether the steppe holds this cell now.
    pub taken: bool,
    /// The year the current (or last) taking began; -1 never.
    pub taken_year: i32,
    /// How many times the cell has changed hands.
    pub flips: u16,
}

/// What a year's change to a reach was worth in the chronicle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Spoke {
    None = 0,
    Onset = 1,
    Widen = 2,
    Return = 3,
}

/// One year in which a reach's standing extent moved.
#[derive(Clone, Debug)]
pub struct EdgeRow {
    pub year: i64,
    /// Cells the steppe took this year.
    pub taken: u32,
    /// Cells the grass took back this year.
    pub released: u32,
    /// Standing extent after this year.
    pub taken_now: u32,
    /// Centroid of the standing extent (cells), the event's anchor.
    pub x: i64,
    pub y: i64,
    pub spoke: Spoke,
    /// Indices (into `DryEdge::cells`) of the cells taken this year —
    /// the extent the harness re-derives against the sky.
    pub cells: Vec<u32>,
}

/// A connected run of edge cells — the named ground of an entry.
#[derive(Clone, Debug, Default)]
pub struct Reach {
    pub id: u32,
    /// Coined when the reach first speaks; empty until then.
    pub name: String,
    /// The named place the reach was christened after.
    pub place: String,
    /// Centroid of the whole reach (cells).
    pub x: i64,
    pub y: i64,
    /// Member cells.
    pub cells: u32,
    /// Cells the steppe holds now.
    pub taken_now: u32,
    /// Peak standing extent of the open (or last) episode.
    pub peak: u32,
    /// Extent last spoken (onset or widening) in the open episode.
    pub spoken_extent: u32,
    pub episode_open: bool,
    /// The year the open (or last) episode was spoken.
    pub episode_year: i64,
    pub episodes: u16,
    pub rows: Vec<EdgeRow>,
}

/// One dated failure of a grove, and its return if it came.
#[derive(Clone, Debug)]
pub struct OasisEpisode {
    pub fail_year: i64,
    /// -1 while the oasis still lies dry.
    pub return_year: i64,
    /// Effective table depth at failure, m.
    pub depth_at_fail: f64,
    /// The recharge deficit (mean fractional anomaly) at failure.
    pub deficit_at_fail: f64,
}

/// A grove: the shallowest well of an arid reach.
#[derive(Clone, Debug)]
pub struct Oasis {
    pub id: u32,
    /// Coined when the grove first fails; empty until then.
    pub name: String,
    pub x: i64,
    pub y: i64,
    /// Dawn depth to the table, m.
    pub d0: f64,
    /// The last `OASIS_MEMORY_YEARS` fractional rain anomalies, newest
    /// first; filled from prehistory on the first pass.
    pub hist: Vec<f64>,
    /// Effective table depth after the last pass, m.
    pub depth: f64,
    pub failed: bool,
    pub episodes: Vec<OasisEpisode>,
}

/// The sky before the dawn, for the ledger's first pass: the founding
/// is a date in the chronicle, not a discontinuity in the rain. Per edge
/// cell the previous `ENCROACH_YEARS − 1` anomalies (oldest first), so a
/// run already four years deep can close in the dawn year; per grove
/// the previous `OASIS_MEMORY_YEARS` anomalies (newest first), so the
/// aquifer's memory starts full and a grove already dead before the
/// founding is dead — silently — at it.
#[derive(Clone, Debug, Default)]
pub struct Prehistory {
    pub edge: Vec<Vec<f64>>,
    pub oases: Vec<Vec<f64>>,
}

/// What a year's pass decided that the chronicle should hear.
#[derive(Clone, Debug)]
pub enum Speech {
    /// A reach turned: `row` indexes `reaches[reach].rows`.
    Edge { reach: u32, row: usize },
    /// A grove failed (`failed`) or watered again (`!failed`); `episode`
    /// indexes `oases[oasis].episodes`.
    Oasis { oasis: u32, episode: usize, failed: bool },
}

/// The ledger.
#[derive(Clone, Debug, Default)]
pub struct DryEdge {
    pub cells: Vec<EdgeCell>,
    pub reaches: Vec<Reach>,
    pub oases: Vec<Oasis>,
    /// Last year the pass ran; -1 at the dawn.
    pub last_year: i64,
    /// Whether the first pass has primed the clocks and the memory.
    pub primed: bool,
    /// Names this ledger has coined (reaches and groves).
    pub taken_names: HashSet<String>,
}

/// Biomes that are not the dry edge whatever their rain: the cold
/// margin's limit is frost, not rain, and the desert is already past
/// the line.
fn edge_biome(b: u8) -> bool {
    b != gc::WATER && b != gc::ICE && b != gc::TUNDRA && b != gc::WET_TUNDRA && b != gc::DESERT
}

impl DryEdge {
    /// Found the ledger from the dawn grids (post-widen, final
    /// coordinates). Row-major scans, so the cell and reach order is a
    /// pure function of the grids.
    pub fn found(
        height: &Array2<f32>,
        precip: &Array2<f32>,
        biomes: &Array2<u8>,
        near_fresh: &Array2<bool>,
        landform: &Array2<u8>,
        aquifer: &Array2<f32>,
    ) -> DryEdge {
        let (h, w) = height.dim();
        let mut cells: Vec<EdgeCell> = Vec::new();
        let mut index: Vec<i32> = vec![-1; h * w];
        for y in 0..h {
            for x in 0..w {
                if height[[y, x]] < 0.0 || near_fresh[[y, x]] || !edge_biome(biomes[[y, x]]) {
                    continue;
                }
                let p0 = precip[[y, x]] as f64;
                if !(p0 >= PASTORAL_MM && p0 < EDGE_CEIL_MM) {
                    continue;
                }
                index[y * w + x] = cells.len() as i32;
                cells.push(EdgeCell {
                    x: x as u16,
                    y: y as u16,
                    p0: p0 as f32,
                    reach: u32::MAX,
                    run_dry: 0,
                    run_wet: 0,
                    taken: false,
                    taken_year: -1,
                    flips: 0,
                });
            }
        }
        // 8-connected reaches, flood-filled in first-cell order.
        let mut reaches: Vec<Reach> = Vec::new();
        let mut stack: Vec<usize> = Vec::new();
        for i in 0..cells.len() {
            if cells[i].reach != u32::MAX {
                continue;
            }
            let id = reaches.len() as u32;
            let mut n = 0u32;
            let (mut sx, mut sy) = (0i64, 0i64);
            cells[i].reach = id;
            stack.push(i);
            while let Some(c) = stack.pop() {
                n += 1;
                let (cx, cy) = (cells[c].x as usize, cells[c].y as usize);
                sx += cx as i64;
                sy += cy as i64;
                for dy in -1isize..=1 {
                    for dx in -1isize..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let ny = cy as isize + dy;
                        let nx = cx as isize + dx;
                        if ny < 0 || nx < 0 || ny >= h as isize || nx >= w as isize {
                            continue;
                        }
                        let j = index[ny as usize * w + nx as usize];
                        if j >= 0 && cells[j as usize].reach == u32::MAX {
                            cells[j as usize].reach = id;
                            stack.push(j as usize);
                        }
                    }
                }
            }
            reaches.push(Reach {
                id,
                x: (sx as f64 / n as f64).round() as i64,
                y: (sy as f64 / n as f64).round() as i64,
                cells: n,
                ..Default::default()
            });
        }
        // The groves: every OASIS landform word, one oasis each.
        let mut oases: Vec<Oasis> = Vec::new();
        for y in 0..h {
            for x in 0..w {
                if landform[[y, x]] != crate::landform::OASIS {
                    continue;
                }
                oases.push(Oasis {
                    id: oases.len() as u32,
                    name: String::new(),
                    x: x as i64,
                    y: y as i64,
                    d0: aquifer[[y, x]] as f64,
                    hist: Vec::new(),
                    depth: aquifer[[y, x]] as f64,
                    failed: false,
                    episodes: Vec::new(),
                });
            }
        }
        DryEdge { cells, reaches, oases, last_year: -1, primed: false, taken_names: HashSet::new() }
    }

    /// Whether the oasis histories still need their prehistory fill.
    pub fn primed(&self) -> bool {
        self.primed
    }

    /// The year's realized rain at an edge cell, mm.
    #[inline]
    pub fn realized_mm(p0: f64, dp: f64) -> f64 {
        p0 * (1.0 + dp)
    }

    /// The effective table depth under a grove, m, from its dawn depth
    /// and the recharge deficit the aquifer remembers: the mound rises
    /// and falls linearly with the mean anomaly. Never negative.
    #[inline]
    pub fn oasis_depth(d0: f64, deficit: f64) -> f64 {
        (d0 - OASIS_MOUND_M * deficit).max(0.0)
    }

    /// How many of a grove's remembered years were under normal rain.
    #[inline]
    pub fn dry_years(hist: &[f64]) -> usize {
        hist.iter().filter(|&&d| d < 0.0).count()
    }

    /// The mean of a grove's remembered anomalies.
    #[inline]
    pub fn oasis_deficit(hist: &[f64]) -> f64 {
        if hist.is_empty() {
            0.0
        } else {
            hist.iter().sum::<f64>() / hist.len() as f64
        }
    }

    /// Advance one year. `edge_dp[i]` is the year's fractional rain
    /// anomaly at `cells[i]`; `oasis_dp[k]` at `oases[k]`; `pre` is the
    /// sky before the dawn, read only on the first pass.
    pub fn advance(
        &mut self,
        year: i64,
        edge_dp: &[f64],
        oasis_dp: &[f64],
        pre: Option<&Prehistory>,
    ) -> Vec<Speech> {
        let mut speech = Vec::new();
        if self.last_year >= year {
            return speech;
        }
        let prime = !self.primed();
        self.last_year = year;
        debug_assert_eq!(edge_dp.len(), self.cells.len());
        debug_assert_eq!(oasis_dp.len(), self.oases.len());

        // ---- the dawn: prime the clocks and the memory ------------------
        if prime {
            if let Some(pre) = pre {
                for (i, c) in self.cells.iter_mut().enumerate() {
                    for &dp in pre.edge.get(i).map(|v| v.as_slice()).unwrap_or(&[]) {
                        let p = Self::realized_mm(c.p0 as f64, dp);
                        if p < PASTORAL_MM {
                            c.run_dry = c.run_dry.saturating_add(1);
                            c.run_wet = 0;
                        } else if p >= RECOVER_MM {
                            c.run_wet = c.run_wet.saturating_add(1);
                            c.run_dry = 0;
                        } else {
                            c.run_dry = 0;
                            c.run_wet = 0;
                        }
                    }
                    // the clocks are primed one year short of a taking:
                    // nothing is taken before the dawn, so the dawn year
                    // is the first that can close a run.
                    c.run_dry = c.run_dry.min(ENCROACH_YEARS - 1);
                    c.run_wet = c.run_wet.min(RECOVER_YEARS - 1);
                }
                for (k, o) in self.oases.iter_mut().enumerate() {
                    let mut h: Vec<f64> = pre.oases.get(k).cloned().unwrap_or_default();
                    h.truncate(OASIS_MEMORY_YEARS);
                    // a grove already past the line on the eve of the
                    // dawn is failed at it, unspoken: its episode is
                    // dated to the year before, and re-derives there.
                    let deficit = Self::oasis_deficit(&h);
                    let depth = Self::oasis_depth(o.d0, deficit);
                    let line = crate::hydrology::OASIS_DEPTH_M;
                    if h.len() == OASIS_MEMORY_YEARS
                        && Self::dry_years(&h) >= OASIS_DRY_MAJORITY
                        && depth > line + OASIS_HYSTERESIS_M
                    {
                        o.failed = true;
                        o.episodes.push(OasisEpisode {
                            fail_year: year - 1,
                            return_year: -1,
                            depth_at_fail: depth,
                            deficit_at_fail: deficit,
                        });
                    }
                    o.depth = depth;
                    o.hist = h;
                }
            }
            // with or without a prehistory, the memory now counts as
            // primed: an empty history is a full history of zeros.
            for o in self.oases.iter_mut() {
                if o.hist.is_empty() {
                    o.hist = vec![0.0; OASIS_MEMORY_YEARS];
                }
            }
            self.primed = true;
        }

        // ---- the edge --------------------------------------------------
        // Per reach: cells taken this year (in cell order), released count.
        let nr = self.reaches.len();
        let mut taken_by: Vec<Vec<u32>> = vec![Vec::new(); nr];
        let mut released_by: Vec<u32> = vec![0; nr];
        for (i, c) in self.cells.iter_mut().enumerate() {
            let p = Self::realized_mm(c.p0 as f64, edge_dp[i]);
            if p < PASTORAL_MM {
                c.run_dry = c.run_dry.saturating_add(1);
                c.run_wet = 0;
                if !c.taken && c.run_dry >= ENCROACH_YEARS {
                    c.taken = true;
                    c.taken_year = year as i32;
                    c.flips = c.flips.saturating_add(1);
                    taken_by[c.reach as usize].push(i as u32);
                }
            } else if p >= RECOVER_MM {
                c.run_wet = c.run_wet.saturating_add(1);
                c.run_dry = 0;
                if c.taken && c.run_wet >= RECOVER_YEARS {
                    c.taken = false;
                    c.flips = c.flips.saturating_add(1);
                    released_by[c.reach as usize] += 1;
                }
            } else {
                // between the line and the recovery mark: the ground
                // holds what it holds, and neither clock runs.
                c.run_dry = 0;
                c.run_wet = 0;
            }
        }
        // Standing centroids for the reaches that moved.
        let mut moved: Vec<usize> = (0..nr)
            .filter(|&r| !taken_by[r].is_empty() || released_by[r] > 0)
            .collect();
        moved.sort_unstable();
        let mut cen: Vec<(i64, i64, u32)> = vec![(0, 0, 0); nr];
        if !moved.is_empty() {
            for c in &self.cells {
                if c.taken {
                    let e = &mut cen[c.reach as usize];
                    e.0 += c.x as i64;
                    e.1 += c.y as i64;
                    e.2 += 1;
                }
            }
        }
        for r in moved {
            let reach = &mut self.reaches[r];
            let taken = taken_by[r].len() as u32;
            let released = released_by[r];
            let now = reach.taken_now + taken - released;
            reach.taken_now = now;
            let (sx, sy, n) = cen[r];
            let (ax, ay) = if n > 0 {
                ((sx as f64 / n as f64).round() as i64, (sy as f64 / n as f64).round() as i64)
            } else {
                (reach.x, reach.y)
            };
            let mut spoke = Spoke::None;
            if !reach.episode_open {
                if taken > 0 && now >= EVENT_MIN_CELLS {
                    reach.episode_open = true;
                    reach.episodes = reach.episodes.saturating_add(1);
                    reach.peak = now;
                    reach.spoken_extent = now;
                    reach.episode_year = year;
                    spoke = Spoke::Onset;
                }
            } else {
                reach.peak = reach.peak.max(now);
                if taken > 0 && (now as f64) >= WIDEN_FACTOR * reach.spoken_extent as f64 {
                    reach.spoken_extent = now;
                    spoke = Spoke::Widen;
                } else if released > 0 && (now as f64) <= RETURN_SHARE * reach.peak as f64 {
                    reach.episode_open = false;
                    reach.spoken_extent = 0;
                    spoke = Spoke::Return;
                }
            }
            reach.rows.push(EdgeRow {
                year,
                taken,
                released,
                taken_now: now,
                x: ax,
                y: ay,
                spoke,
                cells: std::mem::take(&mut taken_by[r]),
            });
            if spoke != Spoke::None {
                speech.push(Speech::Edge { reach: r as u32, row: reach.rows.len() - 1 });
            }
        }

        // ---- the groves -------------------------------------------------
        for (k, o) in self.oases.iter_mut().enumerate() {
            o.hist.insert(0, oasis_dp[k]);
            o.hist.truncate(OASIS_MEMORY_YEARS);
            let deficit = Self::oasis_deficit(&o.hist);
            o.depth = Self::oasis_depth(o.d0, deficit);
            let line = crate::hydrology::OASIS_DEPTH_M;
            let dry_run = Self::dry_years(&o.hist) >= OASIS_DRY_MAJORITY;
            if !o.failed && dry_run && o.depth > line + OASIS_HYSTERESIS_M {
                o.failed = true;
                o.episodes.push(OasisEpisode {
                    fail_year: year,
                    return_year: -1,
                    depth_at_fail: o.depth,
                    deficit_at_fail: deficit,
                });
                speech.push(Speech::Oasis { oasis: k as u32, episode: o.episodes.len() - 1, failed: true });
            } else if o.failed && o.depth < line - OASIS_HYSTERESIS_M {
                o.failed = false;
                if let Some(ep) = o.episodes.last_mut() {
                    ep.return_year = year;
                }
                speech.push(Speech::Oasis { oasis: k as u32, episode: o.episodes.len() - 1, failed: false });
            }
        }
        speech
    }

    /// The dawn geometry with the history struck: for the harness's twin
    /// replay (same cells, same reaches, same groves; nothing taken,
    /// nothing remembered, nothing spoken).
    pub fn rewound(&self) -> DryEdge {
        let mut t = self.clone();
        for c in t.cells.iter_mut() {
            c.run_dry = 0;
            c.run_wet = 0;
            c.taken = false;
            c.taken_year = -1;
            c.flips = 0;
        }
        for r in t.reaches.iter_mut() {
            r.name.clear();
            r.place.clear();
            r.taken_now = 0;
            r.peak = 0;
            r.spoken_extent = 0;
            r.episode_open = false;
            r.episode_year = 0;
            r.episodes = 0;
            r.rows.clear();
        }
        for o in t.oases.iter_mut() {
            o.name.clear();
            o.hist.clear();
            o.depth = o.d0;
            o.failed = false;
            o.episodes.clear();
        }
        t.last_year = -1;
        t.primed = false;
        t.taken_names.clear();
        t
    }

    /// Cells the steppe holds now.
    pub fn taken_count(&self) -> usize {
        self.cells.iter().filter(|c| c.taken).count()
    }

    /// Cells that have ever been taken.
    pub fn ever_taken(&self) -> usize {
        self.cells.iter().filter(|c| c.flips > 0).count()
    }

    /// Whether the steppe holds this cell (M99 reads this).
    pub fn taken_at(&self, x: usize, y: usize) -> bool {
        self.cells.iter().any(|c| c.taken && c.x as usize == x && c.y as usize == y)
    }

    /// Groves lying dry now.
    pub fn failed_count(&self) -> usize {
        self.oases.iter().filter(|o| o.failed).count()
    }

    /// Spoken turns of the edge, by kind.
    pub fn spoken(&self) -> (usize, usize, usize) {
        let mut n = (0, 0, 0);
        for r in &self.reaches {
            for row in &r.rows {
                match row.spoke {
                    Spoke::Onset => n.0 += 1,
                    Spoke::Widen => n.1 += 1,
                    Spoke::Return => n.2 += 1,
                    Spoke::None => {}
                }
            }
        }
        n
    }

    /// Every path-dependent number except the names, in a fixed order.
    pub fn law_hash(&self) -> u64 {
        let mut s = String::new();
        s.push_str(&format!("y{}|{}|{}|{}\n", self.last_year, self.cells.len(), self.reaches.len(), self.oases.len()));
        for c in &self.cells {
            if c.flips > 0 || c.run_dry > 0 {
                s.push_str(&format!("c{}|{}|{}|{}|{}|{}\n", c.x, c.y, c.run_dry, c.taken as u8, c.taken_year, c.flips));
            }
        }
        for r in &self.reaches {
            if r.rows.is_empty() {
                continue;
            }
            s.push_str(&format!("r{}|{}|{}|{}|{}|{}\n", r.id, r.taken_now, r.peak, r.episode_open as u8, r.episode_year, r.episodes));
            for row in &r.rows {
                s.push_str(&format!("w{}|{}|{}|{}|{}|{}|{}\n", row.year, row.taken, row.released, row.taken_now, row.x, row.y, row.spoke as u8));
            }
        }
        for o in &self.oases {
            s.push_str(&format!("o{}|{:.6}|{}\n", o.id, o.depth, o.failed as u8));
            for e in &o.episodes {
                s.push_str(&format!("e{}|{}|{:.6}|{:.6}\n", e.fail_year, e.return_year, e.depth_at_fail, e.deficit_at_fail));
            }
        }
        crate::util::fnv1a64(s.as_bytes())
    }

    /// Everything path-dependent, names included — the replay identity line.
    pub fn hash(&self) -> u64 {
        let mut s = format!("{:016x}\n", self.law_hash());
        for r in &self.reaches {
            if !r.name.is_empty() {
                s.push_str(&format!("n{}|{}|{}\n", r.id, r.name, r.place));
            }
        }
        for o in &self.oases {
            if !o.name.is_empty() {
                s.push_str(&format!("m{}|{}\n", o.id, o.name));
            }
        }
        crate::util::fnv1a64(s.as_bytes())
    }
}

/// M94 — diagnostics bands for the dry edge.
pub const BANDS: &[Band] = &[
    Band { name: "dry edge share of land", sweet: (0.01, 0.20), hard: (0.002, 0.40), target: "M94: rain-fed land cells within reach of the 300 mm pastoral line (300–460 mm) as a share of land — a margin, not a continent" },
    Band { name: "steppe takings per edge-century", sweet: (0.2, 15.0), hard: (0.02, 40.0), target: "M94: cells taken per 100 edge cells per century — the Sahel lost ~10 % of its pasture in the 1968–85 run; a quiet century takes less than one in a hundred" },
    Band { name: "dry edge turns per century", sweet: (0.5, 40.0), hard: (0.1, 90.0), target: "M94: spoken reach onsets + widenings + returns and grove failures + returns per 100 y — the chronicle volume of the named droughts (M80: 3–40 sweet), dated turns a generation apart per place, never one a year per reach" },
    Band { name: "steppe hold years", sweet: (4.0, 60.0), hard: (3.0, 150.0), target: "M94: mean years a taken cell stays under steppe before the grass returns — longer than the recovery clock, shorter than the run" },
];
