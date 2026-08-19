//! M22 — fault seams and earthquakes: the deep earth acting *during*
//! history, not only at world-genesis.
//!
//! The plate-history sketch (M16, ADR-0024) stays frozen — this module
//! reads its boundary grid once at the dawn and derives **fault seams**:
//! connected runs of convergent and transform boundary cells long enough
//! to matter. Each seam then lives on a renewal clock: stress accumulates
//! with time-since-last-rupture at a rate set by the boundary type, the
//! monthly rupture hazard rises with that stress toward a saturation
//! ceiling scaled by seam length, and a rupture's magnitude is drawn from
//! the stress released (long-quiet seams break big). Epicenter cell,
//! magnitude and month land in a **seismic log** — the world's instrument
//! record. M24 wires the log into the chronicle; here it is pure state.
//!
//! Determinism (ADR-0003): the module owns its own PCG stream, seeded
//! from the world seed alone and stored in `Seismic`. It never touches
//! the shared world RNG, so every existing history replays byte-for-byte
//! with or without this system in the lattice — and the log itself
//! replays byte-identical across native and wasm (the M22 gate).

use ndarray::Array2;
use rand::Rng;
use rand_pcg::Pcg64Mcg;

use crate::plates::{Plates, B_CONVERGENT, B_TRANSFORM};
use crate::util::{fnv1a64, Band};

/// Cell edge in km (ADR-0004): fault length is cells × 4 km.
const KM_PER_CELL: f64 = 4.0;

/// A seam shorter than this many cells is boundary confetti, not a fault.
const MIN_CELLS: usize = 8;

/// A connected boundary component longer than this is split into segments
/// of at most this many cells (240 km). Real margins rupture in segments;
/// one break must not reset the clock of a whole 2000-km margin — that
/// starves frequency and pushes every rupture into the great-quake range.
const MAX_CELLS: usize = 60;

/// Characteristic reload time: the rational ramp `t / (t + τ)` is
/// half-loaded this many years after the last rupture. Rational, not
/// exponential, on purpose — `exp()` is libm and its last bits differ
/// between native and wasm; ÷ is IEEE-exact everywhere (the M22
/// cross-runtime replay gate).
const TAU_YEARS: f64 = 40.0;

/// Saturated monthly rupture probability per 100 km of seam. Calibrated
/// so a mature world logs on the order of one notable quake per 100 km
/// of active fault per century (the `diagnose earth` band).
const P100_SAT: f64 = 0.0020;

/// Stress-accumulation rate by boundary type: convergent margins load
/// fastest; transforms slip often but store somewhat less.
fn rate(kind: u8) -> f64 {
    if kind == B_CONVERGENT {
        1.0
    } else {
        0.85
    }
}

/// One fault seam, frozen at derivation: its cells, its kind, its length.
#[derive(Clone)]
pub struct Fault {
    /// `B_CONVERGENT` or `B_TRANSFORM` (divergent seams open ocean floor
    /// far from anyone's chronicle; the spec seats hazard on these two).
    pub kind: u8,
    /// Member cells (y, x) in final world coordinates, scan order.
    pub cells: Vec<(u32, u32)>,
    /// Seam length, km.
    pub km: f64,
}

/// One logged earthquake: month, epicenter cell, magnitude, source seam.
#[derive(Clone)]
pub struct Quake {
    pub m: i64,
    pub y: u32,
    pub x: u32,
    /// Moment magnitude, one decimal (rounded so the wire and the hash
    /// never depend on float formatting).
    pub mag: f64,
    pub fault: u16,
}

/// The living seismic state: the seam table, each seam's renewal clock,
/// and the instrument record of every rupture since the dawn.
#[derive(Clone)]
pub struct Seismic {
    pub faults: Vec<Fault>,
    /// Months since last rupture, per fault (starts staggered so the
    /// world's first century isn't one synchronized drumroll).
    pub since: Vec<u32>,
    pub log: Vec<Quake>,
    rng: Pcg64Mcg,
}

impl Seismic {
    /// A world with no seams yet (pre-dawn placeholder).
    pub fn empty() -> Self {
        Seismic {
            faults: Vec::new(),
            since: Vec::new(),
            log: Vec::new(),
            rng: crate::util::rng(0),
        }
    }

