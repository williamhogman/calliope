//! M99 — *Steppe Pressure*: the herds and the plough.
//!
//! Between the desert and the sown lies the grass: ground too dry for a
//! furrow and too green to be nothing, where a people that moves its
//! wealth on the hoof can live and a people that ploughs cannot. This
//! leaf owns that ground as a *field* — one byte per cell, re-read each
//! year — and the one political consequence the roadmap asks of it: when
//! the sky moves the grass onto ploughed land, the herds and the farmers
//! meet, and the crown that holds those fields feels it as unrest.
//!
//! Two lanes, one window. The window is the pastoral belt the crop law
//! already draws (`agriculture::package_at`): rain from the desert line
//! (`RANGE_P_MIN`, the trapezoid's lower knee) to the dry veto that hands
//! the ground to grain (`RANGE_P_MAX`), and an annual mean warm enough
//! that the grass is not under snow for the herd's lean season
//! (`RANGE_T_MIN`). The *century lane* reads that window under the
//! composed forcing alone — `World::year_forcing`, the M83 drift with the
//! M86/M87 ages on it — through the same two couplings the rest of the
//! sky uses: the forcing as degrees on the mean (M90's law) and the M84
//! belt walk as the fractional change on the rain. That is the **range**:
//! the ground the century has made grass; a pure function of the
//! forcing, so under a flat sky it is the dawn range and never moves.
//!
//! The *decade lane* reads the same window under the sky that actually
//! fell over the last ten years (`climate::year_anomaly_at`, decade-
//! averaged on a coarse lattice; the composed anomaly already carries the
//! forcing on its warmth lane and the belt walk on its rain lane, so the
//! decade stands *on* the century, not beside it): that is where the
//! **herds** are. A hard decade does two things to a herd. At the desert
//! edge the grass fails and the herd leaves it; and the herd does not
//! die of that — it moves *outward*, toward the wetter ground it would
//! not trouble in a good year, the farmers' side of the line (`PUSH`:
//! the pastoral line the herds respect rises with the decade's *want* —
//! its rain and warmth short of the century's own, the weather and not
//! the climate, since a cold century is the herd's new normal and not a
//! failing). A kind decade does the opposite: the desert edge greens and
//! takes the herds back out onto it, and the wet edge retracts to the
//! grass proper. So cold and dry decades push the footprint onto the
//! sown ground and wet ones draw it home, and the whole footprint stays
//! within a tenth of the century's range.
//!
//! Capacity is grass: a herds cell feeds `HEADS_PER_KM2` souls per km²
//! (the pastoral package's own dawn density) scaled by how far its
//! decade rain stands above the desert line.
//!
//! Pressure is *the drift's* overlap. The herds always stood at the
//! sown ground's edge in a hard decade — that is the old border, and it
//! is not news. What is news is the ground the century has moved them
//! onto: a cell is **frontier** when the herds hold it this decade and
//! would not hold it under this same decade over a sky that had never
//! drifted — the same ten years drawn with the drift at zero
//! (`year_anomaly_at`'s unforced twin), the same want, the same pushed
//! line; computed beside the verdict for every cell every year. Under a
//! flat sky the two decades are the same numbers and the frontier is
//! empty by construction — the metamorphic law the roadmap asks for,
//! held in the definition rather than in a gate. A
//! frontier cell under a grain package is *contested*; each realm's
//! share of contested fields over its towns' hinterlands (`pressure`) is
//! written once a year and read every month by the unrest gauge
//! (`PRESSURE_UNREST` at `PRESSURE_FULL`, the weight of every town in
//! the realm going hungry); when the share first crosses `PRESSURE_LINE`,
//! or has grown after a `PRESSURE_REST` of years, the chronicle speaks
//! one dated `frontier-pressure` event for the realm, anchored at its
//! most contested town.
//!
//! Everything here is a pure function of (seed, year, the field grids,
//! who holds what): no die, no wall-clock. The field, the standings and
//! the realm marks are state and ride the replay identity line
//! (`Steppe::hash`); the decade rings are a cache of a pure sky and are
//! not.

use ndarray::Array2;

use crate::agriculture::CropPackage;
use crate::constants::KM_PER_CELL;
use crate::util::{fnv1a64, Band};

