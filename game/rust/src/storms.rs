//! M77 — The Storm Corridors.
//!
//! The westerlies do not blow steadily: along the mid-latitude
//! baroclinic zone — the belt where the pole-to-equator temperature
//! gradient is steepest — the sky sheds its imbalance as travelling
//! cyclones. They spin up over open water, are steered downwind by the
//! zonal wind field the ocean already reads (`currents::wind_stress`),
//! drift poleward as they age, and die when they run onto land and lose
//! the sea that fed them. Coasts on the downwind side of a storm-rich
//! sea therefore live under a recurring, mappable corridor; coasts
//! upwind of one do not.
//!
//! Nothing here is painted. The genesis field is the world's *own*
//! meridional temperature gradient over water, so the corridor's
//! latitude is a consequence of the climate the earlier eras solved,
//! never a constant typed into this file — the 30–60° band the gate
//! looks for has to *emerge*, or the gate fails and says so. Likewise
//! the storm season is read from each hemisphere's realized annual
//! temperature cycle rather than from a hard-coded calendar, so the
//! southern season falls half a year from the northern one because the
//! world's seasons do, not because this module says so.
//!
//! Per ADR-0003 a storm season is *derived*, never stored: it is a pure
//! function of `(seed, year, hemisphere)` and the frozen genesis fields.
//! The replay identity line therefore carries a probe of the source
//! (`probe`), exactly as M73's variability lattice and M74's basin do.

use ndarray::Array2;
use rand::Rng;
use rand::SeedableRng;
use rand_pcg::Pcg64Mcg;

/// A cell only qualifies as a genesis site if its meridional temperature
/// gradient reaches this share of the hemisphere's *reference* gradient
/// (see `STORM_SCALE_PCTL`) — the
/// baroclinic zone is where the gradient concentrates, and a flat
/// tropical sea has no imbalance to shed.
pub const STORM_BAROCLINIC_MIN: f64 = 0.45;
/// The reference gradient is a high percentile of the hemisphere's marine
/// gradient field, not its maximum: a single cell at an ice edge or a
/// river-cooled bay can carry a gradient several times the belt's, and
/// scaling the whole zone to that outlier would shrink the corridor to a
/// handful of cells. A percentile is the robust statistic for "how steep
/// this hemisphere's sea gets" and moves with the climate rather than
/// with one pathological cell.
pub const STORM_SCALE_PCTL: f64 = 0.95;
/// Storms drawn per hemisphere per year, per thousand qualifying ocean
/// cells. A larger baroclinic sea breeds more cyclones; a world whose
/// mid-latitudes are mostly land breeds fewer.
pub const STORM_PER_1000_CELLS: f64 = 0.55;
/// No hemisphere-year draws more than this many, whatever the sea size —
/// a hard stop so a pathological world cannot hand the causal path an
/// absurd century.
pub const STORM_MAX_PER_SEASON: usize = 40;
/// Advection step, in days.
pub const STORM_STEP_DAYS: f64 = 0.5;
/// A cyclone is tracked at most this many steps (30 days).
pub const STORM_MAX_STEPS: usize = 60;
/// Steering speed: degrees of longitude per day at unit zonal wind
/// stress. The westerlies peak at stress 1.0, giving ~7°/day — a
/// mid-latitude cyclone crossing an ocean basin in a week or two.
pub const STORM_STEER_DEG: f64 = 7.0;
/// Poleward drift, degrees of latitude per day: a travelling cyclone
/// climbs the gradient as it occludes.
pub const STORM_POLEWARD_DEG: f64 = 0.55;
/// Per-step intensity growth over open water, as a share of the fuel
/// still unused (the storm saturates rather than running away).
pub const STORM_SEA_GROW: f64 = 0.055;
/// Per-step intensity retained over land — the sea is the engine, and
/// cut off from it a cyclone fills within days.
pub const STORM_LAND_KEEP: f64 = 0.86;
/// Below this intensity the storm has filled and the track ends.
pub const STORM_END: f64 = 0.15;
/// Seasonal contrast: the share by which the coldest month outweighs the
/// warmest in the genesis draw. The *phase* is read from the world.
pub const STORM_SEASON_CONTRAST: f64 = 0.75;

/// One dated point on a cyclone's path.
#[derive(Clone, Copy, Debug)]
pub struct StormPoint {
    /// Grid column, fractional.
    pub x: f64,
    /// Grid row, fractional.
    pub y: f64,
    /// Days since genesis.
    pub day: f64,
    /// Intensity, 0..1.
    pub inten: f64,
    /// Was the storm's centre over land at this point?
    pub over_land: bool,
}