    /// Total seam length in km, optionally filtered by kind.
    pub fn total_km(&self, kind: Option<u8>) -> f64 {
        self.faults
            .iter()
            .filter(|f| kind.map_or(true, |k| f.kind == k))
            .map(|f| f.km)
            .sum()
    }

    /// One month of the renewal model. Every seam loads; loaded seams may
    /// break; a break logs a quake and resets that seam's clock. Pure
    /// function of prior state — no shared RNG, no wall clock.
    pub fn monthly(&mut self, month: i64) {
        for i in 0..self.faults.len() {
            self.since[i] = self.since[i].saturating_add(1);
            let f = &self.faults[i];
            let years = self.since[i] as f64 / 12.0 * rate(f.kind);
            // Hazard ramps toward a ceiling scaled by seam length. The
            // rational saturation t/(t+τ) stands in for 1−e^(−t/τ):
            // same shape, only IEEE-exact ops (cross-runtime identity).
            let ramp = years / (years + TAU_YEARS);
            let p = (f.km / 100.0) * P100_SAT * ramp;
            if self.rng.gen::<f64>() >= p {
                continue;
            }
            // Rupture: magnitude from the stress released (fraction of
            // the reload cycle completed) plus a Gutenberg–Richter-
            // flavored tail. The reciprocal draw stands in for the
            // exponential law with only IEEE-exact ops: most breaks add
            // a tenth or two, the rare top-percentile draw breaks great.
            let jitter = self.rng.gen::<f64>();
            let bonus = if f.kind == B_CONVERGENT { 0.3 } else { 0.0 };
            let tail = 0.09 / (1.03 - jitter) - 0.0874;
            let mag = (4.55 + 2.3 * ramp + bonus + tail).min(9.0);
            let mag = (mag * 10.0).round() / 10.0;
            let ei = (self.rng.gen::<f64>() * self.faults[i].cells.len() as f64) as usize;
            let (y, x) = self.faults[i].cells[ei.min(self.faults[i].cells.len() - 1)];
            self.log.push(Quake {
                m: month,
                y,
                x,
                mag,
                fault: i as u16,
            });
            self.since[i] = 0;
        }
    }

    /// FNV-1a over seams, clocks and the whole log — the replay identity
    /// the M22 gate compares between two native runs and across wasm.
    pub fn hash(&self) -> u64 {
        let mut b: Vec<u8> = Vec::with_capacity(self.log.len() * 24 + self.faults.len() * 16);
        for f in &self.faults {
            b.push(f.kind);
            b.extend_from_slice(&(f.cells.len() as u32).to_le_bytes());
            for &(y, x) in &f.cells {
                b.extend_from_slice(&y.to_le_bytes());
                b.extend_from_slice(&x.to_le_bytes());
            }
            b.extend_from_slice(&f.km.to_bits().to_le_bytes());
        }
        for &s in &self.since {
            b.extend_from_slice(&s.to_le_bytes());
        }
        for q in &self.log {
            b.extend_from_slice(&q.m.to_le_bytes());
            b.extend_from_slice(&q.y.to_le_bytes());
            b.extend_from_slice(&q.x.to_le_bytes());
            b.extend_from_slice(&q.mag.to_bits().to_le_bytes());
            b.extend_from_slice(&q.fault.to_le_bytes());
        }
        fnv1a64(&b)
    }

    /// Sub-hashes for cross-runtime bisection: (faults, since, log).
    /// Debug instrumentation for the M22 gate — not part of the wire.
    pub fn debug_parts(&self) -> (u64, u64, u64) {
        let mut fb: Vec<u8> = Vec::new();
        for f in &self.faults {
            fb.push(f.kind);
            fb.extend_from_slice(&(f.cells.len() as u32).to_le_bytes());
            for &(y, x) in &f.cells {
                fb.extend_from_slice(&y.to_le_bytes());
                fb.extend_from_slice(&x.to_le_bytes());
            }
            fb.extend_from_slice(&f.km.to_bits().to_le_bytes());
        }
        let mut sb: Vec<u8> = Vec::new();
        for &s in &self.since {
            sb.extend_from_slice(&s.to_le_bytes());
        }
        let mut lb: Vec<u8> = Vec::new();
        for q in &self.log {
            lb.extend_from_slice(&q.m.to_le_bytes());
            lb.extend_from_slice(&q.y.to_le_bytes());
            lb.extend_from_slice(&q.x.to_le_bytes());
            lb.extend_from_slice(&q.mag.to_bits().to_le_bytes());
            lb.extend_from_slice(&q.fault.to_le_bytes());
        }
        (fnv1a64(&fb), fnv1a64(&sb), fnv1a64(&lb))
    }
}