// ---------------------------------------------------------------- the law

/// Annual mean below which the grass lies under snow too long for a herd
/// to winter — the cold edge of the range, °C. Steppe pastoralism on
/// Earth runs to about −1 °C annual mean (the Mongolian plateau's warmer
/// half); the tundra beyond it is reindeer country and another life.
pub const RANGE_T_MIN: f64 = -1.0;
/// The desert line, mm/yr — the pastoral trapezoid's lower knee in
/// `agriculture::climatic_score`. Below it there is nothing to graze.
pub const RANGE_P_MIN: f64 = 110.0;
/// The pastoral line, mm/yr — `agriculture::package_at`'s dry veto: at
/// or above it the ground can be sown, and grain takes it from the herd.
pub const RANGE_P_MAX: f64 = 300.0;
/// Years the herds remember: the decade lane averages this many.
pub const DECADE: i64 = 10;
/// Cells per side of the lattice the decade sky is read on. The herd
/// answers a region's decade, not one cell's — and the lattice keeps the
/// yearly cost to a few thousand draws.
pub const BLOCK: usize = 4;
/// Candidate reach around the cold edge, °C: cells colder than
/// `RANGE_T_MIN − SKY_REACH_T` at the dawn are beyond any decade the sky
/// produces and are never read.
pub const SKY_REACH_T: f64 = 8.0;
/// Candidate reach around the rain edges, as a fraction: cells wetter
/// than `RANGE_P_MAX / (1 − SKY_REACH_P)` or drier than
/// `RANGE_P_MIN / (1 + SKY_REACH_P)` at the dawn are never read.
pub const SKY_REACH_P: f64 = 0.5;
/// Souls a full-grass herds cell feeds per km² — the pastoral package's
/// dawn-age density (research/08: pastoral 2 souls/km²).
pub const HEADS_PER_KM2: f64 = 2.0;
/// How far the pastoral line the herds respect rises per unit of the
/// decade's want (`loss`): a decade a tenth short of rain lifts it by
/// `PUSH · 0.1` — the herds walk that much further onto the sown side.
pub const PUSH: f64 = 0.4;
/// Degrees of a decade's cold that count as one unit of want beside a
/// full share of missing rain.
pub const PUSH_T_SCALE: f64 = 3.0;
/// Share of a realm's fields under the herds at which the frontier
/// speaks: one furrow in fifty contested is a quarrel the court hears.
pub const PRESSURE_LINE: f64 = 0.02;
/// Share at which the unrest term stands at full strength.
pub const PRESSURE_FULL: f64 = 0.10;
/// Monthly unrest at full pressure — the same weight `politics::monthly`
/// gives every town in the realm going hungry.
pub const PRESSURE_UNREST: f64 = 0.020;
/// Years before a realm's frontier is spoken of again, and then only if
/// the pressure has grown since it was last told.
pub const PRESSURE_REST: i64 = 20;
/// Hinterland radius the pressure is read over, cells — the same ring
/// the roads' law (M98) counts a town's fields on.
pub const HINTERLAND_R: i64 = crate::migration::HINTERLAND_R;
/// "Never" for the year marks.
pub const NEVER: i64 = i64::MIN;

/// Field bits — one byte per cell, three lanes.
pub const NONE: u8 = 0;
/// The century says grass here (the range).
pub const RANGE: u8 = 1;
/// The herds hold it this decade.
pub const HERDS: u8 = 2;
/// The herds hold it this decade and would not under a sky that never
/// drifted — the drift's own ground.
pub const FRONTIER: u8 = 4;

/// The century's grass: `RANGE` set.
#[inline]
pub fn is_range(code: u8) -> bool {
    code & RANGE != 0
}
/// Under the herds: `HERDS` set.
#[inline]
pub fn is_herds(code: u8) -> bool {
    code & HERDS != 0
}
/// Herds beyond the century's range — the decade's overreach.
#[inline]
pub fn is_over(code: u8) -> bool {
    code & (RANGE | HERDS) == HERDS
}
/// The drift's ground: `FRONTIER` set.
#[inline]
pub fn is_frontier(code: u8) -> bool {
    code & FRONTIER != 0
}