/// One cyclone: where it was born, when, and the path it walked.
#[derive(Clone, Debug)]
pub struct StormTrack {
    /// Calendar year of genesis.
    pub year: i64,
    /// +1 northern, -1 southern.
    pub hemi: i8,
    /// Month of genesis, 0..11.
    pub month: i64,
    /// The genesis cell (row, col).
    pub genesis: (usize, usize),
    /// Latitude of genesis, degrees.
    pub genesis_lat: f64,
    /// The path, one point per advection step, genesis first.
    pub points: Vec<StormPoint>,
    /// Strongest intensity reached anywhere on the path.
    pub peak: f64,
    /// Did the centre ever cross onto land?
    pub landfall: bool,
}

impl StormTrack {
    /// Track length in days.
    pub fn days(&self) -> f64 {
        self.points.last().map(|p| p.day).unwrap_or(0.0)
    }
    /// Net eastward travel, in grid columns (negative = westward).
    pub fn drift_x(&self) -> f64 {
        match (self.points.first(), self.points.last()) {
            (Some(a), Some(b)) => b.x - a.x,
            _ => 0.0,
        }
    }
    /// Net poleward travel, in degrees of latitude (negative = equatorward).
    pub fn drift_pole(&self, rows: usize) -> f64 {
        match (self.points.first(), self.points.last()) {
            (Some(a), Some(b)) => (lat_of(b.y, rows).abs()) - (lat_of(a.y, rows).abs()),
            _ => 0.0,
        }
    }
}

/// Latitude of a (fractional) grid row, degrees, north positive.
pub fn lat_of(y: f64, rows: usize) -> f64 {
    -90.0 + y * 180.0 / (rows as f64 - 1.0)
}

/// The frozen genesis field for one world: baroclinicity over water, and
/// the seasonal phase each hemisphere's own temperature cycle declares.
///
/// Solved once from the finished climate; a season is then drawn from it
/// for any year without touching the grids again.
pub struct StormClimatology {
    rows: usize,
    cols: usize,
    /// |∂T/∂lat| in °C per degree, zero on land and at the poles.
    baro: Array2<f64>,
    /// The strongest gradient in each hemisphere (north, south).
    peak: (f64, f64),
    /// The coldest month of the year in each hemisphere's storm belt,
    /// read from the realized annual cycle (north, south).
    cold_month: (i64, i64),
}

impl StormClimatology {
    /// Solve the genesis field from the world's finished climate.
    ///
    /// `height` gives the land mask (>= 0 is land); `tmean` and `tamp`
    /// are the annual mean and the signed seasonal amplitude the rest of
    /// the climate stack already uses.
    pub fn new(height: &Array2<f32>, tmean: &Array2<f32>, tamp: &Array2<f32>) -> Self {
        let (rows, cols) = tmean.dim();
        let dlat = 180.0 / (rows as f64 - 1.0);
        let mut baro = Array2::<f64>::zeros((rows, cols));
        let mut gn: Vec<f64> = Vec::new();
        let mut gs: Vec<f64> = Vec::new();
        for y in 1..rows - 1 {
            let lat = lat_of(y as f64, rows);
            for x in 0..cols {
                // Cyclogenesis is a marine act: the storm needs the sea's
                // heat and moisture under it. Land cells are not sites.
                if height[[y, x]] >= 0.0 {
                    continue;
                }
                let a = tmean[[y - 1, x]] as f64;
                let b = tmean[[y + 1, x]] as f64;
                if !a.is_finite() || !b.is_finite() {
                    continue;
                }
                let g = ((b - a) / (2.0 * dlat)).abs();
                baro[[y, x]] = g;
                if lat >= 0.0 {
                    gn.push(g);
                } else {
                    gs.push(g);
                }
            }
        }

        let pctl = |v: &mut Vec<f64>| -> f64 {
            if v.is_empty() {
                return 0.0;
            }
            v.sort_by(|a, b| a.total_cmp(b));
            let i = ((v.len() - 1) as f64 * STORM_SCALE_PCTL).round() as usize;
            v[i]
        };
        let peak = (pctl(&mut gn), pctl(&mut gs));

        // The storm season is the belt's own cold season. Take the mean
        // signed seasonal amplitude over each hemisphere's qualifying
        // water and find the month at which `month_temperature` bottoms
        // out — half a year apart if and only if the world's seasons are.
        let mut amp = (0.0f64, 0.0f64);
        let mut n = (0usize, 0usize);
        for y in 0..rows {
            let north = lat_of(y as f64, rows) >= 0.0;
            let p = if north { peak.0 } else { peak.1 };
            if p <= 0.0 {
                continue;
            }
            for x in 0..cols {
                if baro[[y, x]] < STORM_BAROCLINIC_MIN * p {
                    continue;
                }
                if north {
                    amp.0 += tamp[[y, x]] as f64;
                    n.0 += 1;
                } else {
                    amp.1 += tamp[[y, x]] as f64;
                    n.1 += 1;
                }
            }
        }
        let mean_amp = (
            if n.0 > 0 { amp.0 / n.0 as f64 } else { 0.0 },
            if n.1 > 0 { amp.1 / n.1 as f64 } else { 0.0 },
        );
        let coldest = |a: f64| -> i64 {
            let mut best = 0i64;
            let mut bestv = f64::INFINITY;
            for m in 0..12i64 {
                let t = crate::climate::month_temperature(0.0, a, m);
                if t < bestv {
                    bestv = t;
                    best = m;
                }
            }
            best
        };
        Self {
            rows,
            cols,
            baro,
            peak,
            cold_month: (coldest(mean_amp.0), coldest(mean_amp.1)),
        }
    }