/// Derive the seam table from the (already margin-widened) plate sketch.
/// Connected components over same-kind boundary cells, 8-connectivity,
/// fixed scan order — deterministic in the grid alone; the RNG only
/// staggers the initial clocks afterwards, in seam order.
pub fn derive(seed: i64, plates: &Plates) -> Seismic {
    let (h, w) = plates.boundary.dim();
    let mut seen = Array2::from_elem((h, w), false);
    let mut faults: Vec<Fault> = Vec::new();

    for y in 0..h {
        for x in 0..w {
            let kind = plates.boundary[[y, x]];
            if seen[[y, x]] || (kind != B_CONVERGENT && kind != B_TRANSFORM) {
                continue;
            }
            // flood this same-kind component, FIFO, fixed neighbor order
            let mut cells: Vec<(u32, u32)> = Vec::new();
            let mut queue: Vec<(usize, usize)> = vec![(y, x)];
            seen[[y, x]] = true;
            while let Some((cy, cx)) = queue.pop() {
                cells.push((cy as u32, cx as u32));
                for dy in -1isize..=1 {
                    for dx in -1isize..=1 {
                        if dy == 0 && dx == 0 {
                            continue;
                        }
                        let ny = cy as isize + dy;
                        let nx = cx as isize + dx;
                        if ny < 0 || nx < 0 || ny >= h as isize || nx >= w as isize {
                            continue;
                        }
                        let (ny, nx) = (ny as usize, nx as usize);
                        if !seen[[ny, nx]] && plates.boundary[[ny, nx]] == kind {
                            seen[[ny, nx]] = true;
                            queue.push((ny, nx));
                        }
                    }
                }
            }
            if cells.len() < MIN_CELLS {
                continue;
            }
            cells.sort_unstable(); // stack pop order varies with shape; the seam does not

            // Segment long components: nearly equal scan-order chunks of
            // at most MAX_CELLS each, so no segment falls under MIN_CELLS.
            let parts = cells.len().div_ceil(MAX_CELLS);
            let base = cells.len() / parts;
            let extra = cells.len() % parts;
            let mut at = 0usize;
            for p in 0..parts {
                let take = base + usize::from(p < extra);
                let seg: Vec<(u32, u32)> = cells[at..at + take].to_vec();
                at += take;
                let km = seg.len() as f64 * KM_PER_CELL;
                faults.push(Fault { kind, cells: seg, km });
            }
        }
    }

    // Stagger the clocks: each seam starts partway into a reload cycle,
    // drawn from the module's own stream in seam order.
    let mut rng = crate::util::rng(seed.wrapping_mul(53).wrapping_add(2626));
    let period = (TAU_YEARS * 12.0) as u32;
    let since: Vec<u32> = faults
        .iter()
        .map(|_| (rng.gen::<f64>() * period as f64) as u32)
        .collect();

    Seismic {
        faults,
        since,
        log: Vec::new(),
        rng,
    }
}

// ---------------------------------------------------------------- bands

/// Diagnostics bands (E2.7): the pulse of the deep earth. Frequency is
/// the M22 gate quantity — quakes per 100 km of seam per century, the
/// order-unity figure real fault systems log for notable events.
pub const BANDS: &[Band] = &[
    Band { name: "fault seams", sweet: (10.0, 120.0), hard: (4.0, 300.0), target: "M22: a mapped world holds tens of named seams, not confetti" },
    Band { name: "active fault km", sweet: (4000.0, 60000.0), hard: (1000.0, 120000.0), target: "M22: convergent+transform seam length on a 512-world" },
    Band { name: "quakes per 100km-century", sweet: (0.4, 3.0), hard: (0.1, 8.0), target: "M22 gate: order-unity notable events per 100 km per century" },
    Band { name: "mean quake magnitude", sweet: (4.8, 7.0), hard: (4.2, 8.0), target: "M22: renewal model centers on strong-but-survivable" },
    Band { name: "great quakes share (M>=7.5)", sweet: (0.005, 0.35), hard: (0.0, 0.6), target: "M22: the tail exists — long-quiet seams break big" },
];