/// Signed latitude of a row, degrees — the same expression the sky uses.
#[inline]
pub fn lat_signed(rows: usize, y: usize) -> f64 {
    -90.0 + (y as f64) * 180.0 / (rows as f64 - 1.0)
}

/// The window: pastoral ground at effective warmth `t` and rain `p`.
#[inline]
pub fn in_window(t: f64, p: f64) -> bool {
    t >= RANGE_T_MIN && p >= RANGE_P_MIN && p < RANGE_P_MAX
}

/// The century lane at one cell: range under the composed forcing `f`,
/// the forcing as degrees on the dawn mean and the M84 belt walk as the
/// fractional change on the dawn rain. Exactly the dawn verdict at `f = 0`.
#[inline]
pub fn range_at(t: f64, p: f64, lat_s: f64, f: f64) -> bool {
    in_window(t + f, p * (1.0 + crate::climate::belt_anomaly(lat_s, f)))
}

/// A decade's want: the share of rain it fell short by and the degrees
/// of cold it brought, each clamped at nothing when the decade was kind.
#[inline]
pub fn loss(dt: f64, dp: f64) -> f64 {
    (-dp).max(0.0) + (-dt).max(0.0) / PUSH_T_SCALE
}

/// The pastoral line the herds respect under a decade of want: the dry
/// veto lifted by `PUSH · loss` — the outward push onto the sown side.
#[inline]
pub fn herds_line_mm(dt: f64, dp: f64) -> f64 {
    RANGE_P_MAX * (1.0 + PUSH * loss(dt, dp))
}

/// The decade lane at one cell: herds under a decade's composed sky —
/// `dt` degrees on the dawn mean, `dp` the fractional change on the dawn
/// rain (the forcing and the belt walk ride inside them) — against the
/// pastoral line `line_mm` the decade's want has set. The cold edge and
/// the desert edge are the window's own; the wet edge is the pushed line.
#[inline]
pub fn herds_at(t: f64, p: f64, dt: f64, dp: f64, line_mm: f64) -> bool {
    let p_dec = p * (1.0 + dp);
    t + dt >= RANGE_T_MIN && p_dec >= RANGE_P_MIN && p_dec < line_mm
}

/// The field byte from the three lanes: the century's range, the herds
/// under the century, and the herds under the flat-sky twin — the
/// frontier is the first without the second.
#[inline]
pub fn verdict(range: bool, herds: bool, herds_flat: bool) -> u8 {
    (if range { RANGE } else { 0 }) | (if herds { HERDS } else { 0 }) | (if herds && !herds_flat { FRONTIER } else { 0 })
}

/// How much grass a decade's rain grows on a range cell, 0..1 from the
/// desert line to the pastoral line.
#[inline]
pub fn grass(p_eff: f64) -> f64 {
    ((p_eff - RANGE_P_MIN) / (RANGE_P_MAX - RANGE_P_MIN)).clamp(0.0, 1.0)
}

/// Souls one herds cell feeds under the decade's rain.
#[inline]
pub fn cell_capacity(p: f64, dp: f64) -> f64 {
    HEADS_PER_KM2 * KM_PER_CELL * KM_PER_CELL * grass(p * (1.0 + dp))
}

/// A grain package — the plough's ground.
#[inline]
pub fn is_grain(code: u8) -> bool {
    matches!(
        CropPackage::from_code(code),
        CropPackage::Wheat | CropPackage::Rice | CropPackage::Maize
    )
}

/// Whether a cell is *contested*: the drift's herds stand on ploughed
/// ground. Under a flat sky no cell is frontier, so this is false
/// everywhere by construction.
#[inline]
pub fn contested(code: u8, crop_now: u8) -> bool {
    is_frontier(code) && is_grain(crop_now)
}

/// The pressure a realm's fields are under: contested over farmed.
#[inline]
pub fn pressure(overlap: usize, farmed: usize) -> f64 {
    if farmed == 0 {
        0.0
    } else {
        overlap as f64 / farmed as f64
    }
}

/// The monthly unrest the pressure adds, read by `politics::monthly`.
#[inline]
pub fn unrest_term(p: f64) -> f64 {
    PRESSURE_UNREST * (p / PRESSURE_FULL).min(1.0)
}