    /// The hemisphere's reference marine gradient (`STORM_SCALE_PCTL`
    /// percentile), °C per degree of latitude.
    pub fn peak_gradient(&self, hemi: i8) -> f64 {
        if hemi >= 0 { self.peak.0 } else { self.peak.1 }
    }

    /// The month the hemisphere's storm belt is coldest — its season.
    pub fn cold_month(&self, hemi: i8) -> i64 {
        if hemi >= 0 { self.cold_month.0 } else { self.cold_month.1 }
    }

    /// The cells eligible to breed a cyclone in this hemisphere, with the
    /// genesis weight of each. Row-major, so the order is the grid's.
    pub fn sites(&self, hemi: i8) -> Vec<((usize, usize), f64)> {
        let p = self.peak_gradient(hemi);
        let mut out = Vec::new();
        if p <= 0.0 {
            return out;
        }
        let cut = STORM_BAROCLINIC_MIN * p;
        for y in 0..self.rows {
            let north = lat_of(y as f64, self.rows) >= 0.0;
            if north != (hemi >= 0) {
                continue;
            }
            for x in 0..self.cols {
                let g = self.baro[[y, x]];
                if g >= cut {
                    out.push(((y, x), g));
                }
            }
        }
        out
    }

    /// How many cyclones this hemisphere breeds in a year — a property of
    /// the size of its baroclinic sea, not of the calendar.
    pub fn season_count(&self, hemi: i8) -> usize {
        let n = self.sites(hemi).len() as f64;
        ((n / 1000.0) * STORM_PER_1000_CELLS)
            .round()
            .max(0.0)
            .min(STORM_MAX_PER_SEASON as f64) as usize
    }

    /// Draw and walk one hemisphere's storms for one year.
    ///
    /// Pure in `(seed, year, hemi)` and the frozen fields above.
    pub fn season(&self, seed: i64, year: i64, hemi: i8, height: &Array2<f32>) -> Vec<StormTrack> {
        let sites = self.sites(hemi);
        let count = self.season_count(hemi);
        if sites.is_empty() || count == 0 {
            return Vec::new();
        }
        // Cumulative genesis weight, row-major: the draw is an inverse-cdf
        // read, so the same u always picks the same cell.
        let mut cum: Vec<f64> = Vec::with_capacity(sites.len());
        let mut total = 0.0;
        for &(_, g) in &sites {
            total += g;
            cum.push(total);
        }
        // Seasonal weights over the twelve months, peaked on the belt's
        // coldest month — the phase came from the world, the contrast is
        // the one declared constant.
        let cold = self.cold_month(hemi);
        let mut mw = [0.0f64; 12];
        let mut mtot = 0.0;
        for m in 0..12usize {
            // cos of the offset from the cold month: +1 at the peak.
            let off = ((m as i64 - cold) as f64) * std::f64::consts::TAU / 12.0;
            let w = 1.0 + STORM_SEASON_CONTRAST * off.cos();
            mw[m] = w;
            mtot += w;
        }

        let key = (seed as u64)
            ^ (year as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ if hemi >= 0 { 0x5701_1A11u64 } else { 0x50_1D_1CE5u64 };
        let mut rng = Pcg64Mcg::seed_from_u64(key ^ 0x570D_5EA5_0140_C7A1u64);
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            let u = rng.gen_range(0.0..total);
            let i = cum.partition_point(|&c| c < u).min(sites.len() - 1);
            let ((gy, gx), g) = sites[i];
            let um = rng.gen_range(0.0..mtot);
            let mut acc = 0.0;
            let mut month = 11i64;
            for m in 0..12usize {
                acc += mw[m];
                if um < acc {
                    month = m as i64;
                    break;
                }
            }
            let vigour = 0.55 + 0.45 * rng.gen_range(0.0..1.0f64);
            out.push(self.walk(year, hemi, month, (gy, gx), g, vigour, height));
        }
        out
    }