// ---------------------------------------------------------- volcanism

/// M23 — live volcanism: the arcs and hotspot chains that sculpted the
/// map at genesis keep erupting through inhabited centuries.
///
/// Cones are derived once, after the widen, from state the generator
/// already froze: local height maxima inside the volcanic rock province
/// (M18). The generator *writes age into height by construction* — the
/// hotspot pass raises its youngest islands tallest and wears the elders
/// to shoals, and arc beads swell where the magma supply is freshest —
/// so a cone's age is read straight off its summit height. Young cones
/// erupt often; old ones sleep long and break rarely (the arc-age
/// tercile band in `diagnose earth`).
///
/// An eruption is dated, located and sized (VEI-flavored), and acts
/// twice: ash settles as a *permanent* fertility bonus decaying with
/// distance from the cone (volcanic soils stay rich — Java, Sicily),
/// and the burn/bury radius culls population in settlements under the
/// plume. Chronicle beats and ruins arrive with M24; here the record is
/// pure state, like the quake log beside it.

/// A cone may stand as low as this (shoal volcanoes still erupt).
const CONE_MIN_H: f32 = -0.05;
/// Local-maximum test radius (Chebyshev cells).
const LOCALMAX_R: isize = 2;
/// Minimum spacing between accepted cones (Chebyshev cells).
const CONE_SPACING: i64 = 6;
/// Characteristic reload time of a magma chamber, years (rational ramp,
/// same IEEE-exact discipline as the quake clock — ADR-0025).
const TAU_V_YEARS: f64 = 25.0;
/// Saturated monthly eruption probability of a fully young cone.
const P_ERUPT_SAT: f64 = 0.0011;
/// How much age suppresses the hazard: an elder erupts at 28% the pace.
const AGE_DAMP: f64 = 0.72;
/// Cumulative ash-fertility cap per cell.
const ASH_CAP: f32 = 0.30;

/// One volcanic cone, frozen at derivation.
#[derive(Clone)]
pub struct Cone {
    pub y: u32,
    pub x: u32,
    /// 0 young (tall, fresh) .. 1 old (worn to a shoal).
    pub age: f64,
}

/// One logged eruption: month, vent cell, VEI-flavored size, cone index.
#[derive(Clone)]
pub struct Eruption {
    pub m: i64,
    pub y: u32,
    pub x: u32,
    /// Explosivity, one decimal (rounded so hashes never depend on
    /// float formatting).
    pub vei: f64,
    pub cone: u16,
}

/// The living volcanic state: cones, their reload clocks, the ash
/// ledger (cumulative fertility bonus laid per cell), and the record.
#[derive(Clone)]
pub struct Volcanism {
    pub cones: Vec<Cone>,
    /// Months since last eruption, per cone (staggered at the dawn).
    pub since: Vec<u32>,
    pub log: Vec<Eruption>,
    /// Cumulative ash-fertility bonus per cell — what `diagnose earth`
    /// samples for the distance-decay band. The same deltas are added
    /// into `Fields::fertility`, capped by `ASH_CAP`.
    pub ash: ndarray::Array2<f32>,
    rng: Pcg64Mcg,
}

impl Volcanism {
    /// A world with no cones yet (pre-dawn placeholder).
    pub fn empty() -> Self {
        Volcanism {
            cones: Vec::new(),
            since: Vec::new(),
            log: Vec::new(),
            ash: Array2::zeros((1, 1)),
            rng: crate::util::rng(0),
        }
    }