/// One realm's frontier memory: when it was last spoken of, at what
/// share, and whether it is armed to speak on the next crossing.
#[derive(Clone, Copy, Debug)]
pub struct RealmMark {
    pub spoken: i64,
    pub share: f64,
    pub armed: bool,
}

impl Default for RealmMark {
    fn default() -> Self {
        RealmMark { spoken: NEVER, share: 0.0, armed: true }
    }
}

/// Whether the frontier speaks for a realm this year: on the crossing of
/// `PRESSURE_LINE` while armed, or after `PRESSURE_REST` years if the
/// share has grown since it was last told. Pure in (mark, year, share).
#[inline]
pub fn speaks(mark: &RealmMark, year: i64, share: f64) -> bool {
    if share < PRESSURE_LINE {
        return false;
    }
    if mark.armed {
        return true;
    }
    mark.spoken != NEVER && year - mark.spoken >= PRESSURE_REST && share > mark.share
}

// ---------------------------------------------------------------- state

/// One cell the sky can move across the window's edges within reach.
#[derive(Clone, Copy, Debug)]
pub struct RangeCell {
    pub y: u32,
    pub x: u32,
    /// Dawn annual mean, °C, and dawn rain, mm — the grids' own numbers.
    pub t: f64,
    pub p: f64,
    /// Index into `Steppe::blocks`.
    pub block: u32,
}

/// One lattice block's decade sky. The ring is a cache of a pure
/// function (cell × year → anomaly), keyed by year so any chunking of
/// the tick fills it to the same numbers; it is never hashed.
/// One year of one block's sky: the composed anomaly and its unforced
/// twin — the same draw with the drift at zero.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyYear {
    pub dt: f64,
    pub dp: f64,
    pub dt0: f64,
    pub dp0: f64,
}

impl SkyYear {
    pub const ZERO: SkyYear = SkyYear { dt: 0.0, dp: 0.0, dt0: 0.0, dp0: 0.0 };
}

#[derive(Clone, Debug)]
pub struct SkyBlock {
    pub x: u32,
    pub y: u32,
    ring: [(i64, SkyYear); DECADE as usize],
    /// The decade means the last `read` produced: the composed sky and
    /// its unforced twin.
    pub sky: SkyYear,
}

impl SkyBlock {
    fn new(x: u32, y: u32) -> Self {
        SkyBlock { x, y, ring: [(NEVER, SkyYear::ZERO); DECADE as usize], sky: SkyYear::ZERO }
    }

    /// The decade ending in `year`, oldest year first, each year drawn
    /// once and kept. Summation order is fixed (oldest first) so the
    /// harness's own ten-draw mean lands on the same bits.
    pub fn read(&mut self, year: i64, draw: &mut dyn FnMut(usize, usize, i64) -> SkyYear) {
        let mut acc = [0.0f64; 4];
        for k in (0..DECADE).rev() {
            let yr = year - k;
            let slot = yr.rem_euclid(DECADE) as usize;
            if self.ring[slot].0 != yr {
                self.ring[slot] = (yr, draw(self.x as usize, self.y as usize, yr));
            }
            let s = self.ring[slot].1;
            acc[0] += s.dt;
            acc[1] += s.dp;
            acc[2] += s.dt0;
            acc[3] += s.dp0;
        }
        let n = DECADE as f64;
        self.sky = SkyYear { dt: acc[0] / n, dp: acc[1] / n, dt0: acc[2] / n, dp0: acc[3] / n };
    }
}

/// The decade mean the harness re-derives without the ring: the same
/// ten draws in the same order.
pub fn decade_mean(year: i64, mut draw: impl FnMut(i64) -> SkyYear) -> SkyYear {
    let mut acc = [0.0f64; 4];
    for k in (0..DECADE).rev() {
        let s = draw(year - k);
        acc[0] += s.dt;
        acc[1] += s.dp;
        acc[2] += s.dt0;
        acc[3] += s.dp0;
    }
    let n = DECADE as f64;
    SkyYear { dt: acc[0] / n, dp: acc[1] / n, dt0: acc[2] / n, dp0: acc[3] / n }
}