    /// Advect one cyclone from its genesis cell until it fills.
    fn walk(
        &self,
        year: i64,
        hemi: i8,
        month: i64,
        genesis: (usize, usize),
        grad: f64,
        vigour: f64,
        height: &Array2<f32>,
    ) -> StormTrack {
        let rows = self.rows;
        let cols = self.cols;
        let dlat = 180.0 / (rows as f64 - 1.0);
        // Longitude per column: the map spans 360° across its columns.
        let dlon = 360.0 / cols as f64;
        let p = self.peak_gradient(hemi).max(1e-9);
        let mut inten = ((grad / p).min(1.0)) * vigour;
        let mut x = genesis.1 as f64;
        let mut y = genesis.0 as f64;
        let mut day = 0.0f64;
        let mut peak = inten;
        let mut landfall = false;
        let mut points = Vec::with_capacity(STORM_MAX_STEPS + 1);
        let mut over_land = height[[genesis.0, genesis.1]] >= 0.0;
        points.push(StormPoint { x, y, day, inten, over_land });

        for _ in 0..STORM_MAX_STEPS {
            let lat = lat_of(y, rows);
            // Steered by the zonal wind at its latitude: westerlies carry
            // it east, the trades would carry it west. Same field the
            // ocean's gyres read, so corridor and current agree.
            let tau = crate::currents::wind_stress(lat.abs());
            let dx = STORM_STEER_DEG * tau * STORM_STEP_DAYS / dlon;
            // …and climbing poleward as it occludes: toward the pole of
            // its own hemisphere, i.e. toward larger |lat|.
            let pole_sign = if lat >= 0.0 { 1.0 } else { -1.0 };
            let dy = pole_sign * STORM_POLEWARD_DEG * STORM_STEP_DAYS / dlat;
            x += dx;
            y += dy;
            day += STORM_STEP_DAYS;
            if x < 0.0 || x > (cols - 1) as f64 || y < 0.0 || y > (rows - 1) as f64 {
                break;
            }
            let cy = y.round() as usize;
            let cx = x.round() as usize;
            over_land = height[[cy.min(rows - 1), cx.min(cols - 1)]] >= 0.0;
            if over_land {
                landfall = true;
                inten *= STORM_LAND_KEEP;
            } else {
                inten += STORM_SEA_GROW * (1.0 - inten).max(0.0);
            }
            if inten > peak {
                peak = inten;
            }
            points.push(StormPoint { x, y, day, inten, over_land });
            if inten < STORM_END {
                break;
            }
        }

        StormTrack {
            year,
            hemi,
            month,
            genesis,
            genesis_lat: lat_of(genesis.0 as f64, rows),
            points,
            peak,
            landfall,
        }
    }

    /// A fixed read of the storm law for the replay identity line: the
    /// hemispheric peaks, the seasons, the site counts, and the full
    /// tracks of two spaced years. A corridor whose constants or keying
    /// drift breaks replay here.
    pub fn probe(&self, seed: i64, height: &Array2<f32>) -> u64 {
        let mut b: Vec<u8> = Vec::new();
        for v in [self.peak.0, self.peak.1] {
            b.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        for v in [self.cold_month.0, self.cold_month.1] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for h in [1i8, -1i8] {
            b.extend_from_slice(&(self.sites(h).len() as u64).to_le_bytes());
            for year in [1i64, 97] {
                for t in self.season(seed, year, h, height) {
                    b.extend_from_slice(&(t.month as i64).to_le_bytes());
                    b.extend_from_slice(&(t.genesis.0 as u32).to_le_bytes());
                    b.extend_from_slice(&(t.genesis.1 as u32).to_le_bytes());
                    b.extend_from_slice(&t.peak.to_bits().to_le_bytes());
                    b.extend_from_slice(&(t.points.len() as u32).to_le_bytes());
                    if let Some(p) = t.points.last() {
                        b.extend_from_slice(&p.x.to_bits().to_le_bytes());
                        b.extend_from_slice(&p.y.to_bits().to_le_bytes());
                    }
                }
            }
        }
        crate::util::fnv1a64(&b)
    }
}