    /// One month of the reload model. Every cone loads; loaded cones may
    /// blow; a blow logs an eruption and lays ash into the fertility
    /// grid. Burn-and-bury damage lives in `World::eruption_effects`
    /// (M24) — the world pass reads the log tail, so every mark opens
    /// its rebuild arc and the buried get their ruin and their beat.
    /// Own RNG stream — histories replay byte-for-byte (ADR-0003).
    pub fn monthly(&mut self, month: i64, fertility: &mut ndarray::Array2<f32>) {
        let (gh, gw) = self.ash.dim();
        for i in 0..self.cones.len() {
            self.since[i] = self.since[i].saturating_add(1);
            let c = &self.cones[i];
            let years = self.since[i] as f64 / 12.0;
            // Rational saturation t/(t+τ) — the chamber refills.
            let ramp = years / (years + TAU_V_YEARS);
            let youth = 1.0 - AGE_DAMP * c.age;
            let p = P_ERUPT_SAT * youth * ramp;
            if self.rng.gen::<f64>() >= p {
                continue;
            }
            // Size: reload fraction plus a reciprocal heavy tail (the
            // same IEEE-exact stand-in family as the quake magnitudes).
            let u = self.rng.gen::<f64>();
            let tail = 0.35 / (1.04 - u) - 0.3365;
            let vei = (1.6 + 2.4 * ramp + tail).min(7.0);
            let vei = (vei * 10.0).round() / 10.0;
            let (cy, cx) = (c.y as usize, c.x as usize);

            // Ash apron: permanent fertility, quadratic falloff over the
            // plume radius, cumulative cap per cell.
            let r_ash = 2.0 + 1.1 * vei;
            let rr = r_ash.ceil() as isize;
            for dy in -rr..=rr {
                for dx in -rr..=rr {
                    let ny = cy as isize + dy;
                    let nx = cx as isize + dx;
                    if ny < 0 || nx < 0 || ny >= gh as isize || nx >= gw as isize {
                        continue;
                    }
                    let d = ((dy * dy + dx * dx) as f64).sqrt();
                    if d > r_ash {
                        continue;
                    }
                    let fall = 1.0 - d / r_ash;
                    let dep = (0.015 + 0.011 * vei) * fall * fall;
                    let cell = [ny as usize, nx as usize];
                    let room = (ASH_CAP - self.ash[cell]).max(0.0);
                    let dep = (dep as f32).min(room);
                    if dep > 0.0 {
                        self.ash[cell] += dep;
                        fertility[cell] += dep;
                    }
                }
            }

            self.log.push(Eruption {
                m: month,
                y: c.y,
                x: c.x,
                vei,
                cone: i as u16,
            });
            self.since[i] = 0;
        }
    }

    /// FNV-1a over cones, clocks, the log and the ash ledger — folded
    /// into the determinism hash (ADR-0003) and, since M27, into the
    /// cross-runtime deep-earth identity: the wasm-replay lane measured
    /// the terrain-downstream layers byte-identical across runtimes,
    /// so the gate covers them rather than assuming they drift.
    pub fn hash(&self) -> u64 {
        let mut b: Vec<u8> = Vec::with_capacity(self.log.len() * 24 + self.cones.len() * 16);
        for c in &self.cones {
            b.extend_from_slice(&c.y.to_le_bytes());
            b.extend_from_slice(&c.x.to_le_bytes());
            b.extend_from_slice(&c.age.to_bits().to_le_bytes());
        }
        for &s in &self.since {
            b.extend_from_slice(&s.to_le_bytes());
        }
        for e in &self.log {
            b.extend_from_slice(&e.m.to_le_bytes());
            b.extend_from_slice(&e.y.to_le_bytes());
            b.extend_from_slice(&e.x.to_le_bytes());
            b.extend_from_slice(&e.vei.to_bits().to_le_bytes());
            b.extend_from_slice(&e.cone.to_le_bytes());
        }
        for &a in self.ash.iter() {
            b.extend_from_slice(&a.to_bits().to_le_bytes());
        }
        fnv1a64(&b)
    }
}