/// One year of the field: what the pass read and what it found.
/// Diagnostics observation, never hashed.
#[derive(Clone, Debug)]
pub struct RangeRow {
    pub year: i64,
    /// The composed forcing the century lane read.
    pub f: f64,
    /// The decade sky the herds read, averaged over the lattice: the
    /// composed anomaly (degrees on the mean, fractional change on the
    /// rain) and the want — its unforced twin, the weather alone.
    pub sky: SkyYear,
    /// Cells: the century range, the herds, the herds beyond the range,
    /// and the drift's frontier ground.
    pub range: usize,
    pub herds: usize,
    pub over: usize,
    pub frontier: usize,
    /// Souls the herds' ground feeds this year.
    pub capacity: f64,
    /// Fields counted over every town's hinterland, and the contested ones.
    pub farmed: usize,
    pub overlap: usize,
    /// Realms at or over the line, and the tellings this year.
    pub pressed: usize,
    pub spoken: usize,
    /// The hardest-pressed realm's share this year.
    pub max_share: f64,
}

/// One telling: the realm, the town it was anchored at, the share.
#[derive(Clone, Debug)]
pub struct PressureRow {
    pub m: i64,
    pub year: i64,
    pub realm: usize,
    pub realm_name: String,
    pub town: String,
    pub x: i64,
    pub y: i64,
    pub share: f64,
    pub overlap: usize,
    pub farmed: usize,
    pub f: f64,
}

/// The steppe: the range cells, the sky lattice, the field and its
/// standings, the realm marks, and the two observation ledgers.
#[derive(Clone, Debug)]
pub struct Steppe {
    pub cells: Vec<RangeCell>,
    pub blocks: Vec<SkyBlock>,
    /// The field: `RANGE` · `HERDS` · `FRONTIER` bits per cell.
    pub field: Array2<u8>,
    /// The last year the field was read for; `NEVER` before the first.
    pub year: i64,
    /// The forcing the field stands at.
    pub sky: f64,
    /// The decade sky the field stands at, averaged over the lattice.
    pub sky_decade: SkyYear,
    pub range: usize,
    pub herds: usize,
    pub over: usize,
    pub frontier: usize,
    pub capacity: f64,
    /// The century range at the dawn (forcing 0), cells.
    pub dawn_range: usize,
    /// Per realm: the frontier memory, and this year's pressure.
    pub marks: Vec<RealmMark>,
    pub pressure: Vec<f64>,
    pub log: Vec<RangeRow>,
    pub ledger: Vec<PressureRow>,
}

impl Default for Steppe {
    fn default() -> Self {
        Steppe {
            cells: Vec::new(),
            blocks: Vec::new(),
            field: Array2::zeros((0, 0)),
            year: NEVER,
            sky: 0.0,
            sky_decade: SkyYear::ZERO,
            range: 0,
            herds: 0,
            over: 0,
            frontier: 0,
            capacity: 0.0,
            dawn_range: 0,
            marks: Vec::new(),
            pressure: Vec::new(),
            log: Vec::new(),
            ledger: Vec::new(),
        }
    }
}

impl Steppe {
    /// Found the steppe off the dawn grids: every land cell the sky could
    /// carry across the window within reach becomes a range cell on a
    /// `BLOCK`-cell lattice; the field is written at forcing 0 with the
    /// herds on every range cell (the first yearly pass reads the real
    /// decade). Pure in the grids.
    pub fn found(height: &Array2<f32>, tmean: &Array2<f32>, precip: &Array2<f32>, flags: &Array2<u8>) -> Self {
        let lake_bit = crate::state::CellFlags::LAKE.bits();
        let (rows, cols) = height.dim();
        let p_lo = RANGE_P_MIN / (1.0 + SKY_REACH_P);
        let p_hi = RANGE_P_MAX / (1.0 - SKY_REACH_P);
        let t_lo = RANGE_T_MIN - SKY_REACH_T;
        let mut cells = Vec::new();
        let mut blocks: Vec<SkyBlock> = Vec::new();
        let mut block_of: std::collections::HashMap<(u32, u32), u32> = Default::default();
        let mut field = Array2::<u8>::zeros((rows, cols));
        let mut dawn_range = 0usize;
        for y in 0..rows {
            let lat = lat_signed(rows, y);
            for x in 0..cols {
                if height[[y, x]] < 0.0 || flags[[y, x]] & lake_bit != 0 {
                    continue;
                }
                let (t, p) = (tmean[[y, x]] as f64, precip[[y, x]] as f64);
                if t < t_lo || p < p_lo || p >= p_hi {
                    continue;
                }
                let key = ((y / BLOCK) as u32, (x / BLOCK) as u32);
                let block = *block_of.entry(key).or_insert_with(|| {
                    let by = ((key.0 as usize) * BLOCK + BLOCK / 2).min(rows - 1);
                    let bx = ((key.1 as usize) * BLOCK + BLOCK / 2).min(cols - 1);
                    blocks.push(SkyBlock::new(bx as u32, by as u32));
                    (blocks.len() - 1) as u32
                });
                if range_at(t, p, lat, 0.0) {
                    dawn_range += 1;
                    field[[y, x]] = RANGE | HERDS;
                }
                cells.push(RangeCell { y: y as u32, x: x as u32, t, p, block });
            }
        }
        let capacity = cells
            .iter()
            .filter(|c| is_herds(field[[c.y as usize, c.x as usize]]))
            .map(|c| cell_capacity(c.p, 0.0))
            .sum();
        Steppe {
            cells,
            blocks,
            field,
            year: NEVER,
            sky: 0.0,
            sky_decade: SkyYear::ZERO,
            range: dawn_range,
            herds: dawn_range,
            over: 0,
            frontier: 0,
            capacity,
            dawn_range,
            marks: Vec::new(),
            pressure: Vec::new(),
            log: Vec::new(),
            ledger: Vec::new(),
        }
    }

    /// Whether the first yearly pass has read the field.
    pub fn primed(&self) -> bool {
        self.year != NEVER
    }

    /// Read the decade sky at every block: `draw(x, y, year)` is the
    /// pointwise anomaly (`climate::year_anomaly_at`), drawn once per
    /// block-year and kept.
    pub fn read_sky(&mut self, year: i64, draw: &mut dyn FnMut(usize, usize, i64) -> SkyYear) {
        for b in self.blocks.iter_mut() {
            b.read(year, draw);
        }
    }

    /// The century range under forcing `f`, cells — the drift-only curve
    /// the harness holds the realized herds against.
    pub fn range_area_at(&self, f: f64, rows: usize) -> usize {
        let mut n = 0usize;
        let mut last_y = u32::MAX;
        let mut belt = 0.0f64;
        for c in &self.cells {
            if c.y != last_y {
                last_y = c.y;
                belt = crate::climate::belt_anomaly(lat_signed(rows, c.y as usize), f);
            }
            if in_window(c.t + f, c.p * (1.0 + belt)) {
                n += 1;
            }
        }
        n
    }

    /// Write the field for `year` under forcing `f` from the blocks'
    /// decade means as they stand (`read_sky` first): the century's
    /// range, the herds under it, and the herds the same decade would
    /// hold under a sky that never drifted. Updates the standings.
    pub fn advance(&mut self, year: i64, f: f64, rows: usize) {
        let (mut range, mut herds, mut over, mut frontier) = (0usize, 0usize, 0usize, 0usize);
        let mut capacity = 0.0f64;
        let mut last_y = u32::MAX;
        let mut belt = 0.0f64;
        for c in &self.cells {
            if c.y != last_y {
                last_y = c.y;
                belt = crate::climate::belt_anomaly(lat_signed(rows, c.y as usize), f);
            }
            let r = in_window(c.t + f, c.p * (1.0 + belt));
            let b = self.blocks[c.block as usize].sky;
            let line = herds_line_mm(b.dt0, b.dp0);
            let h = herds_at(c.t, c.p, b.dt, b.dp, line);
            let h0 = herds_at(c.t, c.p, b.dt0, b.dp0, line);
            let code = verdict(r, h, h0);
            self.field[[c.y as usize, c.x as usize]] = code;
            if r {
                range += 1;
            }
            if h {
                herds += 1;
                capacity += cell_capacity(c.p, b.dp);
                if !r {
                    over += 1;
                }
                if !h0 {
                    frontier += 1;
                }
            }
        }
        let n = self.blocks.len().max(1) as f64;
        self.sky_decade = SkyYear {
            dt: self.blocks.iter().map(|b| b.sky.dt).sum::<f64>() / n,
            dp: self.blocks.iter().map(|b| b.sky.dp).sum::<f64>() / n,
            dt0: self.blocks.iter().map(|b| b.sky.dt0).sum::<f64>() / n,
            dp0: self.blocks.iter().map(|b| b.sky.dp0).sum::<f64>() / n,
        };
        self.year = year;
        self.sky = f;
        self.range = range;
        self.herds = herds;
        self.over = over;
        self.frontier = frontier;
        self.capacity = capacity;
    }