/// Derive the cone table from frozen genesis state: local height maxima
/// inside the volcanic rock province (M18), thinned to a minimum
/// spacing, tallest first. Runs after the widen, so every vent sits in
/// shipped map coordinates — like the seams above.
pub fn derive_volcanism(
    seed: i64,
    height: &ndarray::Array2<f32>,
    rock: &ndarray::Array2<u8>,
    sealevel: &crate::sealevel::SeaLevel,
) -> Volcanism {
    let (h, w) = height.dim();

    // Candidates: volcanic-province cells at shoal level or above that
    // top every neighbor within LOCALMAX_R (ties broken by scan order:
    // strictly-greater wins over cells later in scan, ties-or-better
    // over earlier ones — one winner per plateau, deterministically).
    let mut cand: Vec<(f32, u32, u32)> = Vec::new();
    for y in 0..h {
        for x in 0..w {
            if rock[[y, x]] != crate::rock::VOLCANIC || height[[y, x]] < CONE_MIN_H {
                continue;
            }
            let v = height[[y, x]];
            let mut top = true;
            'scan: for dy in -LOCALMAX_R..=LOCALMAX_R {
                for dx in -LOCALMAX_R..=LOCALMAX_R {
                    if dy == 0 && dx == 0 {
                        continue;
                    }
                    let ny = y as isize + dy;
                    let nx = x as isize + dx;
                    if ny < 0 || nx < 0 || ny >= h as isize || nx >= w as isize {
                        continue;
                    }
                    let nv = height[[ny as usize, nx as usize]];
                    let earlier = (ny as usize, nx as usize) < (y, x);
                    if nv > v || (nv == v && earlier) {
                        top = false;
                        break 'scan;
                    }
                }
            }
            if top {
                cand.push((v, y as u32, x as u32));
            }
        }
    }

    // Tallest first (scan order breaks height ties), then greedy
    // min-spacing thinning: mountains keep their summits, ridges don't
    // become picket fences of vents.
    cand.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap().then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    let mut picked: Vec<(f32, u32, u32)> = Vec::new();
    for &(v, y, x) in &cand {
        let ok = picked.iter().all(|&(_, cy, cx)| {
            let dy = (cy as i64 - y as i64).abs();
            let dx = (cx as i64 - x as i64).abs();
            dy.max(dx) >= CONE_SPACING
        });
        if !ok {
            continue;
        }
        // Age reads the height the *magma* built, not the height the
        // waterline moved (M25): subtract the freeze-time eustatic and
        // isostatic offsets so an old high-latitude shield rebounding
        // out of the sea does not read as a young cone.
        let dz = sealevel.row[(y as usize).min(sealevel.row.len() - 1)] - sealevel.eustatic;
        picked.push((v - dz as f32, y, x));
    }

    // Age is the summit's height *rank* within the roster — taller is
    // younger by construction (hotspots raise their freshest islands
    // tallest, arc beads swell where supply is freshest), and rank
    // spreads the roster across the whole age axis no matter what the
    // world's absolute relief is. The old affine map off absolute
    // height saturated: most arc summits clear its "fully young"
    // ceiling, two-thirds of every roster read age 0.00, and the M23
    // cadence terciles measured noise.
    let mut order: Vec<usize> = (0..picked.len()).collect();
    order.sort_by(|&a, &b| {
        picked[b].0.partial_cmp(&picked[a].0).unwrap()
            .then(picked[a].1.cmp(&picked[b].1))
            .then(picked[a].2.cmp(&picked[b].2))
    });
    let n = picked.len();
    let mut cones: Vec<Cone> = picked
        .iter()
        .map(|&(_, y, x)| Cone { y, x, age: 0.5 })
        .collect();
    for (rank, &i) in order.iter().enumerate() {
        cones[i].age = if n > 1 { rank as f64 / (n - 1) as f64 } else { 0.5 };
    }

    // Stagger the reload clocks, own stream, cone order.
    let mut rng = crate::util::rng(seed.wrapping_mul(29).wrapping_add(2929));
    let period = (TAU_V_YEARS * 12.0) as u32;
    let since: Vec<u32> = cones
        .iter()
        .map(|_| (rng.gen::<f64>() * period as f64) as u32)
        .collect();

    Volcanism {
        cones,
        since,
        log: Vec::new(),
        ash: Array2::zeros((h, w)),
        rng,
    }
}

/// Diagnostics bands for the volcanism lane (M23).
pub const VOLCANO_BANDS: &[Band] = &[
    Band { name: "volcano cones", sweet: (8.0, 160.0), hard: (3.0, 400.0), target: "M23: a mapped world keeps tens of living vents" },
    Band { name: "eruptions per cone-century", sweet: (0.10, 1.80), hard: (0.02, 4.0), target: "M23: cones blow on decade-to-century clocks" },
    Band { name: "young/old cadence ratio", sweet: (1.25, 40.0), hard: (1.0, 400.0), target: "M23 gate: young tercile erupts oftener than the old" },
    Band { name: "mean eruption VEI", sweet: (2.0, 4.4), hard: (1.5, 5.6), target: "M23: most blows are survivable; the tail is not" },
    Band { name: "ash bonus at the cone", sweet: (0.015, 0.32), hard: (0.005, 0.40), target: "M23: fertile aprons form where ash falls" },
];