    /// The field's code at a cell (NONE off the grid).
    #[inline]
    pub fn code_at(&self, y: usize, x: usize) -> u8 {
        if y < self.field.dim().0 && x < self.field.dim().1 {
            self.field[[y, x]]
        } else {
            NONE
        }
    }

    /// Grow the realm tables to `n` realms.
    pub fn grow(&mut self, n: usize) {
        if self.marks.len() < n {
            self.marks.resize_with(n, RealmMark::default);
        }
        if self.pressure.len() < n {
            self.pressure.resize(n, 0.0);
        }
    }

    /// Replay identity: the field, its standings, and the realm marks.
    pub fn hash(&self) -> u64 {
        let mut s = format!(
            "steppe|{}|{:.4}|{}|{}|{}|{}|{:.3}|{:016x}\n",
            self.year,
            self.sky,
            self.range,
            self.herds,
            self.over,
            self.frontier,
            self.capacity,
            match self.field.as_slice() {
                Some(bytes) => fnv1a64(bytes),
                None => fnv1a64(&self.field.iter().copied().collect::<Vec<u8>>()),
            }
        );
        for (i, m) in self.marks.iter().enumerate() {
            s.push_str(&format!("r{}|{}|{:.4}|{}|{:.4}\n", i, m.spoken, m.share, m.armed, self.pressure.get(i).copied().unwrap_or(0.0)));
        }
        fnv1a64(s.as_bytes())
    }
}

// ---------------------------------------------------------------- the reading over a hinterland

/// What one town's hinterland holds, read off the field and the crops.
#[derive(Clone, Copy, Debug, Default)]
pub struct Hinterland {
    pub cells: usize,
    pub range: usize,
    pub herds: usize,
    pub frontier: usize,
    pub farmed: usize,
    pub contested: usize,
}

impl Steppe {
    /// The hinterland reading at `(y, x)`: every cell within
    /// `HINTERLAND_R`, what the field says of it, what the plough says.
    pub fn hinterland(&self, y: usize, x: usize, crops: &Array2<u8>) -> Hinterland {
        let (rows, cols) = crops.dim();
        let mut h = Hinterland::default();
        let r = HINTERLAND_R;
        for dy in -r..=r {
            for dx in -r..=r {
                if dy * dy + dx * dx > r * r {
                    continue;
                }
                let yy = y as i64 + dy;
                let xx = x as i64 + dx;
                if yy < 0 || xx < 0 || yy >= rows as i64 || xx >= cols as i64 {
                    continue;
                }
                let (yy, xx) = (yy as usize, xx as usize);
                h.cells += 1;
                let code = self.field[[yy, xx]];
                let crop = crops[[yy, xx]];
                if is_range(code) {
                    h.range += 1;
                }
                if is_herds(code) {
                    h.herds += 1;
                }
                if is_frontier(code) {
                    h.frontier += 1;
                }
                if is_grain(crop) {
                    h.farmed += 1;
                }
                if contested(code, crop) {
                    h.contested += 1;
                }
            }
        }
        h
    }
}

// ---------------------------------------------------------------- prose

/// The sky's part of the telling: the century's sign, and whether the
/// decade's want (`dt`, `dp`: the weather short of the century) drove
/// the herds outward.
pub fn sky_clause(f: f64, dt: f64, dp: f64) -> String {
    let century = if f >= 0.25 {
        "a warmer century has lifted the cold edge and carried the grass up onto the sown ground"
    } else if f <= -0.25 {
        "a colder century has walked the rain belts and drawn the grass down onto the sown ground"
    } else {
        "the belts have walked a little and the grass has come with them"
    };
    let want = loss(dt, dp);
    if want >= 0.10 {
        format!("{}, and a hard decade has driven the herds outward off their failing grass", century)
    } else if want <= 0.0 && (dp > 0.04 || dt > 0.08) {
        format!("{} even as a kind decade keeps the herds close to home", century)
    } else {
        century.to_string()
    }
}

/// The frontier-pressure telling: realm, its most contested town, the
/// share and the count, the sky's part, and whether this is the first
/// time or a harder press.
pub fn pressure_text(
    realm: &str,
    town: &str,
    share: f64,
    overlap: usize,
    farmed: usize,
    f: f64,
    dt: f64,
    dp: f64,
    again: bool,
) -> String {
    let per = (share * 100.0).round().max(1.0) as i64;
    let opening = if again {
        format!("The herds press harder on {}", town)
    } else {
        format!("The herds come to the furrows of {}", town)
    };
    let fields = if overlap == 1 {
        format!("one field of {}'s {}", realm, farmed)
    } else {
        format!("{} of {}'s {} fields", overlap, realm, farmed)
    };
    let close = if again {
        "the old quarrel is a feud now, and the crown's peace frays with it."
    } else {
        "the herdsmen and the ploughmen stand in the same grass, and the crown must answer for whose it is."
    };
    format!(
        "{}: {} lie under their hooves this year — {} in a hundred — where {}; {}",
        opening, fields, per, sky_clause(f, dt, dp), close
    )
}

/// The inspector's line for one town.
pub fn herds_line(town: &str, h: &Hinterland, pressure: f64, sky: f64, dt: f64, dp: f64) -> String {
    if h.range == 0 && h.herds == 0 {
        return format!("No grass a herd could winter on lies within a day of {}.", town);
    }
    let share_h = h.herds as f64 / h.cells.max(1) as f64;
    let where_ = if h.herds == 0 {
        "the century has made grass here but this decade was too hard for a herd to hold it".to_string()
    } else if h.range == 0 && loss(dt, dp) > 0.0 {
        "a hard decade has driven the herds beyond the century's grass".to_string()
    } else if h.range == 0 {
        "the herds stand beyond the century's grass".to_string()
    } else if share_h >= 0.5 {
        "this is the herds' country".to_string()
    } else {
        "the herds graze the edge of it".to_string()
    };
    let contest = if h.contested > 0 {
        format!(
            " {} of the {} fields hereabouts are contested, where {} — the crown feels {} in a hundred of its fields pressed.",
            h.contested,
            h.farmed,
            sky_clause(sky, dt, dp),
            (pressure * 100.0).round().max(1.0) as i64
        )
    } else if h.frontier > 0 {
        format!(" {} cells hereabouts are grass the drift made, but no furrow lies under them.", h.frontier)
    } else if h.farmed > 0 && h.herds > 0 {
        " The furrows and the grass keep their old border.".to_string()
    } else {
        String::new()
    };
    format!(
        "{} of the {} cells about {} carry herds this decade ({} range): {}.{}",
        h.herds, h.cells, town, h.range, where_, contest
    )
}

// ---------------------------------------------------------------- bands

/// M99 — diagnostics bands for the steppe.
pub const BANDS: &[Band] = &[
    Band { name: "herding range share of land", sweet: (0.04, 0.30), hard: (0.01, 0.50), target: "M99: the century range as a share of land — Earth's grasslands and dry shrublands run to a quarter of the ice-free land; a margin, not a continent, never absent" },
    Band { name: "herds within the century range", sweet: (0.90, 1.10), hard: (0.85, 1.15), target: "M99: realized herd area over the century range, every year — the decade breathes the footprint inside ±10 % of what the drift dictates" },
    Band { name: "frontier pressure events per century", sweet: (0.2, 12.0), hard: (0.0, 40.0), target: "M99: dated frontier-pressure tellings per 100 y across the world — a quarrel a generation somewhere, never one a year per crown" },
    Band { name: "steppe capacity share of souls", sweet: (0.01, 2.0), hard: (0.001, 8.0), target: "M99: souls the herds' ground could feed over the settled population — pastoral country is thin: a few percent to a few times the towns, never nothing" },
];
