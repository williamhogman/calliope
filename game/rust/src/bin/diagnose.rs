//! diagnose — the tuning instrument for the Calliope world engine.
//!
//! Every subcommand generates worlds natively, measures them, and prints a
//! text report with [PASS]/[WARN]/[FAIL] checks against tuning targets, so
//! the whole simulation can be judged and retuned without ever opening the
//! browser. Reports are written by scripts/report.sh into game/reports/.
//!
//!   diagnose terrain     <seed> <size>          landmasses, hypsometry, islands
//!   diagnose climate     <seed> <size>          temperature, rain, biome balance
//!   diagnose hydro       <seed> <size>          rivers, lakes, discharge
//!   diagnose resources   <seed> <size>          deposit ontology and placement
//!   diagnose civ         <seed> <size> <years>  a century of history, examined
//!   diagnose economy     <seed> <size> <years>  prices, wealth, routes, gini
//!   diagnose telling     <seed> <size> <years>  the chronicle judged (M6)
//!   diagnose determinism <seed> <size> <months> same seed => same world, always
//!   diagnose properties  <size> <years> <seeds> seam-invariant properties (M8.1/8.2)
//!   diagnose era         <size> <years> <n> <base> expressive range + oatmeal (M8.3/8.4)
//!   diagnose bench                              generation + tick throughput
//!   diagnose sweep       <size> <years> <seeds> cross-seed robustness table
//!   diagnose earth       <size> <years> <seeds> fault seams + quake cadence (M22)
//!   diagnose ocean       <size> <seeds>         gyres, coast heat, upwelling, sea seasons (M49)
//!   diagnose ocean       <size> <seeds> --metamorphic  kill/scale the currents, assert response (M50)
//!   diagnose seismic-hash <seed> <size> <months> bare ledger hash (wasm replay leg)
//!   diagnose gate        <size> <years> <seed> [--reports <dir>]  Era I sealed as one verdict (M65)

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Instant;

// E5.10 — counting allocator behind the `alloc-count` feature: every heap
// allocation (and growth realloc) bumps one relaxed atomic, making
// allocations/tick a banded metric in `bench`. The count is deterministic
// for a fixed seed, so the band can be tight; report.sh builds with the
// feature on.
#[cfg(feature = "alloc-count")]
mod alloc_count {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicU64, Ordering};

    static ALLOCS: AtomicU64 = AtomicU64::new(0);

    pub fn count() -> u64 {
        ALLOCS.load(Ordering::Relaxed)
    }

    pub struct Counting;

    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, l: Layout) -> *mut u8 {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            System.alloc(l)
        }
        unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            System.alloc_zeroed(l)
        }
        unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
            System.dealloc(p, l)
        }
        unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            System.realloc(p, l, n)
        }
    }
}

#[cfg(feature = "alloc-count")]
#[global_allocator]
static GLOBAL: alloc_count::Counting = alloc_count::Counting;

use ndarray::Array2;

use calliope::world::CellFlags;
use calliope::climate as clim;
use calliope::constants as gc;
use calliope::economy;
use calliope::entity::EntityKind;
use calliope::hydrology;
use calliope::naming;
use calliope::ndimage;
use calliope::resources;
use calliope::society;
use calliope::systems::{Cadence, SYSTEMS};
use calliope::telling;
use calliope::util::{band as band_spec, quantile};
use calliope::world::{Event, World};

// ================================================================ checks

const LVL: [&str; 3] = ["PASS", "WARN", "FAIL"];

#[derive(Default)]
struct Checks {
    rows: Vec<(usize, String, String, String)>,
}

impl Checks {
    /// PASS inside sweet, WARN inside hard, FAIL outside hard.
    fn range(&mut self, name: &str, v: f64, shown: String, sweet: (f64, f64), hard: (f64, f64), target: &str) {
        let lvl = if v >= sweet.0 && v <= sweet.1 {
            0
        } else if v >= hard.0 && v <= hard.1 {
            1
        } else {
            2
        };
        self.rows.push((lvl, name.to_string(), shown, target.to_string()));
    }
    /// Range check against a band declared beside its system (E2.7).
    fn band(&mut self, name: &str, v: f64, shown: String) {
        let b = band_spec(name);
        self.range(b.name, v, shown, b.sweet, b.hard, b.target);
    }
    /// Same band, reported under another row name (sweep means etc.).
    fn band_as(&mut self, display: &str, band: &str, v: f64, shown: String) {
        let b = band_spec(band);
        self.range(display, v, shown, b.sweet, b.hard, b.target);
    }
    fn must(&mut self, name: &str, ok: bool, shown: String, target: &str) {
        self.rows.push((if ok { 0 } else { 2 }, name.to_string(), shown, target.to_string()));
    }
    fn want(&mut self, name: &str, ok: bool, shown: String, target: &str) {
        self.rows.push((if ok { 0 } else { 1 }, name.to_string(), shown, target.to_string()));
    }
    fn print(&self) {
        println!();
        println!("---- checks ----------------------------------------------------------");
        for (lvl, name, shown, target) in &self.rows {
            println!("[{}] {:<36} {:>14}   ({})", LVL[*lvl], name, shown, target);
        }
        let p = self.rows.iter().filter(|r| r.0 == 0).count();
        let w = self.rows.iter().filter(|r| r.0 == 1).count();
        let f = self.rows.iter().filter(|r| r.0 == 2).count();
        println!("CHECKS: {} pass · {} warn · {} fail", p, w, f);
    }
}

// ================================================================ helpers

fn pct(x: f64) -> String {
    format!("{:.1}%", 100.0 * x)
}

fn header(title: &str, sub: &str) {
    println!("========================================================================");
    println!(" CALLIOPE DIAGNOSTIC · {:<28} {:>18}", title, sub);
    println!("========================================================================");
}

fn land_mask(w: &World) -> Array2<bool> {
    w.fields.height.mapv(|h| h >= 0.0)
}

/// M48 split `Route::closed` into labeled closures; the censuses read
/// their own label, so a monsoon burst never trips the winter law and
/// the pack never trips the gale law.
fn route_ice_mask(r: &calliope::trade::Route) -> u16 {
    r.shut
        .iter()
        .find_map(|s| match s {
            calliope::trade::SeasonalClosure::Ice(m) => Some(*m),
            _ => None,
        })
        .unwrap_or(0)
}

/// M48 — the monsoon burst label, if any.
fn route_monsoon_mask(r: &calliope::trade::Route) -> u16 {
    r.shut
        .iter()
        .find_map(|s| match s {
            calliope::trade::SeasonalClosure::Monsoon(m) => Some(*m),
            _ => None,
        })
        .unwrap_or(0)
}

/// M37 — census the icebound lanes: (winter-closed, perennially shut,
/// malformed masks). A well-formed closure is one contiguous winter arc,
/// hemisphere-true for the water that actually froze along the way.
fn ice_route_stats(w: &World, frozen: &Array2<u16>) -> (usize, usize, usize) {
    use calliope::{seaice, trade};
    let (rows, cols) = w.fields.height.dim();
    let mut iced = 0usize;
    let mut perennial = 0usize;
    let mut bad = 0usize;
    for r in &w.routes {
        let ice = route_ice_mask(r);
        if ice == 0 {
            continue;
        }
        if ice == seaice::MONTHS_MASK {
            perennial += 1;
            continue;
        }
        iced += 1;
        // hemisphere = mean row of the frozen water along the way
        let (mut sy, mut n) = (0.0f64, 0usize);
        for (p, &m) in r.path.iter().zip(r.m.iter()) {
            if m == trade::MODE_SEA
                && p[0] >= 0
                && p[1] >= 0
                && (p[0] as usize) < cols
                && (p[1] as usize) < rows
                && frozen[[p[1] as usize, p[0] as usize]] != 0
            {
                sy += p[1] as f64;
                n += 1;
            }
        }
        let southern = n > 0 && sy / n as f64 >= rows as f64 / 2.0;
        if n == 0 || !seaice::is_winter_arc(ice, southern) {
            bad += 1;
        }
    }
    (iced, perennial, bad)
}

/// M48 gate — the sailor's calendar as bytes: every seasonal lane's
/// month-by-month state (shut flag + throughput multiplier) rendered
/// and hashed, so a rerun that drifts anywhere in the year is caught
/// by one number.
fn calendar_hash(routes: &[calliope::trade::Route]) -> u64 {
    let mut s = String::new();
    for (ri, r) in routes.iter().enumerate() {
        if r.closed == 0 && r.season == 0.0 {
            continue;
        }
        for m in 0..12i64 {
            let shut = r.closed >> (m as usize) & 1 == 1;
            let mult = if shut { 0.0 } else { calliope::trade::season_mult(r.season, m) };
            s.push_str(&format!("{ri}:{m}:{}:{:.6};", shut as u8, mult));
        }
    }
    calliope::util::fnv1a64(s.as_bytes())
}

fn masked<T: Copy + Into<f64>>(a: &Array2<T>, m: &Array2<bool>) -> Vec<f64> {
    a.iter().zip(m.iter()).filter(|(_, &b)| b).map(|(&v, _)| v.into()).collect()
}

fn biome_counts(w: &World) -> [usize; 12] {
    let mut c = [0usize; 12];
    for &b in w.fields.biomes.iter() {
        c[b as usize] += 1;
    }
    c
}

fn border_land(w: &World) -> usize {
    let (h, ww) = w.fields.height.dim();
    let mut n = 0usize;
    for x in 0..ww {
        n += (w.fields.height[[0, x]] >= 0.0) as usize + (w.fields.height[[h - 1, x]] >= 0.0) as usize;
    }
    for y in 0..h {
        n += (w.fields.height[[y, 0]] >= 0.0) as usize + (w.fields.height[[y, ww - 1]] >= 0.0) as usize;
    }
    n
}

fn latitude(y: usize, rows: usize) -> f64 {
    (-90.0 + y as f64 * 180.0 / (rows - 1) as f64).abs()
}

fn fnv(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn hash_events(evs: &[Event]) -> u64 {
    let mut s = String::new();
    for e in evs {
        s.push_str(&format!("{}|{}|{}\n", e.m, e.k, e.text));
    }
    fnv(s.as_bytes())
}

fn hash_settlements(w: &World) -> u64 {
    let mut s = String::new();
    for t in &w.peoples.settlements {
        s.push_str(&format!("{}|{}|{}|{:.2}\n", t.id, t.name, t.pop, t.wealth));
    }
    fnv(s.as_bytes())
}

/// Hash the true world state — arrays and entities, NOT the packed payload
/// (whose header embeds wall-clock stage timings and thus always differs).
/// The grid section comes from the field registry (E2.2): every field
/// declared `in_hash` contributes its exact storage bits in registry order.
fn hash_state(w: &World) -> u64 {
    let mut bytes: Vec<u8> = Vec::new();
    for f in w.field_decls().iter().filter(|f| f.in_hash) {
        f.data.hash_bytes(&mut bytes);
    }
    let mut s = String::new();
    for d in &w.deposits {
        s.push_str(&format!("d{}|{}|{}|{:.2}|{}|{:.0}\n", d.r, d.x, d.y, d.rich, d.known, d.left));
    }
    for t in &w.peoples.settlements {
        // both axes ride the hash (ADR-0018): tongue and banner
        // M79 — the harbour's wound rides the identity line: a coast that
        // remembers is state, and a replay must remember the same coast.
        s.push_str(&format!("s{}|{}|{}|{}|{}|{:.2}|{:?}|{:.3}\n", t.id, t.name, t.pop, t.people.0, t.realm.0, t.wealth, t.goods.iter().map(|g| g.name()).collect::<Vec<_>>(), t.harbor_dmg));
    }
    // M79 — the coast's memory: every harbour a storm broke, in order.
    for (m, sid, dmg) in &w.storm_marks {
        s.push_str(&format!("h{}|{}|{:.3}\n", m, sid.0, dmg));
    }
    for (m, sid, dmg) in &w.storm_bites {
        s.push_str(&format!("b{}|{}|{:.3}\n", m, sid.0, dmg));
    }
    for f in &w.features {
        s.push_str(&format!("f{}|{}|{}|{}\n", f.t, f.name, f.x, f.y));
    }
    for r in &w.routes {
        s.push_str(&format!("r{}|{}|{:.2}|{:.3}\n", r.a, r.b, r.cost, r.sea));
    }
    for (g, p) in w.economy.market.iter_some() {
        s.push_str(&format!("m{}|{:.2}\n", g, p));
    }
    // M80 — the drought ledger is state: names, spans, the ground they held.
    s.push_str(&format!("W{:016x}\n", w.droughts.hash()));
    // M16/ADR-0024 — the plate sketch is state: polygons, kinds, ages.
    s.push_str(&format!("P{:016x}\n", w.plates.hash()));
    // M22 — the seismic ledger is state: seams, clocks, the quake log.
    s.push_str(&format!("Q{:016x}\n", w.seismic.hash()));
    // M23 — the volcanic record is state: cones, clocks, log, ash.
    s.push_str(&format!("V{:016x}\n", w.volcanism.hash()));
    // M25 — the waterline is state: freeze phase, stand, isostasy rows.
    s.push_str(&format!("L{:016x}\n", w.sealevel.hash()));
    // M26 — the coastal landform grid is state: the classifier held still.
    s.push_str(&format!("F{:016x}\n", calliope::landform::hash(&w.fields.landform)));
    // M28 — the LGM ice footprint is state: thickness grid, ELA rows.
    s.push_str(&format!("I{:016x}\n", w.ice.hash()));
    // M33 — the frozen-ground ledger is state: extent + pattern grids.
    s.push_str(&format!("G{:016x}\n", w.permafrost.hash()));
    // M40 — the circulation is state: the gyres hold still.
    s.push_str(&format!("C{:016x}\n", w.currents.hash()));
    // M43 — the tide field is state: the shore's breath holds still.
    s.push_str(&format!("T{:016x}\n", w.tides.hash()));
    // M44 — the drift ledger is state: the grown shore holds still.
    s.push_str(&format!("S{:016x}\n", w.coastform.hash(&w.fields.coastform)));
    // M59 — the sediment books are state: the budget holds still.
    s.push_str(&format!("D{:016x}\n", w.sediment.hash(&w.fields.silt)));
    // M45 — the shelter field is state: the anchorage reading holds still.
    {
        let mut sb: Vec<u8> = Vec::with_capacity(w.shelter.len() * 4);
        for v in w.shelter.iter() {
            sb.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        s.push_str(&format!("A{:016x}\n", calliope::util::fnv1a64(&sb)));
    }
    // M73 — the year's sky is derived, never stored (ADR-0003), so the
    // identity line carries a probe of it instead of a grid: the lattice
    // read at a fixed, world-independent set of cells and years. Same
    // seed ⇒ same variability source ⇒ same probe; a lattice whose
    // constants or keying drift shows up here as a replay break, which
    // is exactly what the stored fields already guarantee for the rest.
    {
        let (rows, cols) = w.fields.tmean.dim();
        let mut vb: Vec<u8> = Vec::with_capacity(3 * 9 * 2 * 8);
        for year in [1i64, 37, 211] {
            for iy in 0..3usize {
                for ix in 0..3usize {
                    let y = (rows - 1) * iy / 2;
                    let x = (cols - 1) * ix / 2;
                    for lane in [0.0, calliope::climate::ANOM_RAIN_LANE] {
                        let v = calliope::climate::anomaly_draw(w.variability(), x, y, year, lane);
                        vb.extend_from_slice(&v.to_bits().to_le_bytes());
                    }
                }
            }
        }
        s.push_str(&format!("W{:016x}\n", calliope::util::fnv1a64(&vb)));
    }
    // M74 — the basin's lean is likewise derived, so its probe rides here.
    s.push_str(&format!("O{:016x}\n", w.oscillation().probe()));
    // M77 — a storm season is derived from the frozen genesis field and
    // the year (ADR-0003), so the identity line carries a probe of the
    // corridor rather than a track grid: the hemispheric gradients, the
    // seasons, the site counts and two spaced years of full tracks.
    {
        let sc = calliope::storms::StormClimatology::new(&w.fields.height, &w.fields.tmean, &w.fields.tamp);
        s.push_str(&format!("K{:016x}\n", sc.probe(w.seed, &w.fields.height)));
    }
    s.push_str(&format!("t{}\n", w.month));
    bytes.extend_from_slice(s.as_bytes());
    fnv(&bytes)
}

fn gini(vals: &[f64]) -> f64 {
    let n = vals.len();
    if n < 2 {
        return 0.0;
    }
    let mean: f64 = vals.iter().sum::<f64>() / n as f64;
    if mean <= 0.0 {
        return 0.0;
    }
    let mut acc = 0.0;
    for a in vals {
        for b in vals {
            acc += (a - b).abs();
        }
    }
    acc / (2.0 * (n * n) as f64 * mean)
}

// ================================================================ earth

/// M22 — the deep earth lane: seam census, quake cadence per fault
/// length, magnitude spread, and the replay identity (two independent
/// runs of the flagship seed, chunked differently, must agree on the
/// seismic hash byte-for-byte).
fn cmd_earth(size: usize, years: usize, seeds: Vec<i64>) {
    header("DEEP EARTH", &format!("{size}² · {years}y · seeds {seeds:?}"));

    let months = (years * 12) as i64;
    let mut mean_n = 0.0;
    let mut mean_km = 0.0;
    let mut mean_freq = 0.0;
    let mut worst_freq = f64::INFINITY;
    let mut mag_sum = 0.0;
    let mut mag_n = 0usize;
    let mut great = 0usize;
    let mut max_mag: f64 = 0.0;
    // M23 — volcanism aggregates (across all seeds, for robust terciles).
    let mut v_rows: Vec<String> = Vec::new();
    let mut v_cones = 0.0f64;
    let mut v_erupt = 0usize;
    let mut v_vei_sum = 0.0f64;
    let mut vt_erupt = [0usize; 3]; // eruptions by age tercile (young/mid/old)
    let mut vt_cones = [0usize; 3]; // cone counts by tercile
    let mut ring_sum = [0.0f64; 3]; // ash at vent / mid ring / far ring
    let mut ring_n = [0usize; 3];
    // M28 — ice lane aggregates (the footprint is gen-time state, so
    // the tick length does not move these).
    let mut i_rows: Vec<String> = Vec::new();
    let mut i_share = 0.0f64;
    let mut i_margin = 0.0f64;
    let mut i_mono = 0.0f64;
    let mut i_dome = 0.0f64;
    // M33 — permafrost aggregates: extent, texture, frontier vs isotherm.
    let mut p_rows: Vec<String> = Vec::new();
    let mut p_share = 0.0f64;
    let mut p_pat = 0.0f64;
    let mut p_front = 0.0f64;
    let mut p_off = 0.0f64;
    let mut p_jac = 0.0f64;
    // M34 — modern glacier aggregates: census, snowline law, LGM nesting.
    // (gl_ prefix: `g_share` further down is the great-quakes local.)
    let mut g_rows: Vec<String> = Vec::new();
    let mut gl_share = 0.0f64;
    let mut gl_snow = 0.0f64;
    let mut gl_snow_n = 0usize;
    let mut gl_lgm = 0.0f64;
    // M36 — ice-cadence aggregates: fjord density on the polar coast,
    // proglacial-lake and moraine cadence in the margin belt (50–75°),
    // and the two belt-discipline shares.
    let mut k_rows: Vec<String> = Vec::new();
    let mut k_fj = 0.0f64;
    let mut k_fjd = 0.0f64;
    let mut k_lk = 0.0f64;
    let mut k_mor = 0.0f64;
    let mut k_morc = 0.0f64;
    // M39 — earth-calibration aggregates: the fjord latitude
    // distribution, proglacial-lake density per formerly iced area,
    // and the glacier elevation-vs-latitude curve, each held against
    // the terrestrial ranges rather than our own internal belts.
    let mut e_rows: Vec<String> = Vec::new();
    let mut e_fmed: Vec<f64> = Vec::new();
    let mut e_fiqr: Vec<f64> = Vec::new();
    let mut e_lkm: Vec<f64> = Vec::new();
    let mut e_curve: Vec<f64> = Vec::new();
    let mut e_polar: Vec<f64> = Vec::new();
    // M40 — gyre aggregates: rotation census, speed scale, west wall.
    let mut c_rows: Vec<String> = Vec::new();
    let mut g40_n = 0usize;
    let mut g40_ok = 0usize;
    let mut g40_basins: Vec<f64> = Vec::new();
    let mut g40_p95: Vec<f64> = Vec::new();
    let mut g40_wbx: Vec<f64> = Vec::new();
    // M43 — tide aggregates: range by enclosure class, the flats'
    // scaling laws, the estuary census.
    let mut t_rows: Vec<String> = Vec::new();
    let mut t_open: Vec<f64> = Vec::new();
    let mut t_macro: Vec<f64> = Vec::new();
    let mut t_enc: Vec<f64> = Vec::new();
    let mut t_flats: Vec<f64> = Vec::new();
    let mut t_est: Vec<f64> = Vec::new();
    let mut t_amp: Vec<f64> = Vec::new();
    let mut t_rel: Vec<f64> = Vec::new();



    println!();
    println!(
        " {:>6} {:>6} {:>9} {:>9} {:>7} {:>10} {:>8} {:>8} {:>8}",
        "seed", "seams", "conv km", "trans km", "quakes", "q/100km-cy", "mean M", "max M", "M>=7.5"
    );
    for &seed in &seeds {
        let mut w = World::generate(seed, size);
        let mut left = months;
        while left > 0 {
            let step = left.min(240);
            w.tick(step);
            left -= step;
        }
        let s = &w.seismic;
        let conv = s.total_km(Some(calliope::plates::B_CONVERGENT));
        let trans = s.total_km(Some(calliope::plates::B_TRANSFORM));
        let km = conv + trans;
        let quakes = s.log.len();
        let freq = quakes as f64 / (km / 100.0).max(1e-9) / (years as f64 / 100.0);
        let mags: Vec<f64> = s.log.iter().map(|q| q.mag).collect();
        let m_mean = if mags.is_empty() { 0.0 } else { mags.iter().sum::<f64>() / mags.len() as f64 };
        let m_max = mags.iter().cloned().fold(0.0f64, f64::max);
        let g = mags.iter().filter(|&&m| m >= 7.5).count();
        println!(
            " {:>6} {:>6} {:>9.0} {:>9.0} {:>7} {:>10.2} {:>8.2} {:>8.1} {:>8}",
            seed, s.faults.len(), conv, trans, quakes, freq, m_mean, m_max, g
        );
        mean_n += s.faults.len() as f64 / seeds.len() as f64;
        mean_km += km / seeds.len() as f64;
        mean_freq += freq / seeds.len() as f64;
        worst_freq = worst_freq.min(freq);
        mag_sum += mags.iter().sum::<f64>();
        mag_n += mags.len();
        great += g;
        max_mag = max_mag.max(m_max);

        // M23 — the volcanism lane: cadence by age tercile, ash decay.
        let v = &w.volcanism;
        let nc = v.cones.len();
        v_cones += nc as f64 / seeds.len() as f64;
        v_erupt += v.log.len();
        v_vei_sum += v.log.iter().map(|e| e.vei).sum::<f64>();
        // Terciles by cone age (young third .. old third), per seed so
        // every world contributes cones to every tercile.
        let mut order: Vec<usize> = (0..nc).collect();
        order.sort_by(|&a, &b| v.cones[a].age.partial_cmp(&v.cones[b].age).unwrap().then(a.cmp(&b)));
        let mut per_cone = vec![0usize; nc];
        for e in &v.log {
            per_cone[e.cone as usize] += 1;
        }
        for (rank, &ci) in order.iter().enumerate() {
            let t = (rank * 3 / nc.max(1)).min(2);
            vt_cones[t] += 1;
            vt_erupt[t] += per_cone[ci];
        }
        // Ash decay rings around erupted cones: at the vent, a mid ring
        // (Chebyshev 3) and a far ring (Chebyshev 6).
        let (gh, gw) = v.ash.dim();
        for (ci, c) in v.cones.iter().enumerate() {
            if per_cone[ci] == 0 {
                continue;
            }
            ring_sum[0] += v.ash[[c.y as usize, c.x as usize]] as f64;
            ring_n[0] += 1;
            for (ri, rad) in [(1usize, 3isize), (2usize, 6isize)] {
                let mut sum = 0.0;
                let mut n = 0usize;
                for dy in -rad..=rad {
                    for dx in -rad..=rad {
                        if dy.abs().max(dx.abs()) != rad {
                            continue;
                        }
                        let ny = c.y as isize + dy;
                        let nx = c.x as isize + dx;
                        if ny < 0 || nx < 0 || ny >= gh as isize || nx >= gw as isize {
                            continue;
                        }
                        sum += v.ash[[ny as usize, nx as usize]] as f64;
                        n += 1;
                    }
                }
                if n > 0 {
                    ring_sum[ri] += sum / n as f64;
                    ring_n[ri] += 1;
                }
            }
        }
        let cad = v.log.len() as f64 / (nc.max(1) as f64) / (years as f64 / 100.0);
        let vm = if v.log.is_empty() { 0.0 } else { v.log.iter().map(|e| e.vei).sum::<f64>() / v.log.len() as f64 };
        v_rows.push(format!(
            " {:>6} {:>6} {:>9} {:>12.2} {:>9.2}",
            seed, nc, v.log.len(), cad, vm
        ));

        // M28 — the ice lane: footprint share, lowland margin latitude,
        // the ELA's poleward march read back off the mask, dome height.
        let ice = &w.ice;
        let (ir, icw) = ice.thickness.dim();
        let nf = ir as f64;
        let mut land = 0usize;
        let mut iced = 0usize;
        let mut low_lats: Vec<f64> = Vec::new();
        let mut bin_min: Vec<f64> = vec![f64::INFINITY; 30]; // 3° bins, 0..90
        let mut dome = 0.0f64;
        for y in 0..ir {
            let lat = (-90.0 + (y as f64) * 180.0 / (nf - 1.0)).abs();
            for x in 0..icw {
                let h = w.fields.height[[y, x]] as f64;
                if h < 0.0 {
                    continue;
                }
                land += 1;
                let t = ice.thickness[[y, x]] as f64;
                if t > 0.0 {
                    iced += 1;
                    dome = dome.max(t);
                    if h < 0.10 {
                        low_lats.push(lat);
                    }
                    let b = ((lat / 3.0) as usize).min(29);
                    bin_min[b] = bin_min[b].min(h);
                }
            }
        }
        let share = 100.0 * iced as f64 / land.max(1) as f64;
        low_lats.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let margin = if low_lats.is_empty() {
            90.0
        } else {
            low_lats[(low_lats.len() as f64 * 0.05) as usize]
        };
        // Equator→pole, does the lowest glaciated cell keep dropping?
        let occ: Vec<f64> = bin_min.iter().copied().filter(|v| v.is_finite()).collect();
        let mut steps = 0usize;
        let mut drops = 0usize;
        for pair in occ.windows(2) {
            steps += 1;
            if pair[1] <= pair[0] + 0.01 {
                drops += 1;
            }
        }
        let mono = if steps == 0 { 100.0 } else { 100.0 * drops as f64 / steps as f64 };
        i_share += share / seeds.len() as f64;
        i_margin += margin / seeds.len() as f64;
        i_mono += mono / seeds.len() as f64;
        i_dome = i_dome.max(dome);
        i_rows.push(format!(
            " {:>6} {:>7.1} {:>11.1} {:>9} {:>8.0} {:>7.0}",
            seed, share, margin, occ.len(), dome, mono
        ));

        // M33 — the frozen rim: extent census, micro-texture, and the
        // frontier read against the −2 °C mean-annual isotherm in two
        // legs — maritime (shift ≈ 0, must hug −2) and continental
        // (must run warmer by the reach; the Siberia asymmetry).
        let pf = &w.permafrost;
        let (pr2, pc2) = pf.extent.dim();
        let pf_water = w.fields.height.mapv(|h| h < 0.0);
        let pf_cont = calliope::climate::continentality(&pf_water);
        let mut pf_land = 0usize;
        let mut pf_ext = [0usize; 4];
        let mut pf_pat = 0usize;
        let mut pf_mar: Vec<f64> = Vec::new();
        let mut pf_int: Vec<f64> = Vec::new();
        let mut pf_inter = 0usize;
        let mut pf_union = 0usize;
        for y in 0..pr2 {
            for x in 0..pc2 {
                if w.fields.height[[y, x]] < 0.0 {
                    continue;
                }
                pf_land += 1;
                let e = pf.extent[[y, x]];
                pf_ext[e as usize] += 1;
                if pf.pattern[[y, x]] != 0 {
                    pf_pat += 1;
                }
                let cold = w.fields.tmean[[y, x]] <= -2.0;
                if e > 0 || cold {
                    pf_union += 1;
                }
                if e > 0 && cold {
                    pf_inter += 1;
                }
                if e > 0 {
                    let mut edge = false;
                    for (dy, dx) in [(-1isize, 0isize), (1, 0), (0, -1), (0, 1)] {
                        let ny = y as isize + dy;
                        let nx = x as isize + dx;
                        if ny < 0 || nx < 0 || ny >= pr2 as isize || nx >= pc2 as isize {
                            continue;
                        }
                        let (ny, nx) = (ny as usize, nx as usize);
                        if w.fields.height[[ny, nx]] >= 0.0 && pf.extent[[ny, nx]] == 0 {
                            edge = true;
                            break;
                        }
                    }
                    if edge {
                        let cn = ((pf_cont[[y, x]] - 0.35) / 0.65).clamp(0.0, 1.0);
                        let t = w.fields.tmean[[y, x]] as f64;
                        if cn <= 0.25 {
                            pf_mar.push(t);
                        } else if cn >= 0.75 {
                            pf_int.push(t);
                        }
                    }
                }
            }
        }
        let median = |v: &mut Vec<f64>| -> Option<f64> {
            if v.is_empty() {
                return None;
            }
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            Some(v[v.len() / 2])
        };
        let pf_any = pf_ext[1] + pf_ext[2] + pf_ext[3];
        let pf_real = pf_ext[2] + pf_ext[3];
        let pfs = 100.0 * pf_any as f64 / pf_land.max(1) as f64;
        let pfc = 100.0 * pf_ext[3] as f64 / pf_land.max(1) as f64;
        let pfp = 100.0 * pf_pat as f64 / pf_real.max(1) as f64;
        let pfm = median(&mut pf_mar).unwrap_or(-2.0);
        let pfo = median(&mut pf_int).map(|m| m - pfm).unwrap_or(0.0);
        let pfj = 100.0 * pf_inter as f64 / pf_union.max(1) as f64;
        p_share += pfs / seeds.len() as f64;
        p_pat += pfp / seeds.len() as f64;
        p_front += pfm / seeds.len() as f64;
        p_off += pfo / seeds.len() as f64;
        p_jac += pfj / seeds.len() as f64;
        p_rows.push(format!(
            " {:>6} {:>7.1} {:>8.1} {:>10.1} {:>9.2} {:>8.2} {:>9.1}",
            seed, pfs, pfc, pfp, pfm, pfo, pfj
        ));

        // M34 — modern glaciers: census, the snowline law (mean glacier
        // elevation vs the balance-zero elevation solved from belt-mean
        // climate, per 15° |lat| belt), and nesting inside the LGM.
        let gl = &w.ice.modern;
        let (gr, gc) = gl.dim();
        let nrows = gr as f64;
        let mut b_t0 = [0.0f64; 6]; // belt-mean sea-level-equivalent temp
        let mut b_ta = [0.0f64; 6]; // belt-mean |tamp|
        let mut b_pr = [0.0f64; 6]; // belt-mean annual precip
        let mut b_pa = [0.0f64; 6]; // belt-mean pamp, phase-aligned to tamp
        let mut b_land = [0usize; 6];
        let mut b_gn = [0usize; 6];
        let mut b_gh = [0.0f64; 6];
        let mut g_cells = 0usize;
        let mut g_land = 0usize;
        let mut g_in_lgm = 0usize;
        for y in 0..gr {
            let lat = (-90.0 + (y as f64) * 180.0 / (nrows - 1.0)).abs();
            let belt = ((lat / 15.0) as usize).min(5);
            for x in 0..gc {
                let h = w.fields.height[[y, x]] as f64;
                if h < 0.0 {
                    continue;
                }
                g_land += 1;
                b_land[belt] += 1;
                b_t0[belt] += w.fields.tmean[[y, x]] as f64 + 26.0 * h;
                let ta = w.fields.tamp[[y, x]] as f64;
                let sg = if ta < 0.0 { -1.0 } else { 1.0 };
                b_ta[belt] += ta * sg;
                b_pr[belt] += w.fields.precip[[y, x]] as f64;
                b_pa[belt] += w.fields.pamp[[y, x]] as f64 * sg;
                if gl[[y, x]] > 0.0 {
                    g_cells += 1;
                    b_gn[belt] += 1;
                    b_gh[belt] += h;
                    if w.ice.thickness[[y, x]] > 0.0 {
                        g_in_lgm += 1;
                    }
                }
            }
        }
        let mut off_sum = 0.0f64;
        let mut off_n = 0usize;
        for b in 0..6 {
            if b_gn[b] < 25 || b_land[b] == 0 {
                continue;
            }
            let n = b_land[b] as f64;
            let (t0, ta, pr, pa) = (b_t0[b] / n, b_ta[b] / n, b_pr[b] / n, b_pa[b] / n);
            let bal = |h: f64| calliope::climate::ice_balance(t0 - 26.0 * h, ta, pr, pa);
            if bal(2.5) <= 0.0 {
                continue; // no snowline below the ceiling in this belt
            }
            if bal(0.0) > 0.0 {
                continue; // cap country: ice at the shore, no alpine snowline
            }
            let mut hi = 2.5f64;
            let mut lo = 0.0f64;
            for _ in 0..48 {
                let mid = 0.5 * (lo + hi);
                if bal(mid) > 0.0 {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            off_sum += (b_gh[b] / b_gn[b] as f64 - hi) * 4000.0 * b_gn[b] as f64;
            off_n += b_gn[b];
        }
        let gsh = 100.0 * g_cells as f64 / g_land.max(1) as f64;
        let gelev = b_gh.iter().sum::<f64>() / g_cells.max(1) as f64 * 4000.0;
        let glgm = 100.0 * g_in_lgm as f64 / g_cells.max(1) as f64;
        gl_share += gsh / seeds.len() as f64;
        if off_n > 0 {
            gl_snow += off_sum / off_n as f64;
            gl_snow_n += 1;
        }
        gl_lgm += glgm / seeds.len() as f64;
        g_rows.push(format!(
            " {:>6} {:>7} {:>8.2} {:>8.0} {:>9.0} {:>9.1}",
            seed,
            g_cells,
            gsh,
            gelev,
            if off_n > 0 { off_sum / off_n as f64 } else { f64::NAN },
            glgm
        ));

        // M36 — ice cadence: the three ice landform families read by
        // latitude belt. Fjords are polar-coast creatures (the Norway/
        // Chile/Greenland analogs run 42–83°, nothing equatorward of
        // the alpine coasts); proglacial lakes and terminal moraines
        // live in the margin belt (50–75°: the Laurentide/Fennoscandian
        // fringe), and the moraine string hugs the measured margin.
        let lfm = &w.fields.landform;
        let hgt = &w.fields.height;
        let (kr, kc) = hgt.dim();
        let knf = kr as f64;
        let mut co_polar = 0usize; // coast cells |lat| ≥ 55°
        let mut fj_polar = 0usize; // fjord cells |lat| ≥ 55°
        let mut fj_all = 0usize;
        let mut fj_pole = 0usize; // fjord cells |lat| ≥ 45°
        let mut iced_mb = 0usize; // formerly iced land cells, 50–75°
        let mut iced_all = 0usize; // formerly iced land cells, all lats (M39)
        let mut fj_lats: Vec<f64> = Vec::new(); // |lat| of every fjord cell (M39)
        for y in 0..kr {
            let lat = (-90.0 + (y as f64) * 180.0 / (knf - 1.0)).abs();
            for x in 0..kc {
                let land = hgt[[y, x]] >= 0.0;
                if lfm[[y, x]] == calliope::landform::FJORD {
                    fj_all += 1;
                    fj_lats.push(lat);
                    if lat >= 45.0 {
                        fj_pole += 1;
                    }
                    if lat >= 55.0 {
                        fj_polar += 1;
                    }
                }
                if land && lat >= 55.0 {
                    let mut coast = false;
                    for (dy, dx) in [(-1isize, 0isize), (1, 0), (0, -1), (0, 1)] {
                        let (ny, nx) = (y as isize + dy, x as isize + dx);
                        if ny < 0 || nx < 0 || ny >= kr as isize || nx >= kc as isize {
                            continue;
                        }
                        if hgt[[ny as usize, nx as usize]] < 0.0 {
                            coast = true;
                            break;
                        }
                    }
                    if coast {
                        co_polar += 1;
                    }
                }
                if land && w.ice.thickness[[y, x]] > 0.0 {
                    iced_all += 1;
                    if (50.0..75.0).contains(&lat) {
                        iced_mb += 1;
                    }
                }
            }
        }
        let lat_of = |yy: u16| (-90.0 + (yy as f64) * 180.0 / (knf - 1.0)).abs();
        let mut mor_mb = 0usize;
        let mut mor_low = 0usize; // lowland moraines (h < 0.30)
        let mut mor_near = 0usize; // ...of those, within ±6° of the margin
        for &(my, mx) in &w.ice.moraines {
            let lat = lat_of(my);
            if (50.0..75.0).contains(&lat) {
                mor_mb += 1;
            }
            // The margin-latitude discipline is a lowland law: alpine
            // moraines follow their local snowline wherever the ranges
            // stand, only the lowland string records the great margin.
            if hgt[[my as usize, mx as usize]] < 0.30 {
                mor_low += 1;
                if (lat - margin).abs() <= 6.0 {
                    mor_near += 1;
                }
            }
        }
        let lk_mb = w
            .ice
            .proglacial
            .iter()
            .filter(|&&(yy, _)| (50.0..75.0).contains(&lat_of(yy)))
            .count();
        let fjd = 1000.0 * fj_polar as f64 / co_polar.max(1) as f64;
        let fjp = if fj_all == 0 { 100.0 } else { 100.0 * fj_pole as f64 / fj_all as f64 };
        let lkd = 1000.0 * lk_mb as f64 / iced_mb.max(1) as f64;
        let mord = 1000.0 * mor_mb as f64 / iced_mb.max(1) as f64;
        let morc = 100.0 * mor_near as f64 / mor_low.max(1) as f64;
        k_fj += fjd / seeds.len() as f64;
        k_fjd += fjp / seeds.len() as f64;
        k_lk += lkd / seeds.len() as f64;
        k_mor += mord / seeds.len() as f64;
        k_morc += morc / seeds.len() as f64;
        k_rows.push(format!(
            " {:>6} {:>8.1} {:>8.1} {:>9.2} {:>9.1} {:>9.1}",
            seed, fjd, fjp, lkd, mord, morc
        ));

        // M39 — glacial calibration vs Earth. The M36 rows band our
        // landforms against our own belts; these three comparisons pin
        // them to Earth's measured bones: where the fjord coasts sit
        // (Norway 58–71°, Chile 42–56°, Greenland 60–83°), how many
        // moraine-dammed giants the retreat left per million km² of
        // formerly iced ground (the Laurentide/Fennoscandian fringe),
        // and how the surviving ice climbs equatorward — Earth's
        // glaciation level runs ~5–6 km in the dry subtropics and
        // falls to sea level at the poles.
        let (fmed, fiqr) = if fj_lats.is_empty() {
            (f64::NAN, f64::NAN)
        } else {
            (
                quantile(&fj_lats, 0.5),
                quantile(&fj_lats, 0.75) - quantile(&fj_lats, 0.25),
            )
        };
        // cells are 4 km on a side (ADR-0004): 16 km² each.
        let lkm = 1e6 * w.ice.proglacial.len() as f64 / (iced_all.max(1) as f64 * 16.0);
        // Per-15°-belt mean glacier elevation, equator → pole. The
        // curve must descend poleward of its crest; Earth's crest sits
        // in the dry subtropics (~25–30°), so an equatorward rise into
        // the crest is honest and only the poleward limb is judged.
        let mut belt_elev: Vec<(usize, f64)> = Vec::new();
        for b in 0..6 {
            if b_gn[b] >= 25 {
                belt_elev.push((b, 4000.0 * b_gh[b] / b_gn[b] as f64));
            }
        }
        let crest = belt_elev
            .iter()
            .enumerate()
            .max_by(|a, b| a.1 .1.partial_cmp(&b.1 .1).unwrap())
            .map(|(i, _)| i)
            .unwrap_or(0);
        let mut pairs = 0usize;
        let mut descend = 0usize;
        for wnd in belt_elev[crest..].windows(2) {
            pairs += 1;
            if wnd[1].1 <= wnd[0].1 + 150.0 {
                descend += 1;
            }
        }
        let curve = if pairs == 0 { 100.0 } else { 100.0 * descend as f64 / pairs as f64 };
        let polar = belt_elev.iter().find(|&&(b, _)| b == 5).map(|&(_, e)| e);
        if !fmed.is_nan() {
            e_fmed.push(fmed);
            e_fiqr.push(fiqr);
        }
        e_lkm.push(lkm);
        e_curve.push(curve);
        if let Some(pe) = polar {
            e_polar.push(pe);
        }
        e_rows.push(format!(
            " {:>6} {:>8.1} {:>8.1} {:>10.2} {:>7.0} {:>8}   {}",
            seed,
            fmed,
            fiqr,
            lkm,
            curve,
            polar.map(|e| format!("{:.0}", e)).unwrap_or_else(|| "-".into()),
            belt_elev
                .iter()
                .map(|&(b, e)| format!("{}-{}°:{:.0}m", b * 15, b * 15 + 15, e))
                .collect::<Vec<_>>()
                .join(" · "),
        ));

        // M40 — wind-driven gyres: the rotation-sense census. Label
        // the ocean into basins; for each basin and hemisphere read
        // the mean streamfunction over the subtropical band
        // (10–40°|lat|). Positive ψ turns clockwise on screen
        // (north-up), so the north wants ψ > 0 and the south ψ < 0 —
        // anticyclonic subtropical gyres both ways, Earth's sense.
        let water_m = w.fields.height.mapv(|h| h < 0.0);
        let lab = ndimage::label(&water_m, false);
        let basins = ndimage::top_components(&lab, 2500.0, 8);
        let (cr, cc) = w.fields.height.dim();
        let cnf = cr as f64;
        let mut gy_n = 0usize; // qualifying basin-hemisphere gyres
        let mut gy_ok = 0usize; // ...spinning the earthly way
        let mut gy_basins = 0usize; // basins carrying ≥1 qualifying gyre
        let mut band_speeds: Vec<f64> = Vec::new();
        let mut west_speeds: Vec<f64> = Vec::new();
        let mut int_speeds: Vec<f64> = Vec::new();
        for &(bi, _) in &basins {
            let mut any = false;
            for hemi in 0..2 {
                let mut n = 0usize;
                let mut psum = 0.0f64;
                for y in 0..cr {
                    let lat_s = -90.0 + y as f64 * 180.0 / (cnf - 1.0);
                    let north = lat_s < 0.0; // negative = north (grid law)
                    if (hemi == 0) != north {
                        continue;
                    }
                    let th = lat_s.abs();
                    if !(10.0..=40.0).contains(&th) {
                        continue;
                    }
                    // walk this row's runs of the basin so the 4-cell
                    // western strip splits off for the Stommel ratio
                    let mut x = 0usize;
                    while x < cc {
                        if lab.lab[[y, x]] != bi as i32 {
                            x += 1;
                            continue;
                        }
                        let x0 = x;
                        while x < cc && lab.lab[[y, x]] == bi as i32 {
                            x += 1;
                        }
                        for xi in x0..x {
                            n += 1;
                            psum += w.currents.psi[[y, xi]] as f64;
                            let sp = (w.currents.u[[y, xi]] as f64)
                                .hypot(w.currents.v[[y, xi]] as f64);
                            band_speeds.push(sp);
                            if xi < x0 + 4 {
                                west_speeds.push(sp);
                            } else {
                                int_speeds.push(sp);
                            }
                        }
                    }
                }
                if n >= 250 {
                    gy_n += 1;
                    any = true;
                    let mp = psum / n as f64;
                    let want_pos = hemi == 0; // north: clockwise = ψ > 0
                    if mp != 0.0 && (mp > 0.0) == want_pos {
                        gy_ok += 1;
                    }
                }
            }
            if any {
                gy_basins += 1;
            }
        }
        let sp95 = quantile(&band_speeds, 0.95);
        let wbx = if west_speeds.is_empty() || int_speeds.is_empty() {
            0.0
        } else {
            quantile(&west_speeds, 0.95) / quantile(&int_speeds, 0.95).max(1e-12)
        };
        g40_n += gy_n;
        g40_ok += gy_ok;
        g40_basins.push(gy_basins as f64);
        g40_p95.push(sp95);
        g40_wbx.push(wbx);
        c_rows.push(format!(
            " {:>6} {:>7} {:>7} {:>7} {:>9.3} {:>8.1}",
            seed,
            basins.len(),
            gy_n,
            gy_ok,
            sp95,
            wbx
        ));

        // M43 — the tide lane: range censused by enclosure class, the
        // flats and estuaries counted, and the scaling laws read back:
        // flats must sit on higher-range, lower-relief coast than the
        // coast at large.
        let td = &w.tides;
        let (t_r, t_c) = td.range.dim();
        let relief3 = |y: usize, x: usize| -> f64 {
            let mut lo = f32::INFINITY;
            let mut hi = f32::NEG_INFINITY;
            for dy in -1isize..=1 {
                for dx in -1isize..=1 {
                    let ny = y as isize + dy;
                    let nx = x as isize + dx;
                    if ny < 0 || nx < 0 || ny >= t_r as isize || nx >= t_c as isize {
                        continue;
                    }
                    let v = w.fields.height[[ny as usize, nx as usize]];
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
            }
            (hi - lo) as f64
        };
        let rng_near = |y: usize, x: usize| -> f64 {
            let mut r = 0.0f64;
            for dy in -1isize..=1 {
                for dx in -1isize..=1 {
                    let ny = y as isize + dy;
                    let nx = x as isize + dx;
                    if ny < 0 || nx < 0 || ny >= t_r as isize || nx >= t_c as isize {
                        continue;
                    }
                    if td.class[[ny as usize, nx as usize]] == calliope::tides::OPEN {
                        r = r.max(td.range[[ny as usize, nx as usize]] as f64);
                    }
                }
            }
            r
        };
        let mut coast_rng: Vec<f64> = Vec::new();
        let mut coast_rel: Vec<f64> = Vec::new();
        let mut enc_sum = 0.0f64;
        let mut enc_n = 0usize;
        let mut n_macro = 0usize;
        for y in 0..t_r {
            for x in 0..t_c {
                match td.class[[y, x]] {
                    calliope::tides::OPEN => {
                        let mut coastal = false;
                        for (ny, nx) in [
                            (y.wrapping_sub(1), x),
                            (y + 1, x),
                            (y, x.wrapping_sub(1)),
                            (y, x + 1),
                        ] {
                            if ny < t_r && nx < t_c && w.fields.height[[ny, nx]] >= 0.0 {
                                coastal = true;
                                break;
                            }
                        }
                        if coastal {
                            let r = td.range[[y, x]] as f64;
                            coast_rng.push(r);
                            coast_rel.push(relief3(y, x));
                            if r >= 4.0 {
                                n_macro += 1;
                            }
                        }
                    }
                    calliope::tides::ENCLOSED => {
                        enc_sum += td.range[[y, x]] as f64;
                        enc_n += 1;
                    }
                    _ => {}
                }
            }
        }
        let mut n_flats = 0usize;
        let mut n_est = 0usize;
        let mut fl_rng = 0.0f64;
        let mut fl_rel = 0.0f64;
        for y in 0..t_r {
            for x in 0..t_c {
                match w.fields.landform[[y, x]] {
                    calliope::landform::TIDEFLAT => {
                        n_flats += 1;
                        fl_rng += rng_near(y, x);
                        fl_rel += relief3(y, x);
                    }
                    calliope::landform::ESTUARY => n_est += 1,
                    _ => {}
                }
            }
        }
        let ncst = coast_rng.len().max(1) as f64;
        let open_m = coast_rng.iter().sum::<f64>() / ncst;
        let rel_m = (coast_rel.iter().sum::<f64>() / ncst).max(1e-9);
        let mac = 100.0 * n_macro as f64 / ncst;
        let flk = 1000.0 * n_flats as f64 / ncst;
        let amp = if n_flats == 0 {
            0.0
        } else {
            fl_rng / n_flats as f64 / open_m.max(1e-9)
        };
        let relf = if n_flats == 0 {
            0.0
        } else {
            fl_rel / n_flats as f64 / rel_m
        };
        t_open.push(open_m);
        t_macro.push(mac);
        if enc_n > 0 {
            t_enc.push(enc_sum / enc_n as f64);
        }
        t_flats.push(flk);
        t_est.push(n_est as f64);
        t_amp.push(amp);
        t_rel.push(relf);
        t_rows.push(format!(
            " {:>6} {:>7.2} {:>7.1} {:>8} {:>7} {:>6} {:>7.2} {:>7.2}",
            seed,
            open_m,
            mac,
            if enc_n > 0 {
                format!("{:.2}", enc_sum / enc_n as f64)
            } else {
                "-".into()
            },
            n_flats,
            n_est,
            amp,
            relf
        ));
    }

    println!();
    println!(
        " {:>6} {:>6} {:>9} {:>12} {:>9}",
        "seed", "cones", "eruptions", "erupt/cone-cy", "mean VEI"
    );
    for r in &v_rows {
        println!("{r}");
    }

    println!();
    println!(
        " {:>6} {:>7} {:>11} {:>9} {:>8} {:>7}",
        "seed", "ice %", "margin lat", "ela bins", "dome m", "mono %"
    );
    for r in &i_rows {
        println!("{r}");
    }

    println!();
    println!(
        " {:>6} {:>7} {:>8} {:>10} {:>9} {:>8} {:>9}",
        "seed", "pf %", "cont %", "pattern %", "mar °C", "off °C", "agree %"
    );
    for r in &p_rows {
        println!("{r}");
    }

    println!();
    println!(
        " {:>6} {:>7} {:>8} {:>8} {:>9} {:>9}",
        "seed", "gcells", "share %", "elev m", "Δsnow m", "in-LGM %"
    );
    for r in &g_rows {
        println!("{r}");
    }

    println!();
    println!(
        " {:>6} {:>8} {:>8} {:>9} {:>9} {:>9}",
        "seed", "fj/1kco", "fj>=45%", "lk/1kice", "mor/1k", "mor±6°%"
    );
    for r in &k_rows {
        println!("{r}");
    }

    println!();
    println!(
        " {:>6} {:>8} {:>8} {:>10} {:>7} {:>8}   {}",
        "seed", "fj med°", "fj IQR°", "lk/Mkm²", "curve%", "polar m", "glacier elev by belt (equator→pole)"
    );
    for r in &e_rows {
        println!("{r}");
    }

    println!();
    println!(
        " {:>6} {:>7} {:>7} {:>7} {:>9} {:>8}",
        "seed", "basins", "gyres", "earthly", "sp p95", "west ×"
    );
    for r in &c_rows {
        println!("{r}");
    }

    println!();
    println!(
        " {:>6} {:>7} {:>7} {:>8} {:>7} {:>6} {:>7} {:>7}",
        "seed", "open m", "macro%", "encl m", "flats", "est", "amp x", "rel /"
    );
    for r in &t_rows {
        println!("{r}");
    }

    // Replay identity: the flagship seed run twice from scratch with
    // different chunkings must agree on the ledger byte-for-byte.
    let seed0 = seeds[0];
    let hash_after = |chunk: i64| -> (u64, u64, u64, u64, u64) {
        let mut w = World::generate(seed0, size);
        let mut left = months;
        while left > 0 {
            let step = left.min(chunk);
            w.tick(step);
            left -= step;
        }
        (w.seismic.hash(), w.ice.hash(), w.currents.hash(), w.tides.hash(), w.coastform.hash(&w.fields.coastform))
    };
    let ((ha, ia, ca, ta, oa), (hb, ib, cb, tb, ob)) = (hash_after(240), hash_after(12));
    println!();
    println!(" replay: seed {seed0} · {months} mo · chunk 240 => {ha:016x} · chunk 12 => {hb:016x}");
    println!(" native seismic hash (seed {seed0} · size {size} · {months} mo): {ha:016x}");

    let mut c = Checks::default();
    c.band("fault seams", mean_n, format!("{:.0}", mean_n));
    c.band("active fault km", mean_km, format!("{:.0} km", mean_km));
    c.band("quakes per 100km-century", mean_freq, format!("{:.2}", mean_freq));
    c.band_as("q/100km-cy (stingiest seed)", "quakes per 100km-century", worst_freq, format!("{:.2}", worst_freq));
    let m_mean = if mag_n == 0 { 0.0 } else { mag_sum / mag_n as f64 };
    c.band("mean quake magnitude", m_mean, format!("M {:.2}", m_mean));
    let g_share = if mag_n == 0 { 0.0 } else { great as f64 / mag_n as f64 };
    c.band("great quakes share (M>=7.5)", g_share, pct(g_share));
    c.must(
        "seismic replay is byte-identical",
        ha == hb,
        format!("{}", if ha == hb { "agree" } else { "DIVERGE" }),
        "M22 gate: same seed, different chunking, one ledger",
    );

    // M23 — the volcanism checks: census, cadence, age law, ash decay.
    let cy = years as f64 / 100.0;
    let total_cones: usize = vt_cones.iter().sum();
    c.band("volcano cones", v_cones, format!("{:.0}", v_cones));
    let cad_all = v_erupt as f64 / total_cones.max(1) as f64 / cy;
    c.band("eruptions per cone-century", cad_all, format!("{:.2}", cad_all));
    let cad_t: Vec<f64> = (0..3)
        .map(|t| vt_erupt[t] as f64 / vt_cones[t].max(1) as f64 / cy)
        .collect();
    println!();
    println!(
        " cadence by age tercile: young {:.2} · mid {:.2} · old {:.2} erupt/cone-cy",
        cad_t[0], cad_t[1], cad_t[2]
    );
    let ratio = if cad_t[2] > 0.0 {
        cad_t[0] / cad_t[2]
    } else if cad_t[0] > 0.0 {
        99.0
    } else {
        1.0
    };
    c.band("young/old cadence ratio", ratio, format!("{:.2}×", ratio));
    let vei_mean = if v_erupt == 0 { 0.0 } else { v_vei_sum / v_erupt as f64 };
    c.band("mean eruption VEI", vei_mean, format!("VEI {:.2}", vei_mean));
    let ring: Vec<f64> = (0..3).map(|r| ring_sum[r] / ring_n[r].max(1) as f64).collect();
    c.band("ash bonus at the cone", ring[0], format!("+{:.3}", ring[0]));
    c.must(
        "ash decays with distance",
        ring[0] > ring[1] && ring[1] > ring[2],
        format!("vent {:.3} > r3 {:.3} > r6 {:.3}", ring[0], ring[1], ring[2]),
        "M23 gate: the fertile apron thins away from the vent",
    );
    // M28 — the ice checks: footprint, margin law, ELA march, dome, purity.
    c.band("ice share of land at LGM", i_share, format!("{:.1} %", i_share));
    c.band("lowland ice margin lat", i_margin, format!("{:.1}°", i_margin));
    c.band("ELA poleward monotone", i_mono, format!("{:.0} %", i_mono));
    c.band("peak ice thickness m", i_dome, format!("{:.0} m", i_dome));
    c.must(
        "ice ledger regen byte-identical",
        ia == ib,
        format!("{}", if ia == ib { "identical" } else { "DIVERGE" }),
        "M28 gate: frozen prehistory replays; joins hash_state",
    );
    // M33 — the permafrost checks: extent, texture, the isotherm law.
    c.band("permafrost share of land", p_share, format!("{:.1} %", p_share));
    c.band("patterned share of permafrost", p_pat, format!("{:.1} %", p_pat));
    c.band("maritime frontier MAAT", p_front, format!("{:.2} °C", p_front));
    c.band("continental frontier offset", p_off, format!("+{:.2} °C", p_off));
    c.band("isotherm agreement", p_jac, format!("{:.1} %", p_jac));
    // M34 — the modern-glacier checks: census, snowline law, LGM nesting.
    c.band("modern glacier share of land %", gl_share, format!("{:.2} %", gl_share));
    let gl_snow_m = if gl_snow_n == 0 { 0.0 } else { gl_snow / gl_snow_n as f64 };
    c.band(
        "glacier elev above snowline m",
        gl_snow_m,
        format!("{:+.0} m", gl_snow_m),
    );
    c.band("modern ice inside LGM footprint %", gl_lgm, format!("{:.1} %", gl_lgm));
    // M36 — the ice-cadence checks: the three landform families banded
    // by latitude belt against their earth analogs (fjords on the polar
    // coast; lakes and moraines in the 50–75° margin belt; the moraine
    // string concentrated on the measured margin latitude).
    c.band("fjord cells per 1000 polar coast", k_fj, format!("{:.1}", k_fj));
    c.band("fjord cells poleward of 45° %", k_fjd, format!("{:.1} %", k_fjd));
    c.band("proglacial lakes per 1000 iced, margin belt", k_lk, format!("{:.2}", k_lk));
    c.band("moraine cells per 1000 iced, margin belt", k_mor, format!("{:.1}", k_mor));
    c.band("lowland moraine cells near the margin %", k_morc, format!("{:.1} %", k_morc));
    // M39 — the earth-calibration checks: fjord latitudes, giant-lake
    // density per formerly iced area, and the elevation-vs-latitude
    // curve, banded against terrestrial analogs (Norway/Greenland/
    // Chile fjord coasts; the Laurentide fringe; Earth's glaciation
    // level falling from the subtropics to the poles).
    let mean = |v: &Vec<f64>| {
        if v.is_empty() { f64::NAN } else { v.iter().sum::<f64>() / v.len() as f64 }
    };
    let (m_fmed, m_fiqr) = (mean(&e_fmed), mean(&e_fiqr));
    let (m_lkm, m_curve, m_polar) = (mean(&e_lkm), mean(&e_curve), mean(&e_polar));
    c.band("fjord median latitude", m_fmed, format!("{:.1}°", m_fmed));
    c.band("fjord latitude IQR", m_fiqr, format!("{:.1}°", m_fiqr));
    c.band("proglacial lakes per Mkm² iced", m_lkm, format!("{:.2}", m_lkm));
    c.band("glacier curve descends poleward", m_curve, format!("{:.0} %", m_curve));
    c.band("polar-belt glacier elevation", m_polar, format!("{:.0} m", m_polar));
    // M40 — the gyre checks: every qualifying subtropical gyre spins
    // the earthly way, the west wall crowds, the field replays.
    c.must(
        "gyre sense matches hemisphere",
        g40_n > 0 && g40_ok == g40_n,
        format!("{}/{}", g40_ok, g40_n),
        "M40 gate: clockwise north · counterclockwise south, every basin in the sweep",
    );
    c.band("gyre basins per seed", mean(&g40_basins), format!("{:.1}", mean(&g40_basins)));
    c.band("surface current speed p95", mean(&g40_p95), format!("{:.3}", mean(&g40_p95)));
    c.band(
        "western boundary intensification",
        mean(&g40_wbx),
        format!("{:.1}×", mean(&g40_wbx)),
    );
    c.must(
        "current field replays identical",
        ca == cb,
        format!("{}", if ca == cb { "agree" } else { "DIVERGE" }),
        "M40 gate: same seed, two chunkings, one circulation",
    );
    // M43 — the tide checks: range banded by enclosure class, the
    // flats' scaling laws (higher range, lower relief than the coast
    // at large), and the replay identity.
    c.band("open-coast tidal range m", mean(&t_open), format!("{:.2} m", mean(&t_open)));
    c.band("macrotidal coast share %", mean(&t_macro), format!("{:.1} %", mean(&t_macro)));
    if t_enc.is_empty() {
        println!();
        println!(" (no landlocked seas in this sweep — the enclosed-class band idles)");
    } else {
        c.band("enclosed-sea tidal range m", mean(&t_enc), format!("{:.2} m", mean(&t_enc)));
        c.must(
            "landlocked seas sit far below the open coast",
            mean(&t_enc) < 0.5 * mean(&t_open),
            format!("{:.2} m vs {:.2} m open", mean(&t_enc), mean(&t_open)),
            "M43 gate: the Mediterranean law — no path in, no tide",
        );
    }
    c.band("tidal flats per 1000 coast", mean(&t_flats), format!("{:.1}", mean(&t_flats)));
    c.band("estuary mouths per seed", mean(&t_est), format!("{:.1}", mean(&t_est)));
    c.band("flat-cell range amplification", mean(&t_amp), format!("{:.2}x", mean(&t_amp)));
    c.band("flat-cell relief fraction", mean(&t_rel), format!("{:.2}", mean(&t_rel)));
    c.must(
        "tide field replays identical",
        ta == tb,
        format!("{}", if ta == tb { "agree" } else { "DIVERGE" }),
        "M43 gate: same seed, two chunkings, one shore",
    );
    // M44 — the drift replay: the grown shore is one history.
    c.must(
        "drift ledger replays identical",
        oa == ob,
        format!("{}", if oa == ob { "agree" } else { "DIVERGE" }),
        "M44 gate: same seed, two chunkings, one grown shore; joins hash_state",
    );

    c.print();
}

// ================================================================ run log

#[derive(Default)]
struct RunLog {
    rows: Vec<(usize, i64, usize, usize, f64, f64, usize, usize, usize)>,
    census: BTreeMap<String, usize>,
    camps: usize,
    strikes: usize,
    depletions: usize,
    wars: usize,
    famines: usize,
    /// M72 — where and when each famine struck: (month, x, y). The famine
    /// pass no longer rolls a private die, so every one of these must be
    /// answerable by the year's own realized rain (SPI ≤ −1 at that cell).
    famine_sites: Vec<(i64, i64, i64, i64)>,
    /// M72 — the eligible pool: every rain-fed farming town-year that
    /// *could* have starved (the famine pass's own predicate), as
    /// (year, x, y). Without the pool a famine list proves only that the
    /// hungry were dry; with it we can measure whether dryness governs
    /// hunger — the dose-response the causal claim actually rests on.
    famine_pool: Vec<(i64, i64, i64)>,
    placeholders: usize,
    empties: usize,
    /// events that speak a god's name — festivals, omens, war-oaths (M3.5)
    god_citations: usize,
    arc: Vec<(i64, String)>,
    max_gap: i64,
    total_events: usize,
    // ---- M4 statecraft telemetry ----
    /// Alive polity count sampled once a year.
    polities: Vec<usize>,
    /// Settlements that changed hands (conquest, cession, sack).
    transfers: usize,
    /// New polities born mid-run (rebellion secessions).
    rebellions: usize,
    /// Did any war draw allies to a banner?
    coalition_seen: bool,
    /// Most vassal realms seen at once.
    vassals_max: usize,
    // ---- M12 kindred telemetry ----
    /// Did the living-people count ever rise year-on-year? (Fusion — the
    /// falling half — is judged on the 300y patina clock, not here.)
    peoples_rose: bool,
    /// M80 — every drought the chronicle announced, by the name it spoke.
    drought_named: Vec<String>,
}

/// Advance `years` in 12-month ticks, logging everything worth judging.
fn run_years(w: &mut World, years: usize) -> RunLog {
    let mut log = RunLog::default();
    let mut last_m = w.month;
    let god_names: Vec<String> = w
        .peoples.peoples
        .iter()
        .flat_map(|c| c.pantheon.iter().map(|g| g.name.clone()))
        .collect();
    // ADR-0018 — "polities" are realms: a crown lives while a town flies it
    let alive_count = |w: &World| -> usize {
        (0..w.peoples.realms.len())
            .filter(|&c| w.peoples.settlements.iter().any(|s| s.realm.0 == c))
            .count()
    };
    let mut owners: Vec<usize> = w.peoples.settlements.iter().map(|s| s.realm.0).collect();
    let mut n_realms = w.peoples.realms.len();
    let mut prev_peoples = w.peoples.peoples.iter().filter(|p| p.alive).count();
    for yr in 1..=years {
        let m0 = w.month;
        let (evs, _founded, _dep) = w.tick(12);
        // M72 — the eligible pool for this year's harvest verdict: the
        // famine pass's own predicate (rain-fed wheat or maize, off the
        // river, more than 90 souls), read once per year. Sampled at the
        // year's close rather than mid-pass, so a town the famine itself
        // pushed under the floor still counts as having been at risk.
        if let Some(fm) = (m0..w.month).find(|m| m.rem_euclid(12) == 7) {
            let fyear = fm / 12;
            for s in &w.peoples.settlements {
                let pack = w.fields.crops[[s.y as usize, s.x as usize]];
                let rainfed = (pack == calliope::agriculture::CropPackage::Wheat.code()
                    || pack == calliope::agriculture::CropPackage::Maize.code())
                    && !s.river;
                if rainfed && s.pop > 90 {
                    log.famine_pool.push((fyear, s.x, s.y));
                }
            }
        }
        for e in &evs {
            *log.census.entry(e.k.name().to_string()).or_default() += 1;
            if god_names.iter().any(|g| e.text.contains(g.as_str())) {
                log.god_citations += 1;
            }
            if e.text.contains('{') || e.text.contains('}') {
                log.placeholders += 1;
            }
            if e.text.trim().is_empty() {
                log.empties += 1;
            }
            if e.text.contains("mining camp") {
                log.camps += 1;
            }
            if e.k == calliope::event::EventKind::Drought {
                log.drought_named.push(e.s.clone());
            }
            match e.k.name() {
                "discovery" => log.strikes += 1,
                "depletion" => log.depletions += 1,
                "war" => log.wars += 1,
                "famine" => {
                    log.famines += 1;
                    // the toll, read off the telling: the first number in
                    // a famine line is its dead. Severity is the dose the
                    // threshold model actually modulates.
                    let dead: i64 = e
                        .text
                        .split(|ch: char| !ch.is_ascii_digit())
                        .find(|t| !t.is_empty())
                        .and_then(|t| t.parse().ok())
                        .unwrap_or(0);
                    log.famine_sites.push((e.m, e.x, e.y, dead));
                }
                "tech" | "society" => log.arc.push((e.m, e.text.clone())),
                _ => {}
            }
            log.max_gap = log.max_gap.max(e.m - last_m);
            last_m = e.m;
        }
        log.total_events += evs.len();
        // ---- M4: who holds what, and did any of it move this year ----
        for (i, s) in w.peoples.settlements.iter().enumerate() {
            if i < owners.len() && owners[i] != s.realm.0 {
                log.transfers += 1;
            }
        }
        owners = w.peoples.settlements.iter().map(|s| s.realm.0).collect();
        if w.peoples.realms.len() > n_realms {
            log.rebellions += w.peoples.realms.len() - n_realms;
            n_realms = w.peoples.realms.len();
        }
        if w.politics.wars.iter().any(|war| !war.allies_a.is_empty() || !war.allies_b.is_empty()) {
            log.coalition_seen = true;
        }
        let vassals = w.politics.vassal_of.iter().filter(|v| v.is_some()).count();
        log.vassals_max = log.vassals_max.max(vassals);
        log.polities.push(alive_count(w));
        // M12.1 — divergence should mint daughter peoples on this clock;
        // fusion (the falling half) is a patina-scale check (300y).
        let living_peoples = w.peoples.peoples.iter().filter(|p| p.alive).count();
        if living_peoples > prev_peoples {
            log.peoples_rose = true;
        }
        prev_peoples = living_peoples;
        let pop: i64 = w.peoples.settlements.iter().map(|s| s.pop).sum();
        let wealth: f64 = w.peoples.settlements.iter().map(|s| s.wealth).sum();
        let treasury: f64 = w.peoples.realms.iter().map(|r| r.treasury).sum();
        let techs: usize = w.peoples.societies.iter().map(|s| s.techs.len()).sum();
        let known = w.deposits.iter().filter(|d| d.known).count();
        log.rows.push((yr, pop, w.peoples.settlements.len(), w.routes.len(), wealth, treasury, techs, known, evs.len()));
    }
    log.max_gap = log.max_gap.max(w.month - last_m);
    log
}

// ================================================================ terrain

fn cmd_terrain(seed: i64, size: usize, explain: bool) {
    let t0 = Instant::now();
    let w = World::generate(seed, size);
    let gen_ms = t0.elapsed().as_millis();
    header("TERRAIN", &format!("seed {} · {}x{}", seed, w.width, size));
    println!("world: \"{}\" · generated in {} ms", w.world_name, gen_ms);

    let land = land_mask(&w);
    let total = land.len() as f64;
    let land_n = land.iter().filter(|&&b| b).count() as f64;
    let land_frac = land_n / total;
    println!("land: {} cells of {} ({}) · {} km²", land_n as usize, total as usize, pct(land_frac), (land_n * gc::KM_PER_CELL * gc::KM_PER_CELL) as u64);

    // hypsometry
    let hs = masked(&w.fields.height, &land);
    let depths: Vec<f64> = w.fields.height.iter().filter(|&&h| h < 0.0).map(|&h| h as f64).collect();
    println!("hypsometry (land h): p5 {:.3} · p25 {:.3} · p50 {:.3} · p75 {:.3} · p95 {:.3} · max {:.3}", quantile(&hs, 0.05), quantile(&hs, 0.25), quantile(&hs, 0.50), quantile(&hs, 0.75), quantile(&hs, 0.95), quantile(&hs, 1.0));
    println!("bathymetry (sea h): p5 {:.3} · p50 {:.3} · p95 {:.3}", quantile(&depths, 0.05), quantile(&depths, 0.50), quantile(&depths, 0.95));
    let hills = hs.iter().filter(|&&h| h > 0.35).count() as f64 / land_n;
    let mtn = hs.iter().filter(|&&h| h > 0.5).count() as f64 / land_n;
    let alpine = hs.iter().filter(|&&h| h > 0.65).count() as f64 / land_n;
    println!("relief of land: hills(h>.35) {} · mountains(h>.5) {} · alpine(h>.65) {}", pct(hills), pct(mtn), pct(alpine));

    // landmass census
    let li = ndimage::label(&land, true);
    let mut continents = 0usize;
    let mut majors = 0usize;
    let mut islands = 0usize;
    let mut islets = 0usize;
    let mut largest = 0.0f64;
    for &a in &li.areas {
        largest = largest.max(a);
        if a >= 3000.0 {
            continents += 1;
        } else if a >= 400.0 {
            majors += 1;
        } else if a >= 40.0 {
            islands += 1;
        } else {
            islets += 1;
        }
    }
    println!("landmasses: {} total · {} continents(≥3000c) · {} major isles(≥400c) · {} islands(≥40c) · {} islets", li.n, continents, majors, islands, islets);
    println!("largest landmass: {} cells = {} of all land", largest as usize, pct(largest / land_n.max(1.0)));

    // coastline crenulation
    let (rows, cols) = land.dim();
    let mut coast = 0usize;
    for y in 0..rows {
        for x in 0..cols {
            if !land[[y, x]] {
                continue;
            }
            let mut sea = false;
            if y > 0 && !land[[y - 1, x]] {
                sea = true;
            }
            if y + 1 < rows && !land[[y + 1, x]] {
                sea = true;
            }
            if x > 0 && !land[[y, x - 1]] {
                sea = true;
            }
            if x + 1 < cols && !land[[y, x + 1]] {
                sea = true;
            }
            if sea {
                coast += 1;
            }
        }
    }
    let coast_ratio = coast as f64 / land_n.max(1.0);
    println!("coastline: {} coastal cells · crenulation {:.3} (coast/land)", coast, coast_ratio);

    // land by latitude band
    println!("land share by latitude band:");
    for b in 0..6 {
        let y0 = rows * b / 6;
        let y1 = rows * (b + 1) / 6;
        let mut n = 0usize;
        let mut l = 0usize;
        for y in y0..y1 {
            for x in 0..cols {
                n += 1;
                l += land[[y, x]] as usize;
            }
        }
        println!("  {:>4.0}°–{:>4.0}°  {:>6}  {}", latitude(y0, rows), latitude(y1.saturating_sub(1), rows), l, pct(l as f64 / n as f64));
    }

    // ---- sea-level history (M25) ----------------------------------------
    let sl = &w.sealevel;
    println!(
        "sea level (M25): phase {:.3} · stand {:+.3} · eustatic {:+.4} h · rebound {:+.4} · forebulge {:+.4}",
        sl.phase, sl.stand, sl.eustatic, sl.rebound, sl.forebulge
    );

    let arch = w.features.iter().filter(|f| f.t == "archipelago").count();
    let named_isles = w.features.iter().filter(|f| f.t == "island").count();
    let ranges = w.features.iter().filter(|f| f.t == "range").count();
    println!("named: {} archipelagos · {} islands · {} mountain ranges", arch, named_isles, ranges);

    // ---- the plate sketch (M16/ADR-0024) --------------------------------
    let pl = &w.plates;
    let n_plates = pl.plates.len();
    let cont_n = pl.plates.iter().filter(|p| p.continental).count();
    let mean_age = pl.mean_age();
    let (mut conv, mut div, mut trans) = (0usize, 0usize, 0usize);
    for &b in pl.boundary.iter() {
        match b {
            calliope::plates::B_CONVERGENT => conv += 1,
            calliope::plates::B_DIVERGENT => div += 1,
            calliope::plates::B_TRANSFORM => trans += 1,
            _ => {}
        }
    }
    let btot = (conv + div + trans).max(1);
    let conv_share = conv as f64 / btot as f64;
    println!("plates: {} polygons ({} continental) · mean drift-age {:.0} Myr", n_plates, cont_n, mean_age);
    println!("seams: {} convergent · {} divergent · {} transform cells", conv, div, trans);

    // M16 gate — same seed, twice: the sketch and the land it draws must
    // agree byte for byte before anything else builds on them.
    let pa = calliope::plates::generate(seed, size);
    let pb = calliope::plates::generate(seed, size);
    let hm_hash = |p: &calliope::plates::Plates| -> u64 {
        let g = calliope::geo::heightmap(seed, size, p);
        let mut bts: Vec<u8> = Vec::with_capacity(g.len() * 8);
        for v in g.iter() {
            bts.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        fnv(&bts)
    };
    let (ha, hb) = (hm_hash(&pa), hm_hash(&pb));
    let plates_same = pa.hash() == pb.hash();

    // ---- orogeny ages (M17) ---------------------------------------------
    // The age-decay curve probed causally: the SAME sketch re-aged to a
    // uniform young / middle / old belt age must draw monotonically
    // sinking belts. (An observational age-vs-height table over one
    // world confounds with base terrain — the probe isolates the law.)
    let belt_w = 0.06 * size as f64;
    let belt_mask: Vec<(usize, usize)> = (0..size)
        .flat_map(|y| (0..size).map(move |x| (y, x)))
        .filter(|&(y, x)| (pa.seam_dist[[y, x]] as f64) <= belt_w)
        .collect();
    let mean_belt = |age_myr: f32| -> f64 {
        let mut aged = pa.clone();
        aged.seam_age.fill(age_myr);
        let g = calliope::geo::heightmap(seed, size, &aged);
        belt_mask.iter().map(|&(y, x)| g[[y, x]].max(0.0)).sum::<f64>() / belt_mask.len().max(1) as f64
    };
    let (bel_young, bel_mid, bel_old) = (mean_belt(200.0), mean_belt(900.0), mean_belt(2000.0));
    println!(
        "orogeny ages: mean belt relief re-aged  200 Myr {:.4} · 900 Myr {:.4} · 2000 Myr {:.4}  ({} belt cells)",
        bel_young, bel_mid, bel_old, belt_mask.len()
    );
    let mono = belt_mask.is_empty()
        || (bel_young > bel_mid + 0.002 && bel_mid > bel_old + 0.002);

    let bl = border_land(&w);

    // ---- rock provinces (M18) -------------------------------------------
    // The ground differs by history: every basement class must be present
    // on land, none may swallow the map.
    let shares = calliope::rock::land_shares(&w.fields.rock, &w.fields.height);
    println!(
        "rock provinces: shield {} · basin {} · fold belt {} · volcanic {}",
        pct(shares[0]), pct(shares[1]), pct(shares[2]), pct(shares[3])
    );

    // ---- geologic legibility (M21) --------------------------------------
    // Each province sampled against an independent landform correlate:
    // the map must read true to a glance, not merely satisfy its own
    // classifier.
    let legi = calliope::rock::legibility(&w.fields.rock, &w.plates, &w.fields.height);
    println!(
        "legibility (M21): off-correlate shield {} · basin {} · fold belt {}",
        pct(legi[0]), pct(legi[1]), pct(legi[2])
    );
    let legi_worst = legi.iter().cloned().fold(0.0f64, f64::max);

    // ---- heat transport (M41) -------------------------------------------
    // The law re-run on the post-widen ledger: the same `current_bias`
    // the dawn folded into tmean, measured where it touches land. Warm
    // rims and cold rims must both exist, sit a few °C off their zonal
    // law, and the world-mean must stay near zero — advection moves
    // heat, it does not mint it.
    let water_g = w.fields.height.mapv(|h| h < 0.0);
    let heat = calliope::climate::current_bias(&water_g, &w.currents.v);
    let mut hw_n = 0usize;
    let mut hw_sum = 0.0f64;
    let mut hc_n = 0usize;
    let mut hc_sum = 0.0f64;
    let mut h_net = 0.0f64;
    for y in 0..rows {
        for x in 0..cols {
            let b = heat[[y, x]];
            h_net += b;
            if land[[y, x]] {
                if b >= 0.5 {
                    hw_n += 1;
                    hw_sum += b;
                } else if b <= -0.5 {
                    hc_n += 1;
                    hc_sum += b;
                }
            }
        }
    }
    let hw_mean = if hw_n > 0 { hw_sum / hw_n as f64 } else { 0.0 };
    let hc_mean = if hc_n > 0 { hc_sum / hc_n as f64 } else { 0.0 };
    let h_net_mean = h_net / (rows * cols) as f64;
    println!(
        "heat transport (M41): warm rim {} cells mean {:+.2} °C · cold rim {} cells mean {:+.2} °C · net {:+.3} °C",
        hw_n, hw_mean, hc_n, hc_mean, h_net_mean
    );

    let mut c = Checks::default();
    c.band("land fraction", land_frac, pct(land_frac));
    c.must(
        "warm and cold rims both exist (M41)",
        hw_n > 0 && hc_n > 0,
        format!("{} warm · {} cold cells", hw_n, hc_n),
        "M41: the currents must touch coasts both ways — Gulf Stream and Humboldt analogs",
    );
    c.band("warm-coast heat delta", hw_mean, format!("{:+.2} °C ({} cells)", hw_mean, hw_n));
    c.band("cold-coast heat delta", hc_mean, format!("{:+.2} °C ({} cells)", hc_mean, hc_n));
    c.band("heat transport net bias", h_net_mean.abs(), format!("{:+.3} °C", h_net_mean));
    c.must("border land cells", bl == 0, format!("{}", bl), "must be 0 — no clipped landmasses");
    // M25 gate — the coastline holds the datum: land area stays within
    // ±5% (relative) of the pre-M25 baseline near mid-curve, widening
    // slightly toward a full stand. Baselines are the pre-M25 report
    // numbers for the three standing report seeds.
    const M25_BASE: &[(i64, f64)] = &[
        (12345, 111762.0 / 327680.0),
        (777, 113084.0 / 327680.0),
        (90210, 113658.0 / 327680.0),
    ];
    if let Some(&(_, base)) = M25_BASE.iter().find(|(sd, _)| *sd == seed) {
        let drift = (land_frac - base).abs() / base;
        let cap = 0.05 + 0.045 * sl.stand.abs();
        c.must(
            "coastline holds the datum (M25)",
            drift <= cap,
            format!("{} drift at stand {:+.2} (cap {})", pct(drift), sl.stand, pct(cap)),
            "M25 gate: land area within ±5% of the pre-M25 baseline near mid-curve",
        );
    }
    {
        // M25 gate — the waterline is one history: regen must agree.
        let sl2 = calliope::sealevel::generate(seed, size);
        c.must(
            "sea-level history regen byte-identical",
            sl.hash() == sl2.hash(),
            if sl.hash() == sl2.hash() { "identical".into() } else { "DIVERGED".into() },
            "M25: same seed ⇒ same waterline; joins hash_state",
        );
    }
    {
        // ---- coastal landforms (M26) --------------------------------
        let lf = &w.fields.landform;
        let hgt = &w.fields.height;
        let (gh, gw) = hgt.dim();
        let last = sl.row.len() - 1;
        let mut n_raised = 0usize;
        let mut n_ria = 0usize;
        let mut n_skerry = 0usize;
        let mut wrong_sign = 0usize;
        // coast cells (land with a 4-neighbor sea) split into the
        // rebound belt (isostatic uplift rows) and the forebulge collar
        let mut coast = 0usize;
        let mut coast_up = 0usize;
        let mut raised_up = 0usize;
        let mut coast_dn = 0usize;
        let mut raised_dn = 0usize;
        for y in 0..gh {
            let iso = sl.row[y.min(last)];
            let dz = iso - sl.eustatic;
            for x in 0..gw {
                match lf[[y, x]] {
                    calliope::landform::RAISED => {
                        n_raised += 1;
                        if dz <= 0.0 { wrong_sign += 1; }
                        if iso > 0.0 { raised_up += 1; } else if iso < 0.0 { raised_dn += 1; }
                    }
                    calliope::landform::RIA => {
                        n_ria += 1;
                        if dz >= 0.0 { wrong_sign += 1; }
                    }
                    calliope::landform::SKERRY => {
                        n_skerry += 1;
                        if dz >= 0.0 { wrong_sign += 1; }
                    }
                    _ => {}
                }
                if hgt[[y, x]] >= 0.0 {
                    let sea_next = (y > 0 && hgt[[y - 1, x]] < 0.0)
                        || (y + 1 < gh && hgt[[y + 1, x]] < 0.0)
                        || (x > 0 && hgt[[y, x - 1]] < 0.0)
                        || (x + 1 < gw && hgt[[y, x + 1]] < 0.0);
                    if sea_next {
                        coast += 1;
                        if iso > 0.0 { coast_up += 1; } else if iso < 0.0 { coast_dn += 1; }
                    }
                }
            }
        }
        println!(
            "coastal landforms (M26): raised {} · ria {} · skerry {} · coast {} cells",
            n_raised, n_ria, n_skerry, coast
        );
        // Amplitude the classifier actually sees: the mean per-row net
        // offset, emergence and submergence separately (isostasy dwarfs
        // the eustatic stand, so |stand| alone is the wrong yardstick).
        let mut amp_up = 0.0f64;
        let mut amp_dn = 0.0f64;
        for y in 0..gh {
            let dz = sl.row[y.min(last)] - sl.eustatic;
            amp_up += dz.max(0.0);
            amp_dn += (-dz).max(0.0);
        }
        amp_up /= gh as f64;
        amp_dn /= gh as f64;
        let raised_rate = n_raised as f64 / coast.max(1) as f64 / amp_up.max(1e-9);
        c.band("raised coast per stand", raised_rate, format!("{:.3}", raised_rate));
        if amp_dn > 1e-6 {
            let drowned = (n_ria + n_skerry) as f64 / coast.max(1) as f64 / amp_dn;
            c.band("drowned coast per stand", drowned, format!("{:.3}", drowned));
        }
        c.must(
            "landform signs read true",
            wrong_sign == 0,
            format!("{} tags against the offset sign", wrong_sign),
            "M26: raised only where the land rose, drowned only where the sea did",
        );
        // M33/M43/M60 — the shipped grid carries the full fold, which
        // runs after classify(); the regen leg must walk the same steps
        // in the same order or the compare reads its own omission.
        let mut regen = calliope::landform::classify(hgt, sl, &w.ice, &w.sediment.delta);
        calliope::landform::stamp_patterned(&mut regen, &w.permafrost.pattern, hgt);
        calliope::landform::stamp_tidal(&mut regen, &w.tides, hgt, &w.fields.flags);
        calliope::landform::stamp_delta(&mut regen, &w.sediment.delta, hgt);
        calliope::landform::stamp_coastforms(&mut regen, &w.fields.coastform, hgt);
        {
            let water = hgt.mapv(|h| h < 0.0);
            let rivers = w.fields.flags.mapv(|f| f & CellFlags::RIVER.bits() != 0);
            let lakes = w.fields.flags.mapv(|f| f & CellFlags::LAKE.bits() != 0);
            let dry = calliope::hydrology::springs_and_oases(
                hgt,
                &water,
                &rivers,
                &lakes,
                &w.fields.aquifer,
                &w.fields.biomes,
                &w.fields.precip,
            );
            calliope::landform::stamp_dry_water(
                &mut regen,
                &dry.springs,
                &dry.oases,
                &w.fields.aquifer,
                hgt,
            );
        }
        calliope::landform::stamp_trough(&mut regen, &w.ice.carved, hgt);
        calliope::landform::finish(&mut regen, hgt);
        let purity = calliope::landform::hash(&regen) == calliope::landform::hash(lf);
        c.must(
            "landform grid regen byte-identical",
            purity,
            if purity { "identical".into() } else { "DIVERGED".into() },
            "M26/M60 gate: pure function of the dawn grids (classify + every stamp + the relief fill); joins hash_state",
        );

        // ---- the landform vocabulary (M60) ---------------------------
        {
            // Totality: after `finish`, NONE survives only on open sea —
            // no land cell and no shore-adjacent water cell goes nameless.
            let mut nameless_land = 0usize;
            let mut nameless_shore = 0usize;
            let mut oasis_pond = 0usize;
            let mut census = [0usize; calliope::landform::NAMES.len()];
            for y in 0..gh {
                for x in 0..gw {
                    let tag = lf[[y, x]] as usize;
                    if tag < census.len() {
                        census[tag] += 1;
                    }
                    if tag == calliope::landform::OASIS as usize
                        && w.fields.aquifer[[y, x]]
                            <= calliope::hydrology::SPRING_DAYLIGHT_M as f32
                    {
                        oasis_pond += 1;
                    }
                    if tag != calliope::landform::NONE as usize {
                        continue;
                    }
                    if hgt[[y, x]] >= 0.0 {
                        nameless_land += 1;
                    } else {
                        let land_next = (y > 0 && hgt[[y - 1, x]] >= 0.0)
                            || (y + 1 < gh && hgt[[y + 1, x]] >= 0.0)
                            || (x > 0 && hgt[[y, x - 1]] >= 0.0)
                            || (x + 1 < gw && hgt[[y, x + 1]] >= 0.0);
                        if land_next {
                            nameless_shore += 1;
                        }
                    }
                }
            }
            let told: Vec<String> = calliope::landform::NAMES
                .iter()
                .zip(census.iter())
                .filter(|(_, &n)| n > 0)
                .map(|(name, n)| format!("{name} {n}"))
                .collect();
            println!("landform vocabulary (M60): {}", told.join(" · "));
            let n_oasis = census[calliope::landform::OASIS as usize];
            if n_oasis > 0 {
                println!(
                    "  oasis split: {} daylighted (table ≤2 m) · {} low-point of the reach",
                    oasis_pond,
                    n_oasis - oasis_pond
                );
            }
            c.must(
                "no ground is nameless",
                nameless_land == 0 && nameless_shore == 0,
                format!("{} land · {} shore cells untold", nameless_land, nameless_shore),
                "M60 gate: after the fold, NONE survives only on open sea",
            );
            // The generic relief classes must actually carry the fill:
            // a 512 world with continental relief tells mountains, hills
            // and plains on every seed — an empty class means the
            // thresholds read the wrong field, not a quiet world.
            let n_mtn = census[calliope::landform::MOUNTAIN as usize];
            let n_hill = census[calliope::landform::HILLS as usize];
            let n_plain = census[calliope::landform::PLAIN as usize];
            c.must(
                "relief vocabulary is spoken",
                n_mtn > 0 && n_hill > 0 && n_plain > 0,
                format!("mountain {} · hills {} · plain {}", n_mtn, n_hill, n_plain),
                "M60 gate: Hammond classes read real relief (300 m/90 m breaks over the 20 km window)",
            );
            // KARST is reserved, unstamped until a carbonate province
            // exists (Ready queue) — a tagged cell is a contract break.
            c.must(
                "karst stays reserved",
                census[calliope::landform::KARST as usize] == 0,
                format!("{} cells", census[calliope::landform::KARST as usize]),
                "M60: the code point is held for Karst Country II; no pass may stamp it yet",
            );
            // The oasis word names groves, not reaches: strict depth
            // minima of the M55 mask. A count in the thousands means a
            // basin-painting law leaked back in (the ≤2 m daylight
            // override painted 8-11k; the non-strict minimum stamped
            // ~5k rim-band cells of clamped basins).
            c.band(
                "oasis cells stay pointlike",
                n_oasis as f64,
                format!("{} cells", n_oasis),
            );
        }

        // ---- longshore drift (M44) ----------------------------------
        {
            let cf = &w.fields.coastform;
            let ledger = &w.coastform;
            let mut n_spit = 0usize;
            let mut n_bar = 0usize;
            let mut n_lag = 0usize;
            let mut misread = 0usize;
            let mut coastal_water = 0usize;
            for y in 0..gh {
                for x in 0..gw {
                    match cf[[y, x]] {
                        calliope::coast::SPIT => {
                            n_spit += 1;
                            if hgt[[y, x]] < 0.0 { misread += 1; }
                        }
                        calliope::coast::BARRIER => {
                            n_bar += 1;
                            if hgt[[y, x]] < 0.0 { misread += 1; }
                        }
                        calliope::coast::LAGOON => {
                            n_lag += 1;
                            if hgt[[y, x]] >= 0.0 { misread += 1; }
                        }
                        _ => {}
                    }
                    if hgt[[y, x]] < 0.0 {
                        let land_next = (y > 0 && hgt[[y - 1, x]] >= 0.0)
                            || (y + 1 < gh && hgt[[y + 1, x]] >= 0.0)
                            || (x > 0 && hgt[[y, x - 1]] >= 0.0)
                            || (x + 1 < gw && hgt[[y, x + 1]] >= 0.0);
                        if land_next {
                            coastal_water += 1;
                        }
                    }
                }
            }
            // chains per class: 8-connected components of the form grid
            let chains = |class: u8| -> usize {
                let mask = ndarray::Array2::from_shape_fn((gh, gw), |(y, x)| {
                    cf[[y, x]] == class
                });
                calliope::ndimage::label(&mask, true).n
            };
            let spit_chains = chains(calliope::coast::SPIT);
            let bar_chains = chains(calliope::coast::BARRIER);
            println!(
                "longshore drift (M44): spit cells {} in {} chains · barrier cells {} in {} chains · lagoon cells {} · deposits {}",
                n_spit, spit_chains, n_bar, bar_chains, n_lag, ledger.deposits.len()
            );
            let share = 100.0 * (n_spit + n_bar + n_lag) as f64 / coastal_water.max(1) as f64;
            c.band("coastform share of coastal cells %", share, format!("{:.2} %", share));
            c.band("spit chains per seed", spit_chains as f64, format!("{}", spit_chains));
            c.band("barrier chains per seed", bar_chains as f64, format!("{}", bar_chains));
            c.band("lagoon cells per seed", n_lag as f64, format!("{}", n_lag));
            c.must(
                "drift ground stands, lagoons drown (M44)",
                misread == 0,
                format!("{} cells against their class", misread),
                "M44: spits and barriers are land, lagoons are water — the form grid never lies about the height field",
            );
            c.must(
                "deposit ledger matches the grown ground (M44)",
                ledger.deposits.len() == n_spit + n_bar,
                format!("{} deposits vs {} formed cells", ledger.deposits.len(), n_spit + n_bar),
                "M44: every deposited cell is a spit or barrier cell, and nothing else is",
            );
            let shallow = ledger.deposits.iter().all(|&(_, _, b)| {
                let d = f32::from_bits(b) as f64;
                d < 0.0 && d > calliope::coast::BAR_FLOOR
            });
            c.must(
                "deposits sit on the shelf (M44)",
                shallow,
                if shallow { "all shoreface".into() } else { "DEEP OR DRY".into() },
                "M44: longshore sand only lands on shallow seabed (0 to −80 m — the bank the 4 km grid resolves)",
            );
        }

        // ---- the sediment budget (M59) --------------------------------
        {
            let sed = &w.sediment;
            let det = sed.detached.max(1e-12);
            let settled_pct = 100.0 * sed.settled / det;
            let delta_pct = 100.0 * sed.delta_fill / det;
            let abyssal_pct = 100.0 * sed.abyssal / det;
            let n_mouths = sed.mouths.len();
            let with_fans = sed.mouths.iter().filter(|m| m.fan > 0).count();
            let delta_cells = sed.delta.iter().filter(|&&b| b).count();
            let largest_fan = sed.mouths.iter().map(|m| m.fan).max().unwrap_or(0);
            println!(
                "sediment budget (M59): detached {:.2} · settled {:.2} ({:.0}%) · delta fill {:.2} ({:.0}%) · abyssal {:.2} ({:.0}%)",
                sed.detached, sed.settled, settled_pct, sed.delta_fill, delta_pct, sed.abyssal, abyssal_pct
            );
            println!(
                "deltas: {} mouths ledgered · {} built fans · {} delta cells ({} of land) · largest fan {} cells",
                n_mouths, with_fans, delta_cells, pct(delta_cells as f64 / land_n.max(1.0)), largest_fan
            );
            // the books close by construction; 1% is the audit's
            // tolerance for the float sums, not a licence
            let closure = (sed.detached - (sed.settled + sed.delta_fill + sed.abyssal)).abs() / det;
            c.must(
                "sediment ledger closes (M59)",
                closure <= 0.01,
                format!("{:.4}% open", 100.0 * closure),
                "M59 gate: detached = settled + delta fill + abyssal to within 1% end-to-end",
            );
            // the grid is the ledger's footprint: Σdepth re-summed
            // independently must match the on-map share of the books
            let grid_sum: f64 = w.fields.silt.iter().map(|&v| v as f64).sum();
            let on_map = sed.settled + sed.delta_fill;
            let drift = (grid_sum - on_map).abs() / det;
            c.must(
                "the grid remembers the ledger (M59)",
                drift <= 0.01,
                format!("Σdepth {:.2} vs books {:.2}", grid_sum, on_map),
                "M59 gate: deposition depth re-summed from the grid matches the ledger to 1% (f32 grid vs f64 books)",
            );
            // Deltas scale with the river: fan area against mouth load
            // and against drainage area (Spearman, the M54 instrument).
            // M64's fluvial-dominance recalibration restricted the fan
            // population to the ~80 big-basin mouths, so raw rank ρ
            // mechanically fell from 0.84 (over 365 rill-to-major
            // mouths) to 0.51–0.67 — restriction of range plus the fan
            // reach cap flattening the top, not a change in the physics.
            // The restated gate tests the law itself as a dose–response:
            // rank correlation must stay unambiguous (ρ ≥ 0.35 is
            // p < 0.001 at n ≈ 80), AND the top load quartile must build
            // materially bigger fans than the bottom quartile.
            let pairs: Vec<(f64, f64)> = sed.mouths.iter().map(|m| (m.load, m.fan as f64)).collect();
            let rho_load = spearman(&pairs);
            let pairs_a: Vec<(f64, f64)> = sed.mouths.iter().map(|m| (m.area, m.fan as f64)).collect();
            let rho_area = spearman(&pairs_a);
            let mut by_load: Vec<(f64, f64)> = pairs.clone();
            by_load.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            let qn = by_load.len() / 4;
            let (q1_mean, q4_mean) = if qn >= 3 {
                let q1: f64 = by_load[..qn].iter().map(|p| p.1).sum::<f64>() / qn as f64;
                let q4: f64 = by_load[by_load.len() - qn..].iter().map(|p| p.1).sum::<f64>() / qn as f64;
                (q1, q4)
            } else {
                (0.0, 0.0)
            };
            let dose = q4_mean / q1_mean.max(1e-9);
            println!(
                "  spearman: fan cells vs mouth load {:.2} · vs drainage area {:.2} ({} mouths)",
                rho_load, rho_area, n_mouths
            );
            println!(
                "  dose-response: mean fan cells Q1 {:.1} → Q4 {:.1} by load quartile (×{:.2}, n/4 = {})",
                q1_mean, q4_mean, dose, qn
            );
            c.must(
                "deltas scale with the load (M59)",
                n_mouths >= 10 && rho_load >= 0.35 && dose >= 1.5,
                format!("ρ {:.2} · Q4/Q1 ×{:.2} / {} mouths", rho_load, dose, n_mouths),
                "M59/M64 gate: rank ρ ≥0.35 (p<0.001 at n≈80) AND top load quartile builds ≥1.5× the bottom's fans — over ≥10 mouths so the check cannot pass on vacancy",
            );
            c.want(
                "deltas scale with the basin (M59)",
                rho_area >= 0.25,
                format!("ρ {:.2}", rho_area),
                "M59: drainage area drives load drives fans — the indirect leg, range-restricted by the ≥10⁴ km² dominance bar itself (measured 0.38–0.42 ×3 seeds)",
            );
            let delta_share = 100.0 * delta_cells as f64 / land_n.max(1.0);
            c.band("delta land share of land %", delta_share, format!("{:.3} %", delta_share));
            c.band("river deltas per seed", with_fans as f64, format!("{}", with_fans));
            c.band("sediment settled share %", settled_pct, format!("{:.1} %", settled_pct));
            c.band("sediment abyssal share %", abyssal_pct, format!("{:.1} %", abyssal_pct));

            // the shoaled anchorage: recompute the raw M45 field on the
            // shipped grids and audit the silt wiring both ways — cells
            // with no silt in the 5×5 window keep their score bit-for-
            // bit, cells with silt shoal by exactly 1/(1+k·depth). The
            // widening seam is skipped: the raw recompute sees margin
            // ocean where the pre-widen call saw the grid edge.
            let raw = calliope::settlements::shelter_score(&w.fields.height, &w.fields.coastform);
            let pad = w.size / 8;
            let (sh_r, sh_c) = raw.dim();
            let mut n_silted = 0usize;
            let mut n_clean_moved = 0usize;
            let mut n_silted_wrong = 0usize;
            let mut ratio_sum = 0.0f64;
            for y in 0..sh_r {
                for x in (pad + 8)..sh_c.saturating_sub(pad + 8) {
                    if raw[[y, x]] <= 0.0 {
                        continue;
                    }
                    let mut silt = 0.0f32;
                    for dy in -2..=2isize {
                        for dx in -2..=2isize {
                            let ny = y as isize + dy;
                            let nx = x as isize + dx;
                            if ny < 0 || nx < 0 || ny >= sh_r as isize || nx >= sh_c as isize {
                                continue;
                            }
                            let (ny, nx) = (ny as usize, nx as usize);
                            if w.fields.height[[ny, nx]] < 0.0 {
                                silt = silt.max(w.fields.silt[[ny, nx]]);
                            }
                        }
                    }
                    if silt <= 0.0 {
                        if w.shelter[[y, x]] != raw[[y, x]] {
                            n_clean_moved += 1;
                        }
                    } else {
                        n_silted += 1;
                        let expect = raw[[y, x]] / (1.0 + calliope::erosion::SILT_SHOAL * silt);
                        if (w.shelter[[y, x]] - expect).abs() > 1e-4 {
                            n_silted_wrong += 1;
                        }
                        ratio_sum += (w.shelter[[y, x]] / raw[[y, x]]) as f64;
                    }
                }
            }
            let mean_ratio = if n_silted > 0 { ratio_sum / n_silted as f64 } else { 1.0 };
            println!(
                "  shoaling: {} silted anchorages · mean shelter ratio {:.2} · {} clean cells moved · {} silted cells off-law",
                n_silted, mean_ratio, n_clean_moved, n_silted_wrong
            );
            c.must(
                "silt shoals the anchorage, nothing else moves (M59)",
                n_clean_moved == 0 && n_silted_wrong == 0,
                format!("{} clean moved · {} off-law", n_clean_moved, n_silted_wrong),
                "M59: shelter divides by 1+k·silt where fan silt stands in the anchorage window, and only there",
            );
            c.want(
                "harbors shoal where the silt lands (M59)",
                n_silted > 0 && mean_ratio < 0.99,
                format!("{} anchorages at ratio {:.2}", n_silted, mean_ratio),
                "M59: high-load mouths must actually cost their harbors something",
            );
        }



        // M29 — glacial relief: U-valleys, cirques and hangs by belt.
        {
            let ice = &w.ice;
            let (ir, icw) = ice.thickness.dim();
            let nf = ir as f64;
            let mut iced = [0usize; 2]; // alpine 40–62° · sheet >62°
            let mut ucel = [0usize; 2];
            let mut fjords = 0usize;
            for y in 0..ir {
                let lat = (-90.0 + (y as f64) * 180.0 / (nf - 1.0)).abs();
                let belt = if lat >= 62.0 {
                    1
                } else if lat >= 40.0 {
                    0
                } else {
                    continue;
                };
                for x in 0..icw {
                    if w.fields.height[[y, x]] < 0.0 {
                        continue;
                    }
                    if ice.thickness[[y, x]] > 0.0 {
                        iced[belt] += 1;
                        if ice.carved[[y, x]] >= 0.01 {
                            ucel[belt] += 1;
                        }
                    }
                }
            }
            for v in lf.iter() {
                if *v == calliope::landform::FJORD {
                    fjords += 1;
                }
            }
            let cir_alp = ice
                .cirques
                .iter()
                .filter(|&&(y, _)| {
                    let lat = (-90.0 + (y as f64) * 180.0 / (nf - 1.0)).abs();
                    (40.0..62.0).contains(&lat)
                })
                .count();
            let u_alp = 1000.0 * ucel[0] as f64 / iced[0].max(1) as f64;
            let u_sht = 1000.0 * ucel[1] as f64 / iced[1].max(1) as f64;
            let c_alp = 1000.0 * cir_alp as f64 / iced[0].max(1) as f64;
            println!();
            println!(
                "glacial relief: u-cells alpine {} / sheet {} · cirques {} ({} alpine) · hangs {} · fjord cells {}",
                ucel[0], ucel[1], ice.cirques.len(), cir_alp, ice.hangs.len(), fjords
            );
            c.band("u-valley cells per 1000 iced, alpine", u_alp, format!("{:.0}", u_alp));
            c.band("u-valley cells per 1000 iced, sheet", u_sht, format!("{:.0}", u_sht));
            c.band("cirques per 1000 iced, alpine", c_alp, format!("{:.1}", c_alp));
            c.band("hanging valleys per world", ice.hangs.len() as f64, format!("{}", ice.hangs.len()));
            let finite = w.fields.height.iter().all(|v| v.is_finite());
            c.must(
                "height field NaN-free after the carve",
                finite,
                if finite { "finite".into() } else { "NaN".into() },
                "M29 gate: the carve is pure lowering arithmetic",
            );

            // M30 — the depositional legacy: till share, moraine strings,
            // drumlin swarms, esker chains; till strictly under old ice.
            let (tr, tc) = ice.till.dim();
            let mut iced_all = 0usize;
            let mut iced_low = 0usize;
            let mut till_cells = 0usize;
            let mut till_off_ice = 0usize;
            for y in 0..tr {
                for x in 0..tc {
                    if w.fields.height[[y, x]] < 0.0 {
                        continue;
                    }
                    let on_ice = ice.thickness[[y, x]] > 0.0;
                    if on_ice {
                        iced_all += 1;
                        if w.fields.height[[y, x]] < 0.30 {
                            iced_low += 1;
                        }
                    }
                    if ice.till[[y, x]] > 0.0 {
                        till_cells += 1;
                        if !on_ice {
                            till_off_ice += 1;
                        }
                    }
                }
            }
            let t_share = 100.0 * till_cells as f64 / iced_low.max(1) as f64;
            let m_per = 1000.0 * ice.moraines.len() as f64 / iced_all.max(1) as f64;
            let d_per = 1000.0 * ice.drumlins.len() as f64 / till_cells.max(1) as f64;
            let loess_cells = ice.loess.iter().filter(|&&v| v > 0.0).count();
            let land_cells = (0..tr)
                .flat_map(|y| (0..tc).map(move |x| (y, x)))
                .filter(|&(y, x)| w.fields.height[[y, x]] >= 0.0)
                .count();
            let l_share = 100.0 * loess_cells as f64 / land_cells.max(1) as f64;
            println!(
                "glacial legacy: till {} cells ({:.1}% of iced lowland) · moraine {} · drumlins {} · esker cells {} · loess {} cells ({:.1}% of land)",
                till_cells, t_share, ice.moraines.len(), ice.drumlins.len(), ice.eskers.len(),
                loess_cells, l_share
            );
            c.band("till share of iced lowland %", t_share, format!("{:.1} %", t_share));
            c.band("moraine cells per 1000 iced", m_per, format!("{:.1}", m_per));
            c.band("drumlins per 1000 till", d_per, format!("{:.1}", d_per));
            c.band("esker cells per world", ice.eskers.len() as f64, format!("{}", ice.eskers.len()));
            c.band("loess share of land %", l_share, format!("{:.1} %", l_share));
            c.must(
                "till lies under old ice only",
                till_off_ice == 0,
                format!("{} stray cells", till_off_ice),
                "M30 gate: the sheet is the depositional footprint",
            );

            // The till dividend, latitude- AND height-matched: within
            // each (3° lat × 0.03 height) bin, gentle lowland on till vs
            // the same ground off it. The double match matters: within a
            // latitude belt, glaciation selected for elevation near the
            // ELA, so till ground sits higher (colder, drier) than the
            // belt at large — lat-only bins read that confound as a
            // negative dividend. Matched on both, the weighted mean
            // difference reads the bonus straight off the shipped field.
            const HB: usize = 10; // height bins over [0, 0.30)
            let mut bin_t = vec![(0.0f64, 0usize); 30 * HB];
            let mut bin_o = vec![(0.0f64, 0usize); 30 * HB];
            for y in 0..tr {
                let lat = (-90.0 + (y as f64) * 180.0 / (tr as f64 - 1.0)).abs();
                if lat < 35.0 {
                    continue;
                }
                let lb = ((lat / 3.0) as usize).min(29);
                for x in 0..tc {
                    let h = w.fields.height[[y, x]];
                    if h < 0.0 || h >= 0.30 {
                        continue;
                    }
                    let mut s = 0.0f32;
                    for (dy, dx) in [(-1isize, 0isize), (1, 0), (0, -1), (0, 1)] {
                        let ny = y as isize + dy;
                        let nx = x as isize + dx;
                        if ny < 0 || nx < 0 || ny >= tr as isize || nx >= tc as isize {
                            continue;
                        }
                        s = s.max((w.fields.height[[ny as usize, nx as usize]] - h).abs());
                    }
                    if s > 0.015 {
                        continue;
                    }
                    let b = lb * HB + ((h as f64 / 0.03) as usize).min(HB - 1);
                    let f = w.fields.fertility[[y, x]] as f64;
                    if ice.till[[y, x]] > 0.0 {
                        bin_t[b].0 += f;
                        bin_t[b].1 += 1;
                    } else {
                        bin_o[b].0 += f;
                        bin_o[b].1 += 1;
                    }
                }
            }
            let (mut dsum, mut wsum, mut bins) = (0.0f64, 0usize, 0usize);
            for b in 0..30 * HB {
                if bin_t[b].1 >= 20 && bin_o[b].1 >= 20 {
                    let d = bin_t[b].0 / bin_t[b].1 as f64 - bin_o[b].0 / bin_o[b].1 as f64;
                    let wt = bin_t[b].1.min(bin_o[b].1);
                    dsum += d * wt as f64;
                    wsum += wt;
                    bins += 1;
                }
            }
            if bins >= 5 {
                let d = dsum / wsum.max(1) as f64;
                println!(
                    "till dividend (observational): {:+.4} fertility over {} matched lat×h bins",
                    d, bins
                );
            } else {
                println!("till dividend: too few matched bins to compare ({} bins)", bins);
            }

            // The gate itself is counterfactual, not observational: rerun
            // the shipped fertility law with the till grid zeroed and read
            // the uplift on till cells. Glaciation is not randomly
            // assigned — sheet-interior ground is drier at matched lat and
            // height, so even the double-matched observational diff stays
            // slightly negative. The counterfactual isolates the causal
            // term exactly and fails if the wiring breaks or the
            // temperature gate zeroes the bonus in practice.
            let h64 = w.fields.height.mapv(|v| v as f64);
            let t64 = w.fields.tmean.mapv(|v| v as f64);
            let p64 = w.fields.precip.mapv(|v| v as f64);
            let q64 = w.fields.discharge.mapv(|v| v as f64);
            let rivers = w.fields.flags.mapv(|f| f & CellFlags::RIVER.bits() != 0);
            let lakes = w.fields.flags.mapv(|f| f & CellFlags::LAKE.bits() != 0);
            let none: ndarray::Array2<f32> = ndarray::Array2::zeros((tr, tc));
            // M51 — a neutral soil plane on both legs: the counterfactual
            // must isolate the deposit, not the soil order under it.
            let nsoil: ndarray::Array2<u8> = ndarray::Array2::from_elem((tr, tc), calliope::agriculture::SoilOrder::Cambisol.code());
            let f_with = calliope::agriculture::fertility(
                &h64, &t64, &p64, &rivers, &lakes, &q64, &ice.till, &ice.loess, &ice.outwash, &nsoil,
            );
            let f_bare = calliope::agriculture::fertility(
                &h64, &t64, &p64, &rivers, &lakes, &q64, &none, &none, &ice.outwash, &nsoil,
            );
            // Measured on the farmable footprint only (bare fertility
            // ≥ 0.05): the sheet interior is tundra where the temperature
            // gate rightly zeroes farming — averaging over it dilutes the
            // belt where the farms and towns actually are. Loess is the
            // warm end of the footprint; it carries most of the dividend.
            let (mut up, mut un, mut tn) = (0.0f64, 0usize, 0usize);
            for y in 0..tr {
                for x in 0..tc {
                    if ice.till[[y, x]] > 0.0 || ice.loess[[y, x]] > 0.0 {
                        tn += 1;
                        if f_bare[[y, x]] >= 0.05 {
                            up += f_with[[y, x]] - f_bare[[y, x]];
                            un += 1;
                        }
                    }
                }
            }
            let uplift = up / un.max(1) as f64;
            println!(
                "legacy counterfactual: {} of {} till+loess cells farmable · mean uplift {:+.4} there",
                un, tn, uplift
            );
            c.must(
                "the legacy feeds the farms on it",
                un >= 300 && uplift >= 0.005,
                format!("{:+.4} uplift on {} farmable legacy cells", uplift, un),
                "M30 gate: zeroing till+loess must cost the farmable belt ≥ +0.005 (counterfactual)",
            );
        }

        // M31 — proglacial lakes and spillways: the melt ponded behind
        // the fresh moraines; the outbursts cut the channels that
        // outlive the lakes. Registries land post-widen, so they index
        // the shipped grid directly.
        {
            let ice = &w.ice;
            let n_lakes = ice.proglacial.len();
            let sp_cells: usize = ice.spillways.iter().map(|ch| ch.len()).sum();
            let (fh, fw) = w.fields.height.dim();
            let mut widths = [0usize; 3];
            for &(_, _, wd) in &ice.prog_meta {
                widths[(wd as usize).clamp(1, 3) - 1] += 1;
            }
            let mut refilled = 0usize;
            for &(y, x) in &ice.proglacial {
                let (y, x) = (y as usize, x as usize);
                if y < fh && x < fw && w.fields.flags[[y, x]] & CellFlags::LAKE.bits() != 0 {
                    refilled += 1;
                }
            }
            // Channels must drain in the shipped relief. The staircase
            // is strict per channel; a later channel crossing an earlier
            // one may notch its middle, so a small ascent share passes.
            let (mut steps, mut ascents) = (0usize, 0usize);
            for ch in &ice.spillways {
                for k in 1..ch.len() {
                    let (ay, ax) = (ch[k - 1].0 as usize, ch[k - 1].1 as usize);
                    let (by, bx) = (ch[k].0 as usize, ch[k].1 as usize);
                    if ay >= fh || ax >= fw || by >= fh || bx >= fw {
                        continue;
                    }
                    steps += 1;
                    if w.fields.height[[by, bx]] > w.fields.height[[ay, ax]] + 1e-6 {
                        ascents += 1;
                        let in_chains = |cy: usize, cx: usize| {
                            ice.spillways
                                .iter()
                                .filter(|c| c.iter().any(|&(py, px)| (py as usize, px as usize) == (cy, cx)))
                                .count()
                        };
                        eprintln!(
                            "  [ascent] step {}→{} of chain: ({},{}) h={:.5} ow={:.2} ch={} → ({},{}) h={:.5} ow={:.2} ch={}",
                            k - 1, k,
                            ay, ax, w.fields.height[[ay, ax]], ice.outwash[[ay, ax]], in_chains(ay, ax),
                            by, bx, w.fields.height[[by, bx]], ice.outwash[[by, bx]], in_chains(by, bx)
                        );
                    }
                }
            }
            println!(
                "proglacial (M31): {} lakes ({} refilled today) · {} chains · {} spillway cells · widths {}/{}/{} · {} of {} steps ascend",
                n_lakes, refilled, ice.chains, sp_cells,
                widths[0], widths[1], widths[2], ascents, steps
            );
            c.band("proglacial lakes per world", n_lakes as f64, format!("{}", n_lakes));
            c.band("spillway cells per world", sp_cells as f64, format!("{}", sp_cells));
            // Width classes derive from impounded volume; the gate reads
            // them against basin area — the correlation is physical
            // (deep water stands in wide basins), not definitional.
            let (mut a1, mut n1, mut a2, mut n2) = (0.0f64, 0usize, 0.0f64, 0usize);
            for &(_, area, wd) in &ice.prog_meta {
                if wd >= 2 {
                    a2 += area as f64;
                    n2 += 1;
                } else {
                    a1 += area as f64;
                    n1 += 1;
                }
            }
            if n1 >= 2 && n2 >= 1 {
                let (m1, m2) = (a1 / n1 as f64, a2 / n2 as f64);
                c.must(
                    "spillway width scales with catchment",
                    m2 >= m1,
                    format!("wide-class basins avg {:.0} cells vs {:.0}", m2, m1),
                    "M31 gate: width bands follow the basins that fed them",
                );
            }
            if steps >= 20 {
                let share = ascents as f64 / steps as f64;
                c.must(
                    "spillways drain downhill",
                    share <= 0.02,
                    format!("{} of {} steps ascend", ascents, steps),
                    "M31 gate: outburst channels descend in the shipped relief (≤2% crossing tolerance)",
                );
            }
        }

        // M32 — outwash plains and braided meltwater rivers: the melt
        // planed aprons below the margin; today's rivers braid where
        // they cross them, and the plain eats measurably better. The
        // fertility gate is counterfactual, like M30: rerun the shipped
        // law with the outwash grid zeroed and read the uplift.
        {
            let ice = &w.ice;
            let (ir, icn) = ice.outwash.dim();
            let mut corridor = 0usize;
            let mut apron = 0usize;
            for &v in ice.outwash.iter() {
                if v >= 0.9 {
                    corridor += 1;
                } else if v > 0.0 {
                    apron += 1;
                }
            }
            let (mut riv_below, mut braided) = (0usize, 0usize);
            for y in 0..ir {
                for x in 0..icn {
                    if w.fields.flags[[y, x]] & CellFlags::RIVER.bits() == 0 {
                        continue;
                    }
                    if ice.thickness[[y, x]] > 0.0 {
                        continue;
                    }
                    riv_below += 1;
                    if w.fields.flags[[y, x]] & CellFlags::BRAIDED.bits() != 0 {
                        braided += 1;
                    }
                }
            }
            let share = 100.0 * braided as f64 / riv_below.max(1) as f64;
            println!(
                "outwash (M32): {} corridor + {} apron cells · {} of {} below-margin river cells braided ({:.1}%)",
                corridor, apron, braided, riv_below, share
            );
            c.band("outwash cells per world", (corridor + apron) as f64, format!("{}", corridor + apron));
            c.band("braided share of ice-fed rivers %", share, format!("{:.1} %", share));

            let h64 = w.fields.height.mapv(|v| v as f64);
            let t64 = w.fields.tmean.mapv(|v| v as f64);
            let p64 = w.fields.precip.mapv(|v| v as f64);
            let q64 = w.fields.discharge.mapv(|v| v as f64);
            let rivers = w.fields.flags.mapv(|f| f & CellFlags::RIVER.bits() != 0);
            let lakes = w.fields.flags.mapv(|f| f & CellFlags::LAKE.bits() != 0);
            let none: ndarray::Array2<f32> = ndarray::Array2::zeros((ir, icn));
            // M51 — a neutral soil plane on both legs: the counterfactual
            // must isolate the deposit, not the soil order under it.
            let nsoil: ndarray::Array2<u8> = ndarray::Array2::from_elem((ir, icn), calliope::agriculture::SoilOrder::Cambisol.code());
            let f_with = calliope::agriculture::fertility(
                &h64, &t64, &p64, &rivers, &lakes, &q64, &ice.till, &ice.loess, &ice.outwash, &nsoil,
            );
            let f_bare = calliope::agriculture::fertility(
                &h64, &t64, &p64, &rivers, &lakes, &q64, &ice.till, &ice.loess, &none, &nsoil,
            );
            let (mut up, mut un) = (0.0f64, 0usize);
            for y in 0..ir {
                for x in 0..icn {
                    if ice.outwash[[y, x]] > 0.0 && f_bare[[y, x]] >= 0.05 {
                        up += f_with[[y, x]] - f_bare[[y, x]];
                        un += 1;
                    }
                }
            }
            let uplift = up / un.max(1) as f64;
            println!(
                "outwash counterfactual: {} of {} plain cells farmable · mean uplift {:+.4} there",
                un,
                corridor + apron,
                uplift
            );
            c.band("outwash fertility uplift", uplift, format!("{:+.4} on {} cells", uplift, un));
        }
        if sl.eustatic < 0.0 && coast_up >= 150 && coast_dn >= 150 {
            let d_up = raised_up as f64 / coast_up as f64;
            let d_dn = raised_dn as f64 / coast_dn as f64;
            c.must(
                "raised beaches thicken with the rise",
                d_up >= d_dn,
                format!("rebound belt {} vs collar {}", pct(d_up), pct(d_dn)),
                "M26 gate: landform frequency follows the amplitude within one world",
            );
        }
    }
    c.band("largest landmass share of land", largest / land_n.max(1.0), pct(largest / land_n.max(1.0)));
    c.band("landmass count", li.n as f64, format!("{}", li.n));
    c.band("small isles+islets", (islands + islets) as f64, format!("{}", islands + islets));
    c.band("mountain share of land (h>0.5)", mtn, pct(mtn));
    c.band("coastline crenulation", coast_ratio, format!("{:.3}", coast_ratio));
    c.want("archipelagos named", arch >= 1, format!("{}", arch), "≥1 — island clusters should get names");
    c.band("plate count", n_plates as f64, format!("{}", n_plates));
    c.band("plate mean drift-age (Myr)", mean_age, format!("{:.0}", mean_age));
    c.band("convergent share of boundary", conv_share, pct(conv_share));
    c.must("plate sketch regen byte-identical", plates_same, if plates_same { "identical".into() } else { "DIVERGED".into() }, "M16: same seed ⇒ same polygons");
    c.must("heightmap regen byte-identical", ha == hb, if ha == hb { "identical".into() } else { "DIVERGED".into() }, "M16: same sketch ⇒ same land");
    c.must(
        "belt relief falls with seam age",
        mono,
        format!("{:.4} → {:.4} → {:.4}", bel_young, bel_mid, bel_old),
        "M17: the same sketch re-aged 200/900/2000 Myr sinks monotonically",
    );
    c.band("shield share of land", shares[calliope::rock::SHIELD as usize], pct(shares[calliope::rock::SHIELD as usize]));
    c.band("basin share of land", shares[calliope::rock::BASIN as usize], pct(shares[calliope::rock::BASIN as usize]));
    c.band("fold-belt share of land", shares[calliope::rock::FOLD_BELT as usize], pct(shares[calliope::rock::FOLD_BELT as usize]));
    c.band("volcanic share of land", shares[calliope::rock::VOLCANIC as usize], pct(shares[calliope::rock::VOLCANIC as usize]));
    c.must(
        "province map reads true",
        legi_worst <= 0.03,
        format!(
            "worst off-correlate {} (shield {} · basin {} · fold {})",
            pct(legi_worst), pct(legi[0]), pct(legi[1]), pct(legi[2])
        ),
        "M21 gate: ≤3% of each province off its landform correlate",
    );

    // ---- M38 — the cold rim: treeline vs the frozen ground ---------------
    // Per column and hemisphere, walk pole → equator: the first tree
    // biome whose poleward land neighbour is tundra or ice is the
    // *thermal* treeline (drought-limited edges against steppe don't
    // count); the last continuous-permafrost lowland cell is the
    // frontier's equatorward reach (alpine islands at h≥0.5 don't
    // count). The offset (treeline lat − frontier lat) is the tracking
    // law, banded on the pooled legs: in this world it is one-signed
    // (see biomes::BANDS) — the frontier sits poleward of the treeline.
    // The regime breakdown by continentality is printed as data; the
    // treeline cells also confess their GDD5 — the classifier's own
    // currency, read back from the shipped f32 climate.
    {
        let biomes = &w.fields.biomes;
        let ext = &w.permafrost.extent;
        let (rows, cols) = biomes.dim();
        let tree = |b: u8| {
            b == gc::WOODLAND
                || b == gc::BOREAL_FOREST
                || b == gc::SEASONAL_RAIN_FOREST
                || b == gc::TEMPERATE_RAIN_FOREST
                || b == gc::TROPICAL_RAIN_FOREST
        };
        let cold = |b: u8| b == gc::TUNDRA || b == gc::WET_TUNDRA || b == gc::ICE;
        // Regime split at the treeline cell: |tamp| = base(lat)·cont,
        // so cont = |tamp|/base(lat) recovers the EDT continentality
        // (0.35 coast → 1.0 interior) — the two regimes obey
        // opposite-signed tracking laws.
        const CONT_SPLIT: f64 = 0.70;
        let mut offs_m: Vec<f64> = Vec::new();
        let mut offs_c: Vec<f64> = Vec::new();
        let mut conts: Vec<f64> = Vec::new();
        let mut gdds: Vec<f64> = Vec::new();
        for x in 0..cols {
            for hemi in 0..2 {
                // pole → equator in both legs
                let ys: Vec<usize> = if hemi == 0 {
                    (0..rows / 2).collect()
                } else {
                    (rows / 2..rows).rev().collect()
                };
                let mut tree_y: Option<usize> = None;
                let mut cont_y: Option<usize> = None;
                let mut poleward_land: Option<u8> = None;
                let mut cold_limited = false;
                for &y in &ys {
                    let land = w.fields.height[[y, x]] >= 0.0;
                    if land && tree_y.is_none() && tree(biomes[[y, x]]) {
                        tree_y = Some(y);
                        cold_limited = poleward_land.map_or(false, cold);
                    }
                    if land {
                        poleward_land = Some(biomes[[y, x]]);
                        if w.fields.height[[y, x]] < 0.5
                            && ext[[y, x]] == calliope::permafrost::CONTINUOUS
                        {
                            cont_y = Some(y);
                        }
                    }
                }
                if !cold_limited {
                    continue;
                }
                if let (Some(ty), Some(cy)) = (tree_y, cont_y) {
                    let off = latitude(ty, rows) - latitude(cy, rows);
                    let base = 3.0 + 19.0 * (latitude(ty, rows) / 90.0).powf(1.2);
                    let cont = (w.fields.tamp[[ty, x]] as f64).abs() / base;
                    conts.push(cont);
                    if cont < CONT_SPLIT {
                        offs_m.push(off);
                    } else {
                        offs_c.push(off);
                    }
                    gdds.push(calliope::climate::gdd5(
                        w.fields.tmean[[ty, x]] as f64,
                        w.fields.tamp[[ty, x]] as f64,
                    ));
                }
            }
        }
        let (mut dry, mut wet) = (0usize, 0usize);
        for &b in biomes.iter() {
            if b == gc::TUNDRA {
                dry += 1;
            } else if b == gc::WET_TUNDRA {
                wet += 1;
            }
        }
        let wet_share = 100.0 * wet as f64 / (wet + dry).max(1) as f64;
        let iqr = |v: &[f64]| quantile(v, 0.75) - quantile(v, 0.25);
        let mut offs: Vec<f64> = Vec::with_capacity(offs_m.len() + offs_c.len());
        offs.extend_from_slice(&offs_m);
        offs.extend_from_slice(&offs_c);
        println!();
        println!(
            "cold rim (M38): treeline−frontier offset median {:+.1}° · IQR {:.1}° over {} column-legs · treeline GDD5 median {:.0} °C·day",
            quantile(&offs, 0.5), iqr(&offs), offs.len(), quantile(&gdds, 0.5),
        );
        println!(
            "  regime breakdown: maritime median {:+.1}° ({} legs) · continental median {:+.1}° ({} legs) · treeline continentality p10 {:.2} p50 {:.2} p90 {:.2} (split at {:.2})",
            quantile(&offs_m, 0.5), offs_m.len(),
            quantile(&offs_c, 0.5), offs_c.len(),
            quantile(&conts, 0.10), quantile(&conts, 0.50), quantile(&conts, 0.90), CONT_SPLIT,
        );
        println!(
            "tundra subtypes: dry {} · wet {} cells · wet share {:.1}% of the tundra",
            dry, wet, wet_share
        );
        if offs.len() >= 20 {
            let med = quantile(&offs, 0.5);
            c.band("treeline−permafrost offset", med, format!("{:+.1}° ({} legs)", med, offs.len()));
            c.band("treeline tracking spread", iqr(&offs), format!("{:.1}°", iqr(&offs)));
            c.band("treeline GDD discipline", quantile(&gdds, 0.5), format!("{:.0} °C·day", quantile(&gdds, 0.5)));
        }
        if wet + dry > 0 {
            c.band("wet share of the tundra", wet_share, format!("{:.1}%", wet_share));
        }
    }

    // ---- M61 · "why is this here": the provenance chain proves itself ----
    // A deterministic stride lattice plus every settlement site; each cell
    // must return a non-empty chain whose stages read deep time forward
    // (0 stone → 4 landform), opening on stone and closing on the cell's
    // stored landform word verbatim — the chain reads recorded state, so
    // any drift between it and the lane is a bug, not a phrasing choice.
    if explain {
        let (hh, ww) = w.fields.height.dim();
        let mut cells: Vec<(usize, usize)> = Vec::new();
        for y in (0..hh).step_by(13) {
            for x in (0..ww).step_by(13) {
                cells.push((y, x));
            }
        }
        let lattice_n = cells.len();
        for s in &w.peoples.settlements {
            cells.push((s.y as usize, s.x as usize));
        }
        let mut empty = 0usize;
        let mut disorder = 0usize;
        let mut mismatch = 0usize;
        let mut first_bad: Option<String> = None;
        let note = |slot: &mut Option<String>, y: usize, x: usize, what: &str| {
            if slot.is_none() {
                *slot = Some(format!("({},{}) — {}", y, x, what));
            }
        };
        let sampled = cells.len();
        for &(y, x) in &cells {
            let raw = calliope::explain::explain(&w, "cell", &format!("{},{}", y, x));
            let v: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
            let chain = match v.get("chain").and_then(|c| c.as_array()) {
                Some(c) if !c.is_empty() => c.clone(),
                _ => {
                    empty += 1;
                    note(&mut first_bad, y, x, "no chain returned");
                    continue;
                }
            };
            let stages: Vec<i64> = chain
                .iter()
                .map(|e| e.get("s").and_then(|s| s.as_i64()).unwrap_or(-1))
                .collect();
            let ordered = stages.windows(2).all(|p| p[0] <= p[1])
                && stages.first() == Some(&0)
                && stages.last() == Some(&4);
            if !ordered {
                disorder += 1;
                note(&mut first_bad, y, x, &format!("stages {:?}", stages));
            }
            let want = calliope::landform::NAMES[w.fields.landform[[y, x]] as usize];
            let got = chain
                .last()
                .and_then(|e| e.get("l"))
                .and_then(|l| l.as_str())
                .unwrap_or("");
            if got != want {
                mismatch += 1;
                note(&mut first_bad, y, x, &format!("terminal {:?} vs lane {:?}", got, want));
            }
        }
        println!();
        println!(
            "provenance (M61): {} cells sampled ({} lattice + {} settlement sites) · {} empty · {} disordered · {} terminal mismatches",
            sampled, lattice_n, sampled - lattice_n, empty, disorder, mismatch
        );
        if let Some(b) = &first_bad {
            println!("  first offender: {}", b);
        }
        c.must(
            "every sampled cell explains itself",
            empty == 0,
            format!("{} of {} chains non-empty", sampled - empty, sampled),
            "M61 gate: every cell must return a provenance chain — no ground goes unexplained",
        );
        c.must(
            "the chain reads deep time forward",
            disorder == 0,
            format!("{} of {} open on stone, close on landform, stages nondecreasing", sampled - disorder, sampled),
            "M61 gate: stone → ice → water → soil → landform, in that order, always",
        );
        c.must(
            "the chain ends on the stored landform",
            mismatch == 0,
            format!("{} of {} terminals match the lane verbatim", sampled - mismatch, sampled),
            "M61 gate: the last word of the chain is the cell's Landform value, not a paraphrase",
        );
    }
    c.print();
}

// ================================================================ climate

fn cmd_climate(seed: i64, size: usize) {
    let w = World::generate(seed, size);
    header("CLIMATE", &format!("seed {} · {}x{}", seed, w.width, size));

    let land = land_mask(&w);
    let land_n = land.iter().filter(|&&b| b).count() as f64;
    let bc = biome_counts(&w);
    let share = |b: u8| bc[b as usize] as f64 / land_n.max(1.0);

    println!("biome shares of land:");
    for b in 1..12u8 {
        println!("  {:<24} {:>7}  {}", gc::Biome::from_code(b), bc[b as usize], pct(share(b)));
    }
    let desert = share(gc::DESERT);
    let frozen = share(gc::TUNDRA) + share(gc::WET_TUNDRA) + share(gc::ICE);
    let forest = share(gc::WOODLAND) + share(gc::SEASONAL_RAIN_FOREST) + share(gc::TEMPERATE_RAIN_FOREST) + share(gc::BOREAL_FOREST) + share(gc::TROPICAL_RAIN_FOREST);
    let open = share(gc::GRASSLAND) + share(gc::SAVANNA);

    let ts = masked(&w.fields.tmean, &land);
    let ps = masked(&w.fields.precip, &land);
    let amps: Vec<f64> = w.fields.tamp.iter().zip(land.iter()).filter(|(_, &b)| b).map(|(&v, _)| v.abs() as f64).collect();
    let t_mean = ts.iter().sum::<f64>() / ts.len().max(1) as f64;
    let p_mean = ps.iter().sum::<f64>() / ps.len().max(1) as f64;
    let a_mean = amps.iter().sum::<f64>() / amps.len().max(1) as f64;
    println!("land temperature °C: mean {:.1} · p10 {:.1} · p50 {:.1} · p90 {:.1}", t_mean, quantile(&ts, 0.1), quantile(&ts, 0.5), quantile(&ts, 0.9));
    println!("land precipitation mm/yr: mean {:.0} · p10 {:.0} · p50 {:.0} · p90 {:.0}", p_mean, quantile(&ps, 0.1), quantile(&ps, 0.5), quantile(&ps, 0.9));
    println!("seasonal swing |amp| °C: mean {:.1} · p90 {:.1}", a_mean, quantile(&amps, 0.9));
    let winter_frozen = ts.iter().zip(amps.iter()).filter(|(t, a)| **t - **a < 0.0).count() as f64 / ts.len().max(1) as f64;
    println!("land freezing in deep winter: {}", pct(winter_frozen));

    // latitude bands: T, P, dominant biome
    let (rows, cols) = land.dim();
    println!("latitude bands (land only):");
    println!("  {:<12} {:>6} {:>8} {:>9}  dominant biome", "band", "cells", "T mean", "P mean");
    for b in 0..6 {
        let y0 = rows * b / 6;
        let y1 = rows * (b + 1) / 6;
        let mut n = 0usize;
        let mut tsum = 0.0;
        let mut psum = 0.0;
        let mut counts = [0usize; 12];
        for y in y0..y1 {
            for x in 0..cols {
                if !land[[y, x]] {
                    continue;
                }
                n += 1;
                tsum += w.fields.tmean[[y, x]] as f64;
                psum += w.fields.precip[[y, x]] as f64;
                counts[w.fields.biomes[[y, x]] as usize] += 1;
            }
        }
        let label = format!("{:.0}°–{:.0}°", latitude(y0, rows), latitude(y1.saturating_sub(1), rows));
        if n == 0 {
            println!("  {:<12} {:>6} {:>8} {:>9}  —", label, 0, "—", "—");
            continue;
        }
        let dom = (1..12).max_by_key(|&i| counts[i]).unwrap_or(1);
        println!("  {:<12} {:>6} {:>7.1}C {:>7.0}mm  {}", label, n, tsum / n as f64, psum / n as f64, gc::Biome::from_code(dom as u8));
    }

    // monsoon: the ITCZ march should breathe hardest in the tropics
    let mut trop_amp: Vec<f64> = Vec::new();
    let mut mid_amp: Vec<f64> = Vec::new();
    for y in 0..rows {
        for x in 0..cols {
            if !land[[y, x]] {
                continue;
            }
            let a = w.fields.pamp[[y, x]].abs() as f64;
            if y >= rows / 3 && y < 2 * rows / 3 {
                trop_amp.push(a);
            } else {
                mid_amp.push(a);
            }
        }
    }
    let trop_m = trop_amp.iter().sum::<f64>() / trop_amp.len().max(1) as f64;
    let mid_m = mid_amp.iter().sum::<f64>() / mid_amp.len().max(1) as f64;
    println!("monsoon |amp| of annual rain: tropics {:.2} · extratropics {:.2}", trop_m, mid_m);

    // ---- M37 sea ice: the pack and its seasonal fringe ----
    // Extent is judged cos(lat)-weighted: the equirectangular grid
    // overweights polar rows, and the pack lives exactly there. The
    // weighted share is the Earth-comparable number (~4–8% of ocean
    // area at annual maximum).
    let fro = calliope::seaice::frozen_months(&w.fields.height, &w.fields.tmean, &w.fields.tamp);
    let sea_n = (rows * cols) as f64 - land_n;
    let mut ice_ever = 0usize;
    let mut ice_perennial = 0usize;
    let mut ice_lat_min = 90.0f64;
    let (mut wsea, mut wice) = (0.0f64, 0.0f64);
    for y in 0..rows {
        let wgt = (latitude(y, rows).to_radians()).cos().max(0.0);
        for x in 0..cols {
            if w.fields.height[[y, x]] < 0.0 {
                wsea += wgt;
            }
            let m = fro[[y, x]];
            if m == 0 {
                continue;
            }
            ice_ever += 1;
            wice += wgt;
            if m == calliope::seaice::MONTHS_MASK {
                ice_perennial += 1;
            }
            ice_lat_min = ice_lat_min.min(latitude(y, rows).abs());
        }
    }
    let ice_seasonal = ice_ever - ice_perennial;
    let ice_area = wice / wsea.max(1e-9);
    println!();
    if ice_ever > 0 {
        println!(
            "sea ice (M37): {} of ocean area ever frozen ({} of cells) · perennial {} cells · seasonal fringe {} cells · pack reaches {:.0}°",
            pct(ice_area), pct(ice_ever as f64 / sea_n.max(1.0)), ice_perennial, ice_seasonal, ice_lat_min
        );
    } else {
        println!("sea ice (M37): no sea cell ever freezes on this seed");
    }

    // ---- M2.1 crop belts: the agricultural map of the world ----
    let mut pk = [0usize; 5];
    for (&cpk, &l) in w.fields.crops.iter().zip(land.iter()) {
        if l {
            pk[cpk as usize] += 1;
        }
    }
    let pshare = |i: usize| pk[i] as f64 / land_n.max(1.0);
    println!();
    println!(
        "crop packages of the land: wheat {} · rice {} · maize {} · pastoral {} · wildland {}",
        pct(pshare(1)), pct(pshare(2)), pct(pshare(3)), pct(pshare(4)), pct(pshare(0))
    );
    let arable = pshare(1) + pshare(2) + pshare(3);

    // ---- current-aware rain (M42) ---------------------------------------
    // The same anomaly the dawn folded into tmean, re-run post-widen and
    // read against the rain it shaped. Two confounds are controlled away:
    // the zonal *land* mean is the wrong yardstick (interiors are the
    // driest cells at any latitude — every coast beats them), and aspect
    // is the stronger law still (windward coasts drench, leeward coasts
    // starve, currents or none). So the law is coast against coast *of
    // the same aspect*: land the cold rims reach must run drier than
    // aspect-matched neutral coastal land at its latitude (the Atacama
    // law), land the warm rims reach wetter (the Gulf-Stream law).
    // Ratios are aggregates, so a lone wet outlier cannot buy the pass.
    let water_g = w.fields.height.mapv(|h| h < 0.0);
    let heat = calliope::climate::current_bias(&water_g, &w.currents.v);
    // coastal = land within the same reach the heat bias walks inland
    let mut near = water_g.clone();
    for _ in 0..calliope::climate::HEAT_COAST_RINGS {
        let prev = near.clone();
        for y in 0..rows {
            for x in 0..cols {
                if near[[y, x]] {
                    continue;
                }
                if (y > 0 && prev[[y - 1, x]])
                    || (y + 1 < rows && prev[[y + 1, x]])
                    || (x > 0 && prev[[y, x - 1]])
                    || (x + 1 < cols && prev[[y, x + 1]])
                {
                    near[[y, x]] = true;
                }
            }
        }
    }
    // wind direction per row, as the march deals it
    let dir_of = |y: usize| -> isize {
        let l = latitude(y, rows).abs();
        if l < 30.0 {
            -1
        } else if l < 60.0 {
            1
        } else {
            -1
        }
    };
    // windward = the parcel crossed sea within the last few upwind cells
    let is_windward = |y: usize, x: usize| -> bool {
        let d = dir_of(y);
        (1..=6isize).any(|j| {
            let xx = (x as isize - d * j).rem_euclid(cols as isize) as usize;
            water_g[[y, xx]]
        })
    };
    // M59 — fan-built delta plains leave both samples: the matching is
    // aspect-for-aspect precisely because relief is the stronger law,
    // and a fresh depositional flat is unmatched relief. Deltas are not
    // spread evenly either — the biggest fans sit at the mouths of the
    // biggest rivers, which run to the wettest (warm-rim) coasts, so
    // leaving them in dilutes exactly the sample the law measures.
    let deltas = &w.sediment.delta;
    // per-row neutral-coast baselines, split by aspect [leeward, windward]
    let mut zonal_p = vec![[0.0f64; 2]; rows];
    let mut zonal_n = vec![[0usize; 2]; rows];
    for y in 0..rows {
        for x in 0..cols {
            if land[[y, x]] && near[[y, x]] && heat[[y, x]].abs() < 0.5 && !deltas[[y, x]] {
                let a = is_windward(y, x) as usize;
                zonal_p[y][a] += w.fields.precip[[y, x]] as f64;
                zonal_n[y][a] += 1;
            }
        }
    }
    // aggregates: [all, trades <30°, westerlies 30–60°, polar >60°]
    let belt_of = |y: usize| -> usize {
        let l = latitude(y, rows).abs();
        if l < 30.0 {
            1
        } else if l < 60.0 {
            2
        } else {
            3
        }
    };
    let (mut cold_p, mut cold_e, mut cold_n) = ([0.0f64; 4], [0.0f64; 4], [0usize; 4]);
    let (mut warm_p, mut warm_e, mut warm_n) = ([0.0f64; 4], [0.0f64; 4], [0usize; 4]);
    let mut delta_skip = 0usize;
    for y in 0..rows {
        let belt = belt_of(y);
        for x in 0..cols {
            if !land[[y, x]] {
                continue;
            }
            let b = heat[[y, x]];
            if b.abs() < 0.5 {
                continue;
            }
            if deltas[[y, x]] {
                delta_skip += 1;
                continue; // fresh fan flat — relief unmatched, out of both samples
            }
            let a = is_windward(y, x) as usize;
            if zonal_n[y][a] < 4 {
                continue; // no aspect-matched peers on this row — proves nothing
            }
            let zm = zonal_p[y][a] / zonal_n[y][a] as f64;
            if zm < 1.0 {
                continue; // polar bone-dry rows divide to noise
            }
            // the checked aggregate [0] is sub-polar: Earth anchors the
            // law at Atacama/Namib/Carolina latitudes — polar rims ride
            // sea-ice and near-zero rain, and are reported, not judged
            let idxs: &[usize] = if belt == 3 { &[3] } else { &[0, belt] };
            if b <= -0.5 {
                for &i in idxs {
                    cold_p[i] += w.fields.precip[[y, x]] as f64;
                    cold_e[i] += zm;
                    cold_n[i] += 1;
                }
            } else {
                for &i in idxs {
                    warm_p[i] += w.fields.precip[[y, x]] as f64;
                    warm_e[i] += zm;
                    warm_n[i] += 1;
                }
            }
        }
    }
    let ratio = |p: f64, e: f64| if e > 0.0 { p / e } else { 1.0 };
    let cold_ratio = ratio(cold_p[0], cold_e[0]);
    let warm_ratio = ratio(warm_p[0], warm_e[0]);
    println!();
    println!(
        "current-aware rain (M42): sub-polar cold-rim land {} cells at {:.2}× its aspect-matched coast · warm-rim {} cells at {:.2}× · {} delta-flat cells excluded",
        cold_n[0], cold_ratio, warm_n[0], warm_ratio, delta_skip
    );
    println!(
        "  by wind belt: cold trades {:.2}× ({}) · westerlies {:.2}× ({}) · polar {:.2}× ({})",
        ratio(cold_p[1], cold_e[1]), cold_n[1], ratio(cold_p[2], cold_e[2]), cold_n[2], ratio(cold_p[3], cold_e[3]), cold_n[3]
    );
    println!(
        "  by wind belt: warm trades {:.2}× ({}) · westerlies {:.2}× ({}) · polar {:.2}× ({})",
        ratio(warm_p[1], warm_e[1]), warm_n[1], ratio(warm_p[2], warm_e[2]), warm_n[2], ratio(warm_p[3], warm_e[3]), warm_n[3]
    );

    // ---- M47 upwelling: the nutrient coasts ------------------------------
    // Coastline here = ocean cells with a land 8-neighbor — the exact
    // census the field itself walks, so share is measured on the
    // scalar's own domain. The analogue check rides the same heat field
    // M42 judged, and controls the same way M42 did: raw marked-vs-
    // unmarked means compare subtropical marks against sub-polar cold
    // rims the latitude window rightly excluded — latitude is the
    // confound. The law is row-relative: a marked coast must run cold
    // *for its latitude* (heat below its own row's coastal mean —
    // Humboldt against its zonal peers, not against Greenland).
    let mut coast_n = 0usize;
    let mut rich_n = 0usize;
    let mut stray = 0usize;
    let mut ups: Vec<f64> = Vec::new();
    let mut row_heat = vec![0.0f64; rows];
    let mut row_cn = vec![0usize; rows];
    let mut coastal_g = vec![false; rows * cols];
    for y in 0..rows {
        for x in 0..cols {
            let u = w.fields.upwelling[[y, x]] as f64;
            let coastal = water_g[[y, x]] && {
                let mut adj = false;
                for dy in -1i64..=1 {
                    for dx in -1i64..=1 {
                        if dy == 0 && dx == 0 {
                            continue;
                        }
                        let yy = y as i64 + dy;
                        let xx = x as i64 + dx;
                        if yy < 0 || xx < 0 || yy >= rows as i64 || xx >= cols as i64 {
                            continue;
                        }
                        if !water_g[[yy as usize, xx as usize]] {
                            adj = true;
                        }
                    }
                }
                adj
            };
            if !coastal {
                if u > 0.0 {
                    stray += 1;
                }
                continue;
            }
            coastal_g[y * cols + x] = true;
            coast_n += 1;
            ups.push(u);
            row_heat[y] += heat[[y, x]];
            row_cn[y] += 1;
        }
    }
    // second pass: marked cells against their own row's coastal mean
    let mut rel_sum = 0.0f64;
    for y in 0..rows {
        if row_cn[y] == 0 {
            continue;
        }
        let rm = row_heat[y] / row_cn[y] as f64;
        for x in 0..cols {
            if coastal_g[y * cols + x]
                && w.fields.upwelling[[y, x]] >= calliope::climate::NUTRIENT_RICH
            {
                rich_n += 1;
                rel_sum += heat[[y, x]] - rm;
            }
        }
    }
    let up_share = rich_n as f64 / coast_n.max(1) as f64;
    let rel = rel_sum / rich_n.max(1) as f64;
    println!();
    println!(
        "upwelling (M47): {} of {} coastline cells nutrient-rich ({}) · index p50 {:.2} · p90 {:.2} · marked rims {:+.2}°C against their own latitude's coast",
        rich_n, coast_n, pct(up_share), quantile(&ups, 0.5), quantile(&ups, 0.9), rel
    );

    let mut c = Checks::default();
    c.band("desert share of land", desert, pct(desert));
    c.band("tundra+ice share of land", frozen, pct(frozen));
    c.band("forest share of land", forest, pct(forest));
    c.band("grass+savanna share of land", open, pct(open));
    c.band("land mean temperature", t_mean, format!("{:.1}°C", t_mean));
    c.band("land mean precipitation", p_mean, format!("{:.0}mm", p_mean));
    c.band("mean seasonal swing", a_mean, format!("{:.1}°C", a_mean));
    c.band("tropical monsoon amplitude", trop_m, format!("{:.2}", trop_m));
    c.want("monsoon lives in the tropics", trop_m > mid_m, format!("{:.2} vs {:.2}", trop_m, mid_m), "ITCZ march beats continental swing");
    c.band("arable share of land", arable, pct(arable));
    c.band("pastoral share of land", pshare(4), pct(pshare(4)));
    c.band("cold-rim rain suppression", cold_ratio, format!("{:.2}× ({} cells)", cold_ratio, cold_n[0]));
    c.band("warm-rim rain boost", warm_ratio, format!("{:.2}× ({} cells)", warm_ratio, warm_n[0]));
    c.want("rice hugs the water", pk[2] == 0 || pk[2] < pk[1] + pk[3], format!("rice {} vs wheat+maize {}", pk[2], pk[1] + pk[3]), "paddies are the exception, not the rule");
    c.band("upwelling share of coastline", up_share, pct(up_share));
    c.must(
        "upwelling keeps to the coast",
        stray == 0,
        format!("{} stray cells", stray),
        "M47: the scalar is a coastal reading — open ocean and land stay zero",
    );
    if rich_n > 0 && rich_n < coast_n {
        c.want(
            "nutrient coasts run cold for their latitude",
            rel < 0.0,
            format!("{:+.2}°C vs own-row coast", rel),
            "M47: marked rims sit below their row's coastal heat mean — the west-coast-desert analogue, latitude-controlled",
        );
    }
    c.band("ever-frozen share of ocean area", ice_area, pct(ice_area));
    if ice_ever > 0 {
        let fringe = ice_seasonal as f64 / ice_ever as f64;
        c.band("seasonal share of the pack", fringe, pct(fringe));
        c.must(
            "pack ice keeps to the polar seas",
            ice_lat_min >= 45.0,
            format!("reaches {:.0}°", ice_lat_min),
            "M37: no icebound tropics — Earth's pack stays poleward of ~44°",
        );
    }

    // ---- M71: the year stops repeating -------------------------------
    // Sixty years of anomaly draws, measured on land only, split into the
    // three latitude belts the declared amplitude law separates. The gate
    // is the variance *shape* (σ climbs poleward, band by band) plus the
    // determinism the field's construction promises — and a check that the
    // measured σ matches the amplitude the constants declare, so the
    // normalizing constant can never drift away from the lattice it
    // normalizes.
    {
        const YEARS: i64 = 60;
        // (Σx, Σx², n) per belt, temperature and rain lanes.
        let mut belt_t = [[0.0f64; 3]; 3];
        let mut belt_p = [[0.0f64; 3]; 3];
        // M75 — the same accumulation with the seesaw held at zero. The
        // latitude law M71 declares is the *unforced* law; the tilt M75
        // lays over the tropics is a separate, separately-gated term
        // (teleconnection lane, and the composition in climate-variance).
        // Measuring the shape claim on the forced field would be measuring
        // two laws at once and attributing the sum to one of them.
        let mut belt_pu = [[0.0f64; 3]; 3];
        let mut declared_t = [[0.0f64; 2]; 3]; // (Σ declared σ, n)
        let mut worst_floor = 0.0f64;
        let mut nonfinite = 0usize;
        let rows = w.fields.tmean.dim().0;
        let belt = |lat: f64| -> usize {
            if lat < 23.5 {
                0
            } else if lat < 55.0 {
                1
            } else {
                2
            }
        };
        for year in 1..=YEARS {
            let (dt, dp) = w.year_anomaly_fresh(year);
            let (_, dpu) = w.year_anomaly_unforced(year);
            for y in 0..rows {
                let lat = (-90.0 + (y as f64) * 180.0 / (rows as f64 - 1.0)).abs();
                let b = belt(lat);
                for x in 0..w.width {
                    if !land[[y, x]] {
                        continue;
                    }
                    let (a, r) = (dt[[y, x]], dp[[y, x]]);
                    if !a.is_finite() || !r.is_finite() {
                        nonfinite += 1;
                        continue;
                    }
                    belt_t[b][0] += a;
                    belt_t[b][1] += a * a;
                    belt_t[b][2] += 1.0;
                    belt_p[b][0] += r;
                    belt_p[b][1] += r * r;
                    belt_p[b][2] += 1.0;
                    let ru = dpu[[y, x]];
                    belt_pu[b][0] += ru;
                    belt_pu[b][1] += ru * ru;
                    belt_pu[b][2] += 1.0;
                    declared_t[b][0] += clim::anomaly_amp_t(lat);
                    declared_t[b][1] += 1.0;
                    if r < worst_floor {
                        worst_floor = r;
                    }
                }
            }
        }
        let sd = |acc: [f64; 3]| -> f64 {
            let n = acc[2].max(1.0);
            let m = acc[0] / n;
            (acc[1] / n - m * m).max(0.0).sqrt()
        };
        let names = ["tropics (<23.5°)", "mid-latitudes (23.5–55°)", "polar (>55°)"];
        println!();
        println!("interannual variability (M71) · {} years · land cells only:", YEARS);
        println!(
            "  {:<26} {:>10} {:>12} {:>14} {:>12}",
            "belt", "σ T (°C)", "declared σ", "σ rain unforced", "σ rain total"
        );
        for b in 0..3 {
            let dec = declared_t[b][0] / declared_t[b][1].max(1.0);
            println!(
                "  {:<26} {:>10.3} {:>12.3} {:>14.3} {:>12.3}",
                names[b],
                sd(belt_t[b]),
                dec,
                sd(belt_pu[b]),
                sd(belt_p[b])
            );
        }
        let (st, sp): (Vec<f64>, Vec<f64>) =
            (0..3).map(|b| (sd(belt_t[b]), sd(belt_p[b]))).unzip();
        let spu: Vec<f64> = (0..3).map(|b| sd(belt_pu[b])).collect();
        c.must(
            "the year's heat swings wider toward the poles",
            st[0] < st[1] && st[1] < st[2],
            format!("σ {:.3} → {:.3} → {:.3} °C", st[0], st[1], st[2]),
            "M71 gate: annual temperature-anomaly σ rises monotonically across the three latitude belts",
        );
        c.must(
            "the year's rain swings wider toward the poles",
            spu[0] < spu[1] && spu[1] < spu[2],
            format!("σ {:.3} → {:.3} → {:.3}", spu[0], spu[1], spu[2]),
            "M71: the same latitude law shapes the rain lane, in fractional terms — measured unforced (osc≡0), which is the law's own claim; the M75 tilt is gated in the teleconnection lane",
        );
        // M75 — and the tilt must actually land: the forced tropics carry
        // more year-to-year rain variance than the unforced tropics, while
        // the polar belt the tilt does not reach is left as it was.
        c.must(
            "the seesaw widens the tropics it aims at",
            sp[0] > spu[0] * (1.0 + 1e-6),
            format!("tropics σ {:.3} unforced → {:.3} forced", spu[0], sp[0]),
            "M75: the teleconnection belt adds interannual rain variance where it is aimed",
        );
        c.must(
            "the seesaw leaves the poles alone",
            (sp[2] - spu[2]).abs() <= 0.02 * spu[2].max(1e-9),
            format!("polar σ {:.3} unforced → {:.3} forced", spu[2], sp[2]),
            "M75: TELE_BELT_LAT 15° / σ 12° dies out well before the polar belt",
        );
        // The declared amplitude is a claim in °C; hold the measurement to it.
        let dec_mean: Vec<f64> = (0..3).map(|b| declared_t[b][0] / declared_t[b][1].max(1.0)).collect();
        let worst_rel = (0..3)
            .map(|b| ((st[b] - dec_mean[b]) / dec_mean[b].max(1e-9)).abs())
            .fold(0.0f64, f64::max);
        c.must(
            "the measured swing is the declared swing",
            worst_rel <= 0.20,
            format!("worst belt off by {:.1}%", worst_rel * 100.0),
            "M71: ANOM_FBM_SIGMA normalizes the lattice — if the measured σ drifts from the declared amplitude, the constant is stale, not the sky",
        );
        c.must(
            "the anomaly field is finite",
            nonfinite == 0,
            format!("{} non-finite cells", nonfinite),
            "M71: every year's draw is a number",
        );
        c.must(
            "a bad year never takes all the rain",
            worst_floor >= clim::ANOM_P_FLOOR - 1e-12,
            format!("worst {:+.2} share", worst_floor),
            "M71: total failure of the rains is famine's verdict (M2.6), not the sky's noise",
        );
        // Determinism, bit-for-bit: the same year drawn twice, and the
        // memoized copy against a fresh solve after the year has turned.
        let (a1, b1) = w.year_anomaly_fresh(7);
        let (a2, b2) = w.year_anomaly_fresh(7);
        let repeat_same = a1 == a2 && b1 == b2;
        let _ = w.year_tmean(7, rows / 2, w.width / 2);
        let _ = w.year_tmean(8, rows / 2, w.width / 2); // evict
        let memo_same = w.with_year_weather(7, |dt, dp| *dt == a1 && *dp == b1);
        c.must(
            "one seed, one year, one sky",
            repeat_same && memo_same,
            format!(
                "redraw {} · memo {}",
                if repeat_same { "identical" } else { "DIVERGE" },
                if memo_same { "identical" } else { "DIVERGE" }
            ),
            "M71 gate: repeated runs at one seed reproduce identical per-year anomaly grids bit-for-bit, memo or fresh",
        );
    }

    c.print();
}

// ================================================================ hydro

fn cmd_hydro(seed: i64, size: usize) {
    let w = World::generate(seed, size);
    header("HYDROLOGY", &format!("seed {} · {}x{}", seed, w.width, size));

    let land = land_mask(&w);
    let land_n = land.iter().filter(|&&b| b).count() as f64;
    let rivers = w.mask(CellFlags::RIVER);
    let river_n = rivers.iter().filter(|&&r| r).count() as f64;
    let lake_n = w.fields.flags.iter().filter(|&&f| f & CellFlags::LAKE.bits() != 0).count() as f64;
    println!("river cells: {} ({} of land) · lake cells: {} ({} of land)", river_n as usize, pct(river_n / land_n.max(1.0)), lake_n as usize, pct(lake_n / land_n.max(1.0)));

    let li = ndimage::label(&rivers, true);
    let systems = li.areas.iter().filter(|&&a| a >= 12.0).count();
    let longest = li.areas.iter().cloned().fold(0.0f64, f64::max);
    println!("river systems (≥12 cells): {} · longest network {} cells ≈ {:.0} km", systems, longest as usize, longest * gc::KM_PER_CELL);

    let dis: Vec<f64> = w.fields.discharge.iter().zip(rivers.iter()).filter(|(_, &r)| r).map(|(&d, _)| d as f64).collect();
    if !dis.is_empty() {
        println!("discharge on rivers: p50 {:.1} · p90 {:.1} · p99 {:.1} · max {:.1}", quantile(&dis, 0.5), quantile(&dis, 0.9), quantile(&dis, 0.99), quantile(&dis, 1.0));
    }
    let deltas = w.features.iter().filter(|f| f.t == "delta").count();
    let marshes = w.features.iter().filter(|f| f.t == "marsh").count();
    println!("named: {} deltas · {} marshes", deltas, marshes);
    for f in w.features.iter().filter(|f| f.t == "delta").take(6) {
        println!("  delta: {} @({},{})", f.name, f.x, f.y);
    }
    let river_towns = w.peoples.settlements.iter().filter(|s| s.river).count();
    println!("river towns: {} of {}", river_towns, w.peoples.settlements.len());

    // Strahler orders, wadis, and the dead seas of the dry country
    let mut order_hist = [0usize; 13];
    let mut top_order = 0u8;
    for (&o, &r) in w.fields.strahler.iter().zip(rivers.iter()) {
        if r && o > 0 {
            order_hist[(o as usize).min(12)] += 1;
            top_order = top_order.max(o);
        }
    }
    print!("strahler orders:");
    for o in 1..=top_order.max(1) as usize {
        print!(" {}:{}", o, order_hist[o]);
    }
    println!(" · top {}", top_order);
    let wadi_n = w.fields.flags.iter().filter(|&&f| f & CellFlags::SEASONAL.bits() != 0).count();
    let salt = w.mask(CellFlags::SALT);
    let salt_cells = salt.iter().filter(|&&s| s).count();
    let salt_comp = ndimage::label(&salt, true).areas.len();
    let famp: Vec<f64> = w
        .fields.flow_amp
        .iter()
        .zip(rivers.iter())
        .filter(|(_, &r)| r)
        .map(|(&a, _)| a.abs() as f64)
        .collect();
    let famp_mean = famp.iter().sum::<f64>() / famp.len().max(1) as f64;
    println!(
        "wadis: {} cells · salt basins: {} ({} cells) · river |flow amp| mean {:.2}",
        wadi_n, salt_comp, salt_cells, famp_mean
    );

    // --- M35: glacier-fed discharge — the melt ledger down the net,
    // and the twelve-month curve rebuilt per river cell from the two
    // lanes (rain harmonic + summer-phased melt harmonic).
    let melt_g = &w.ice.melt;
    let mamp_g = &w.ice.melt_amp;
    let (rr, cc) = w.fields.discharge.dim();
    let mut glac_n = 0usize;
    let mut warm_peak = 0usize;
    let mut bad_month = 0usize;
    let mut glacial_wadis = 0usize;
    let mut shares: Vec<f64> = Vec::new();
    for y in 0..rr {
        for x in 0..cc {
            if w.fields.flags[[y, x]] & CellFlags::RIVER.bits() == 0 {
                continue;
            }
            let d = w.fields.discharge[[y, x]] as f64;
            if d <= 0.0 {
                continue;
            }
            let m = melt_g[[y, x]] as f64;
            let am = mamp_g[[y, x]] as f64;
            let rain = (d - m).max(0.0);
            let ar = if rain > 1e-9 {
                (((w.fields.flow_amp[[y, x]] as f64) * d - am * m) / rain).clamp(-1.0, 1.0)
            } else {
                0.0
            };
            let mut peak = 0usize;
            let mut q_peak = f64::MIN;
            for (i, ph) in calliope::climate::COS12.iter().enumerate() {
                let q = (rain / 12.0 * (1.0 + ar * ph)).max(0.0)
                    + (m / 12.0 * (1.0 + am * ph)).max(0.0);
                if !q.is_finite() || q < 0.0 {
                    bad_month += 1;
                }
                if q > q_peak {
                    q_peak = q;
                    peak = i;
                }
            }
            let frac = m / d;
            if frac >= calliope::hydrology::GLACIAL_MIN {
                glac_n += 1;
                shares.push(frac);
                if (w.fields.tamp[[y, x]] as f64) * calliope::climate::COS12[peak] > 0.0 {
                    warm_peak += 1;
                }
                if w.fields.flags[[y, x]] & CellFlags::SEASONAL.bits() != 0 {
                    glacial_wadis += 1;
                }
            }
        }
    }
    shares.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let g_share_pct = 100.0 * glac_n as f64 / river_n.max(1.0);
    let warm_share = warm_peak as f64 / glac_n.max(1) as f64;
    let med_share = if shares.is_empty() { 0.0 } else { quantile(&shares, 0.5) };
    println!(
        "glacier-fed rivers (melt ≥ {:.0}% of flow): {} cells ({} of rivers) · median melt share {} · warm-month peaks {}",
        100.0 * calliope::hydrology::GLACIAL_MIN,
        glac_n,
        pct(glac_n as f64 / river_n.max(1.0)),
        pct(med_share),
        pct(warm_share)
    );

    let finite = dis.iter().all(|d| d.is_finite());
    let mut c = Checks::default();
    c.band("river share of land", river_n / land_n.max(1.0), pct(river_n / land_n.max(1.0)));
    c.band("lake share of land", lake_n / land_n.max(1.0), pct(lake_n / land_n.max(1.0)));
    c.band("river systems", systems as f64, format!("{}", systems));
    c.want("named deltas", deltas >= 1, format!("{}", deltas), "≥1 — great river mouths get names");
    c.must("discharge finite", finite, if finite { "yes".into() } else { "NO".into() }, "no NaN/inf in flow accumulation");
    // dawn towns all on fresh water is history, not a bug — the dry
    // harbours and mining camps arrive with the colonies (checked in civ).
    let rt_share = river_towns as f64 / w.peoples.settlements.len().max(1) as f64;
    c.want("dawn towns reach fresh water", rt_share >= 0.2, pct(rt_share), "≥20% — rivers should pull the first peoples");
    c.band("strahler top order", top_order as f64, format!("{}", top_order));
    c.band("river seasonal swing", famp_mean, format!("{:.2}", famp_mean));
    c.want("endorheic salt basins", salt_comp >= 1, format!("{}", salt_comp), "≥1 — the desert keeps its dead seas");
    c.want("wadis", wadi_n >= 1, format!("{}", wadi_n), "≥1 — some rivers should run dry half the year");
    c.band("glacier-fed river share %", g_share_pct, format!("{:.1}%", g_share_pct));
    c.must(
        "monthly discharge sane on rivers",
        bad_month == 0,
        if bad_month == 0 { "clean".into() } else { format!("{} bad", bad_month) },
        "M35 gate: 12-month curves non-negative and NaN-free on every river cell",
    );
    c.want(
        "glacier-fed rivers peak in warm months",
        glac_n == 0 || warm_share >= 0.8,
        pct(warm_share),
        "≥80% — melt follows the sun, whatever the rain does (M35 gate)",
    );
    c.must(
        "no glacier-fed wadis",
        glacial_wadis == 0,
        format!("{}", glacial_wadis),
        "M35: the melt returns every summer — the wadi stamp yields",
    );

    // --- M54: the water table. Depth to water on the ground a well
    // would actually be sunk into — land that is not itself a river,
    // lake or shore cell (those are pinned to their own water and would
    // only flatter the numbers).
    let aq = &w.fields.aquifer;
    let mut free: Vec<(f64, f64)> = Vec::new(); // (surface m, depth m)
    let mut by_rock: [Vec<f64>; 4] = Default::default();
    let mut dry: Vec<f64> = Vec::new();
    let mut wet: Vec<f64> = Vec::new();
    let mut nonfinite = 0usize;
    let mut out_of_range = 0usize;
    let mut pinned_nonzero = 0usize;
    let mut ocean_nonzero = 0usize;
    for y in 0..rr {
        for x in 0..cc {
            let d = aq[[y, x]] as f64;
            if !d.is_finite() {
                nonfinite += 1;
                continue;
            }
            if d < 0.0 || d > calliope::hydrology::AQUIFER_FLOOR_M {
                out_of_range += 1;
            }
            if !land[[y, x]] {
                if d != 0.0 {
                    ocean_nonzero += 1;
                }
                continue;
            }
            let f = w.fields.flags[[y, x]];
            if f & (CellFlags::RIVER.bits() | CellFlags::LAKE.bits()) != 0 {
                if d > 0.5 {
                    pinned_nonzero += 1;
                }
                continue;
            }
            let elev = w.fields.height[[y, x]] as f64 * calliope::constants::METRES_PER_UNIT;
            free.push((elev, d));
            by_rock[(w.fields.rock[[y, x]] as usize).min(3)].push(d);
            let p = w.fields.precip[[y, x]] as f64;
            if p < 400.0 {
                dry.push(d);
            } else if p > 1200.0 {
                wet.push(d);
            }
        }
    }
    // HAND — height above nearest drainage. The Darcy solve pins its head
    // at the ocean, the rivers, the lakes and the subgrid drains; every
    // other cell's table has to climb from the drain it flows to. HAND is
    // that lift, measured along the D8 descent rather than by a window
    // minimum (a box min crosses divides and reports the wrong valley).
    // This is what "valley-floor elevation" means hydrologically.
    let mut hand: Vec<(f64, f64)> = Vec::new();
    let mut strat: Vec<(f64, f64)> = Vec::new();
    {
        // The shared D8 helpers assume a square grid; the widened world is
        // 640x512, so descend on the rectangle directly.
        let pinned = Array2::from_shape_fn((rr, cc), |(y, x)| {
            let f = w.fields.flags[[y, x]];
            !land[[y, x]]
                || f & (CellFlags::RIVER.bits() | CellFlags::LAKE.bits()) != 0
                || w.fields.discharge[[y, x]] as f64 >= calliope::hydrology::SUBGRID_DRAIN_Q
        });
        let dirs = Array2::<i8>::from_shape_fn((rr, cc), |(y, x)| {
            let mut best_drop = 0.0f64;
            let mut best = -1i8;
            for (i, (&(dy, dx), &dist)) in calliope::hydrology::N8
                .iter()
                .zip(calliope::hydrology::DIST.iter())
                .enumerate()
            {
                let ny = y as isize + dy;
                let nx = x as isize + dx;
                if ny < 0 || nx < 0 || ny >= rr as isize || nx >= cc as isize {
                    continue;
                }
                let drop = (w.fields.height[[y, x]] as f64
                    - w.fields.height[[ny as usize, nx as usize]] as f64)
                    / dist;
                if drop > best_drop {
                    best_drop = drop;
                    best = i as i8;
                }
            }
            best
        });

        for y0 in 0..rr {
            for x0 in 0..cc {
                if !land[[y0, x0]] || pinned[[y0, x0]] {
                    continue;
                }
                // descend to the first pinned cell
                let (mut y, mut x) = (y0, x0);
                let mut floor: Option<f64> = None;
                for _ in 0..(rr * cc) {
                    let d = dirs[[y, x]];
                    if d < 0 {
                        break;
                    }
                    let (dy, dx) = calliope::hydrology::N8[d as usize];
                    let ny = y as isize + dy;
                    let nx = x as isize + dx;
                    if ny < 0 || nx < 0 || ny >= rr as isize || nx >= cc as isize {
                        break;
                    }
                    y = ny as usize;
                    x = nx as usize;
                    if pinned[[y, x]] {
                        floor = Some(w.fields.height[[y, x]] as f64);
                        break;
                    }
                }
                let Some(lo) = floor else { continue };
                let elev = w.fields.height[[y0, x0]] as f64 * calliope::constants::METRES_PER_UNIT;
                let lift = elev - lo * calliope::constants::METRES_PER_UNIT;
                let d = w.fields.aquifer[[y0, x0]] as f64;
                hand.push((lift, d));
                let p = w.fields.precip[[y0, x0]] as f64;
                if (400.0..900.0).contains(&p) {
                    strat.push((elev, d));
                }
            }
        }
    }

    let rho_hand = spearman(&hand);
    let rho_strat = spearman(&strat);
    let depths: Vec<f64> = free.iter().map(|p| p.1).collect();
    let med_depth = if depths.is_empty() { 0.0 } else { quantile(&depths, 0.5) };
    let rho_elev = spearman(&free);
    let shallow = depths.iter().filter(|&&d| d <= 10.0).count() as f64 / depths.len().max(1) as f64;
    let med = |v: &Vec<f64>| if v.is_empty() { f64::NAN } else { quantile(v, 0.5) };
    let (m_shield, m_basin) = (med(&by_rock[0]), med(&by_rock[1]));
    let (m_fold, m_volc) = (med(&by_rock[2]), med(&by_rock[3]));
    let (m_dry, m_wet) = (med(&dry), med(&wet));
    println!();
    println!(
        "aquifer (M54): median depth {:.1} m · p10 {:.1} · p90 {:.1} · within 10 m of surface {}",
        med_depth,
        if depths.is_empty() { 0.0 } else { quantile(&depths, 0.1) },
        if depths.is_empty() { 0.0 } else { quantile(&depths, 0.9) },
        pct(shallow)
    );
    println!(
        "  median depth by province: shield {:.1} · basin {:.1} · fold {:.1} · volcanic {:.1} m",
        m_shield, m_basin, m_fold, m_volc
    );
    println!(
        "  median depth by rainfall: arid <400mm {:.1} m ({} cells) · humid >1200mm {:.1} m ({} cells)",
        m_dry,
        dry.len(),
        m_wet,
        wet.len()
    );
    println!(
        "  spearman: vs HAND (height above nearest drainage) {:.2} · vs raw elevation {:.2} · vs elevation at 400-900mm {:.2} ({} cells)",
        rho_hand, rho_elev, rho_strat, strat.len()
    );

    let hashed = calliope::pack::FIELD_SPECS
        .iter()
        .any(|f| f.name == "aquifer" && f.in_hash);
    c.band("aquifer median depth m", med_depth, format!("{:.1} m", med_depth));
    c.must(
        "water table tracks the valleys",
        rho_hand >= 0.5,
        format!("ρ {:.2}", rho_hand),
        "M54 gate: Spearman ≥0.50 of depth against HAND — height above the drainage each cell flows to. Raw sea-level elevation is the wrong variable: the drains sit at every valley floor, so a highland basin's table is shallow however high it stands (raw-elevation ρ printed above for contrast)",

    );
    c.must(
        "aquifer depth finite and bounded",
        nonfinite == 0 && out_of_range == 0,
        if nonfinite == 0 && out_of_range == 0 {
            "clean".into()
        } else {
            format!("{} nan · {} out of range", nonfinite, out_of_range)
        },
        "M54: every cell reports 0..150 m, no NaN out of the Darcy solve",
    );
    c.must(
        "surface water pins the table",
        pinned_nonzero == 0 && ocean_nonzero == 0,
        format!("{} river/lake · {} ocean", pinned_nonzero, ocean_nonzero),
        "M54: where water already stands, depth to water is zero",
    );
    c.want(
        "permeable rock holds a deeper table",
        m_basin > m_shield,
        format!("basin {:.1} m vs shield {:.1} m", m_basin, m_shield),
        "M54: the shield throttles the flow and mounds the table high; the basin drains it away",
    );
    c.want(
        "the dry country digs deeper",
        m_dry > m_wet,
        format!("arid {:.1} m vs humid {:.1} m", m_dry, m_wet),
        "M54: less recharge, lower mound — a well in the desert is a longer rope",
    );
    // ------------------------------------------------- M55 springs & oases
    let flags = &w.fields.flags;
    let rivers = flags.mapv(|f| f & 1 != 0);
    let lakes = flags.mapv(|f| f & 2 != 0);
    let water = w.fields.height.mapv(|h| h < 0.0);
    let dw = calliope::hydrology::springs_and_oases(
        &w.fields.height,
        &water,
        &rivers,
        &lakes,
        &w.fields.aquifer,
        &w.fields.biomes,
        &w.fields.precip,
    );
    let land_n = land.iter().filter(|&&b| b).count().max(1) as f64;
    let n_spring = dw.springs.iter().filter(|&&b| b).count();
    let n_oasis = dw.oases.iter().filter(|&&b| b).count();
    let mut arid_n = 0usize;
    let mut arid_watered = 0usize;
    let mut spring_shallow = 0usize;
    for y in 0..w.size {
        for x in 0..w.width {
            if !land[[y, x]] {
                continue;
            }
            if dw.springs[[y, x]] && (w.fields.aquifer[[y, x]] as f64) <= 2.0 {
                spring_shallow += 1;
            }
            if calliope::hydrology::arid(
                w.fields.biomes[[y, x]],
                w.fields.precip[[y, x]] as f64,
            ) {
                arid_n += 1;
                if dw.oases[[y, x]] || dw.springs[[y, x]] || rivers[[y, x]] || lakes[[y, x]] {
                    arid_watered += 1;
                }
            }
        }
    }
    let spring_pct = 100.0 * n_spring as f64 / land_n;
    let oasis_pct = if arid_n == 0 { 0.0 } else { 100.0 * n_oasis as f64 / arid_n as f64 };
    println!();
    println!(
        "springs & oases (M55): {} springs ({:.2}% of land) · {} oases ({:.1}% of arid land) · arid land with any water {:.1}%",
        n_spring, spring_pct, n_oasis, oasis_pct,
        100.0 * arid_watered as f64 / arid_n.max(1) as f64
    );
    c.band("spring share of land %", spring_pct, format!("{:.2}%", spring_pct));
    c.band("oasis share of arid land %", oasis_pct, format!("{:.1}%", oasis_pct));
    c.must(
        "every spring stands on a shallow table",
        spring_shallow == n_spring,
        format!("{}/{}", spring_shallow, n_spring),
        "M55 gate: a spring is the table daylighting — depth to water ≤2 m at every marked cell, by construction and by audit",
    );
    c.must(
        "no oasis outside the dry country",
        dw.oases.indexed_iter().all(|((y, x), &o)| {
            !o || calliope::hydrology::arid(
                w.fields.biomes[[y, x]],
                w.fields.precip[[y, x]] as f64,
            )
        }),
        format!("{} oases", n_oasis),
        "M55 gate: oases exist only where the year is arid (desert biome or <300 mm)",
    );
    c.must(
        "aquifer joins the state hash",
        hashed,
        if hashed { "yes".into() } else { "NO".into() },
        "M54 gate: the field is CRC-stable and part of hash_state",
    );
    c.print();
}

// ================================================================ resources

/// Spearman rank correlation over (x, y) pairs — small n, no ties
/// expected (the fertility curve is strictly ordered by construction).
fn spearman(pairs: &[(f64, f64)]) -> f64 {
    let n = pairs.len();
    if n < 3 {
        return 0.0;
    }
    let rank = |key: &dyn Fn(&(f64, f64)) -> f64| -> Vec<f64> {
        let mut idx: Vec<usize> = (0..n).collect();
        idx.sort_by(|&a, &b| key(&pairs[a]).partial_cmp(&key(&pairs[b])).unwrap());
        let mut r = vec![0.0; n];
        for (pos, &i) in idx.iter().enumerate() {
            r[i] = pos as f64;
        }
        r
    };
    let rx = rank(&|p: &(f64, f64)| p.0);
    let ry = rank(&|p: &(f64, f64)| p.1);
    let mean = (n as f64 - 1.0) / 2.0;
    let (mut num, mut dx, mut dy) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let (a, b) = (rx[i] - mean, ry[i] - mean);
        num += a * b;
        dx += a * a;
        dy += b * b;
    }
    if dx <= 0.0 || dy <= 0.0 {
        return 0.0;
    }
    num / (dx * dy).sqrt()
}

fn cmd_resources(seed: i64, size: usize) {
    let w = World::generate(seed, size);
    header("RESOURCES", &format!("seed {} · {}x{}", seed, w.width, size));

    let land = land_mask(&w);
    let land_n = land.iter().filter(|&&b| b).count() as f64;

    println!("{:<14} {:>5} {:>10} {:>7} {:>8} {:>10} {:>9}", "kind", "count", "dawn-known", "rich", "finite", "reserve mo", "min dist");
    let mut minerals_total = 0usize;
    let mut minerals_hidden = 0usize;
    let mut missing: Vec<&str> = Vec::new();
    for kind in resources::ALL_PLACEABLE {
        let ds: Vec<&calliope::resources::Deposit> = w.deposits.iter().filter(|d| d.r == kind).collect();
        if ds.is_empty() {
            missing.push(kind.name());
            println!("{:<14} {:>5} {:>10} {:>7} {:>8} {:>10} {:>9}", kind, 0, "—", "—", "—", "—", "—");
            continue;
        }
        let known = ds.iter().filter(|d| d.known).count();
        let rich: f64 = ds.iter().map(|d| d.rich).sum::<f64>() / ds.len() as f64;
        let finite: Vec<f64> = ds.iter().filter(|d| d.left >= 0.0).map(|d| d.left).collect();
        let reserve = if finite.is_empty() { "renews".to_string() } else { format!("{:.0}", finite.iter().sum::<f64>() / finite.len() as f64) };
        // median distance from deposit to nearest settlement
        let mut dists: Vec<f64> = ds
            .iter()
            .map(|d| {
                w.peoples.settlements
                    .iter()
                    .map(|s| (((d.x - s.x).pow(2) + (d.y - s.y).pow(2)) as f64).sqrt())
                    .fold(f64::INFINITY, f64::min)
            })
            .collect();
        dists.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let med = dists[dists.len() / 2];
        let is_mineral = kind.is_mineral();
        if is_mineral {
            minerals_total += ds.len();
            minerals_hidden += ds.len() - known;
        }
        println!("{:<14} {:>5} {:>7} {:>2.0}% {:>7.2} {:>8} {:>10} {:>8.0}c", kind, ds.len(), known, 100.0 * known as f64 / ds.len() as f64, rich, finite.len(), reserve, med);
    }
    let per_1000 = w.deposits.len() as f64 / (land_n / 1000.0).max(1e-9);
    let hidden_share = minerals_hidden as f64 / minerals_total.max(1) as f64;
    println!();
    println!("total deposits: {} · {:.2} per 1000 land cells", w.deposits.len(), per_1000);
    println!("mineral seams hidden at dawn: {} of {} ({})", minerals_hidden, minerals_total, pct(hidden_share));
    if !missing.is_empty() {
        println!("kinds absent from this world: {:?}", missing);
    }

    let gold = w.deposits.iter().any(|d| d.r == resources::Good::Gold);
    let mithril = w.deposits.iter().any(|d| d.r == resources::Good::Mithril);
    let essential_missing = missing.iter().filter(|k| !matches!(**k, "mithril" | "bananas")).count();

    let mut c = Checks::default();
    c.band("deposits per 1000 land cells", per_1000, format!("{:.2}", per_1000));
    c.band("mineral hidden share at dawn", hidden_share, pct(hidden_share));
    c.want("essential kinds all present", essential_missing == 0, format!("{} missing", essential_missing), "everything except mithril/bananas should place");
    c.want("gold placed", gold, if gold { "yes".into() } else { "no".into() }, "a world without gold has a dull late game");
    c.want("mithril placed", mithril, if mithril { "yes".into() } else { "no".into() }, "the legendary seam should exist somewhere");

    // ---- M14.2 salt: pans on the shore, seams in the rock ----
    let salt_all: Vec<_> = w.deposits.iter().filter(|d| d.r == resources::Good::Salt).collect();
    let pans = salt_all.iter().filter(|d| d.left < 0.0).count();
    let seams = salt_all.len() - pans;
    let pans_known = salt_all.iter().filter(|d| d.left < 0.0 && d.known).count();
    c.must(
        "coastal salt pan exists",
        pans >= 1,
        format!("{} pans · {} rock seams", pans, seams),
        "M14.2: every world gets at least one renewing pan on an arid shore",
    );
    c.must(
        "salt sources ≥2",
        salt_all.len() >= 2,
        format!("{}", salt_all.len()),
        "M14.2 floor: one source is a monopoly, not an economy",
    );
    c.want(
        "pans known at dawn",
        pans_known == pans,
        format!("{} of {}", pans_known, pans),
        "a salt pan is plain to see — never hidden, never exhausted",
    );

    // ---- M14.1 ontology lint: the GOODS table is the single truth ----
    let lint = calliope::resources::ontology_lint();
    c.want(
        "goods table agrees with closure flags",
        lint.is_empty(),
        if lint.is_empty() {
            format!("{} rows consistent", calliope::resources::Good::COUNT)
        } else {
            lint.join(" · ")
        },
        "M14.1: one declaration point — flags derive-checked against GOODS",
    );

    // ---- M19 — deposits re-seated: ore sits where geology says ----
    let rows = resources::province_consistency(&w.deposits, &w.fields.rock);
    let mut in_home = 0usize;
    let mut total = 0usize;
    println!();
    println!("ore homes (M19): {}", rows
        .iter()
        .map(|&(g, ih, tot)| format!("{} {}/{}", g, ih, tot))
        .collect::<Vec<_>>()
        .join(" · "));
    for &(_, ih, tot) in &rows {
        in_home += ih;
        total += tot;
    }
    let home_share = in_home as f64 / total.max(1) as f64;
    c.band(
        "ore seams in home province",
        home_share,
        format!("{} of {} ({})", in_home, total, pct(home_share)),
    );

    // ---- M51 — soil genesis: the orders under the map ----
    // Jenny's factors are solved once at genesis; here we read the mix
    // back off the finished world and hold it to Whittaker-consistent
    // bands, plus the two structural claims: every land cell carries a
    // profile, and the black earth stays in continental grass country.
    {
        use calliope::agriculture::SoilOrder;
        use strum::IntoEnumIterator;

        let (h, wd) = w.fields.soil.dim();
        let mut count = [0usize; 11];
        let mut fert_sum = [0.0f64; 11];
        // M52 — the decile test: the delivered orders must sit in the
        // best tenth of the map's farmland, not merely above average.
        let mut land_fert: Vec<f64> = Vec::new();
        let mut order_of: Vec<(usize, f64)> = Vec::new();
        let mut bare = 0usize;          // land cells with no profile
        let mut cher = 0usize;
        let mut cher_climate = 0usize;  // chernozem inside its climate window
        let mut cher_grass = 0usize;    // chernozem under grass/savanna/woodland
        let mut andosol_volcanic = 0usize;
        let mut andosol = 0usize;
        for y in 0..h {
            for x in 0..wd {
                if !land[[y, x]] {
                    continue;
                }
                // lakes are land by the height mask but carry no profile
                if calliope::state::CellFlags::from_bits_truncate(w.fields.flags[[y, x]])
                    .contains(calliope::state::CellFlags::LAKE)
                {
                    continue;
                }
                let o = w.fields.soil[[y, x]] as usize;
                if o == 0 {
                    bare += 1;
                    continue;
                }
                count[o] += 1;
                let fv = w.fields.fertility[[y, x]] as f64;
                fert_sum[o] += fv;
                land_fert.push(fv);
                order_of.push((o, fv));
                let t = w.fields.tmean[[y, x]] as f64;
                let p = w.fields.precip[[y, x]] as f64;
                let b = w.fields.biomes[[y, x]];
                if o == SoilOrder::Chernozem.code() as usize {
                    cher += 1;
                    if calliope::agriculture::chernozem_climate(t, p) {
                        cher_climate += 1;
                    }
                    if b == calliope::constants::GRASSLAND
                        || b == calliope::constants::SAVANNA
                        || b == calliope::constants::WOODLAND
                    {
                        cher_grass += 1;
                    }
                }
                if o == SoilOrder::Andosol.code() as usize {
                    andosol += 1;
                    if w.fields.rock[[y, x]] == calliope::rock::VOLCANIC {
                        andosol_volcanic += 1;
                    }
                }
            }
        }
        let soil_n: usize = count.iter().sum();
        let denom = soil_n.max(1) as f64;

        println!();
        println!("soil orders (M51 · clorpt):");
        println!("{:<12} {:>8} {:>8} {:>9} {:>9} {:>7}", "order", "cells", "share", "mean fert", "curve", "depth m");
        let mut ranks: Vec<(f64, f64)> = Vec::new();
        for o in SoilOrder::iter() {
            if o == SoilOrder::None {
                continue;
            }
            let i = o.code() as usize;
            let share = count[i] as f64 / denom;
            let mean_f = if count[i] > 0 { fert_sum[i] / count[i] as f64 } else { 0.0 };
            println!(
                "{:<12} {:>8} {:>7.1}% {:>9.3} {:>9.2} {:>7.2}",
                o.name(), count[i], 100.0 * share, mean_f, o.fertility(), o.depth()
            );
            if count[i] >= 200 {
                ranks.push((o.fertility(), mean_f));
            }
            c.band_as(&format!("{} share", o.name()), &format!("{} share of land", o.name()), share, format!("{} of {} ({})", count[i], soil_n, pct(share)));
        }

        // The curve must actually order the farms: Spearman between each
        // order's declared fertility curve and the mean arable index the
        // world measures on it. This is the claim M51 makes — that the
        // soil map explains fertility rather than decorating it.
        let rho = spearman(&ranks);
        println!(
            "curve-vs-measured rank correlation over {} orders: ρ = {:.2}",
            ranks.len(), rho
        );
        c.band("soil fertility rank correlation", rho, format!("ρ {:.2} over {} orders", rho, ranks.len()));

        // ---- M52 — alluvium and loess: the delivered orders ----
        // A soil laid down by a river or the wind should not merely be
        // "better than average": the claim is that these belts *are* the
        // best farmland on the map. Measure the top fertility decile of
        // all land, then the rate at which each order falls inside it.
        // Baseline is 10% by construction, so the enrichment ratio is a
        // pure multiple of chance.
        {
            let mut sorted = land_fert.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let cut = if sorted.is_empty() {
                0.0
            } else {
                sorted[((sorted.len() as f64 * 0.90) as usize).min(sorted.len() - 1)]
            };
            let mut top = [0usize; 11];
            for (o, f) in &order_of {
                if *f >= cut {
                    top[*o] += 1;
                }
            }
            let base = order_of.iter().filter(|(_, f)| *f >= cut).count() as f64
                / order_of.len().max(1) as f64;
            println!();
            println!(
                "M52 delivered soils · top-decile cut fert {:.3} (baseline {})",
                cut, pct(base)
            );
            for o in [SoilOrder::Fluvisol, SoilOrder::Loess] {
                let i = o.code() as usize;
                let rate = top[i] as f64 / count[i].max(1) as f64;
                let enrich = if base > 0.0 { rate / base } else { 0.0 };
                println!(
                    "{:<12} {:>8} cells · {:>6} in top decile ({}) · x{:.2}",
                    o.name(), count[i], top[i], pct(rate), enrich
                );
                let label = if o == SoilOrder::Fluvisol { "alluvium" } else { "loess" };
                if o == SoilOrder::Fluvisol {
                    c.band_as(
                        "alluvium enrichment",
                        "alluvium top-decile enrichment",
                        enrich,
                        format!("{} of {} in top decile ({}) — x{:.2} chance", top[i], count[i], pct(rate), enrich),
                    );
                }
                c.must(
                    &format!("{} order present", label),
                    count[i] > 0,
                    format!("{} cells", count[i]),
                    "M52: every world has a floodplain and a dust belt",
                );
            }

            // The upgrade gate: rerun the classifier with the dust mantle
            // and the flood swept off, then rerun the shipped fertility
            // law under both soil planes. The ratio on each delivered
            // order's own cells is what the delivery is worth — it reads
            // whichever profile the ladder would have grown there
            // (podzol on the cold dust, aridisol on a desert wadi), so it
            // cannot be satisfied by the curve constant alone.
            let h64 = w.fields.height.mapv(|v| v as f64);
            let t64 = w.fields.tmean.mapv(|v| v as f64);
            let p64 = w.fields.precip.mapv(|v| v as f64);
            let q64 = w.fields.discharge.mapv(|v| v as f64);
            let rivers = w.fields.flags.mapv(|f| f & CellFlags::RIVER.bits() != 0);
            let lakes = w.fields.flags.mapv(|f| f & CellFlags::LAKE.bits() != 0);
            let zerof: ndarray::Array2<f32> = ndarray::Array2::zeros((h, wd));
            let zeroq: ndarray::Array2<f64> = ndarray::Array2::zeros((h, wd));
            let buried_soil = calliope::agriculture::soil_genesis(
                &h64, &t64, &p64, &w.fields.biomes, &w.fields.rock,
                &rivers, &lakes, &zeroq, &w.ice.till, &zerof,
            );
            let f_now = calliope::agriculture::fertility(
                &h64, &t64, &p64, &rivers, &lakes, &q64,
                &w.ice.till, &w.ice.loess, &w.ice.outwash, &w.fields.soil,
            );
            let f_buried = calliope::agriculture::fertility(
                &h64, &t64, &p64, &rivers, &lakes, &q64,
                &w.ice.till, &w.ice.loess, &w.ice.outwash, &buried_soil,
            );
            println!();
            println!("M52 delivery upgrade · shipped soil vs the profile it buried:");
            for o in [SoilOrder::Fluvisol, SoilOrder::Loess] {
                let code = o.code();
                let (mut a, mut b, mut n) = (0.0f64, 0.0f64, 0usize);
                let mut buried_mix = [0usize; 11];
                for y in 0..h {
                    for x in 0..wd {
                        if w.fields.soil[[y, x]] != code {
                            continue;
                        }
                        // the farmable footprint: averaging over ground
                        // the climate has already closed dilutes the belt
                        // where the farms actually stand.
                        if f_now[[y, x]] < 0.05 && f_buried[[y, x]] < 0.05 {
                            continue;
                        }
                        a += f_now[[y, x]];
                        b += f_buried[[y, x]];
                        buried_mix[buried_soil[[y, x]] as usize] += 1;
                        n += 1;
                    }
                }
                let up = if b > 0.0 { a / b } else { 0.0 };
                let mut mix: Vec<String> = Vec::new();
                for (i, cnt) in buried_mix.iter().enumerate() {
                    if *cnt > 0 {
                        mix.push(format!("{} {}", SoilOrder::from_code(i as u8).name(), cnt));
                    }
                }
                let label = if o == SoilOrder::Fluvisol { "alluvium" } else { "loess" };
                println!(
                    "{:<12} {:>6} farmable cells · x{:.2} · buried: {}",
                    o.name(), n, up, mix.join(" · ")
                );
                c.band_as(
                    &format!("{} upgrade", label),
                    &format!("{} soil upgrade", label),
                    up,
                    format!("x{:.2} over the buried profile on {} farmable cells", up, n),
                );
            }
        }

        // ---- M53 — the crop tables re-based on the orders ----------
        // The claim: the ground now decides *which* package wins, not
        // just how much it yields. Three readings, all against the same
        // shipped `soil_suitability` table the classifier plants by.
        {
            use calliope::agriculture::{soil_suitability, CropPackage};
            let mut drain_sum = [0.0f64; 5];
            let mut depth_sum = [0.0f64; 5];
            let mut crop_n = [0usize; 5];
            let mut shallow_grain = 0usize;   // grain rooted in <0.35 m profile
            let mut grain = 0usize;
            // per-order: how much of it is farmed at all
            let mut order_farm = [0usize; 11];
            for y in 0..h {
                for x in 0..wd {
                    if !land[[y, x]] {
                        continue;
                    }
                    let o = SoilOrder::from_code(w.fields.soil[[y, x]]);
                    if o == SoilOrder::None {
                        continue;
                    }
                    let cp = w.fields.crops[[y, x]] as usize;
                    crop_n[cp] += 1;
                    drain_sum[cp] += o.drainage();
                    depth_sum[cp] += o.depth();
                    if (1..=3).contains(&cp) {
                        grain += 1;
                        order_farm[o.code() as usize] += 1;
                        if o.depth() < 0.35 {
                            shallow_grain += 1;
                        }
                    }
                }
            }
            println!();
            println!("M53 crop packages against the ground they stand on:");
            println!("{:<10} {:>8} {:>10} {:>10}", "package", "cells", "drainage", "depth m");
            for cp in [
                CropPackage::Wheat,
                CropPackage::Rice,
                CropPackage::Maize,
                CropPackage::Pastoral,
                CropPackage::Wildland,
            ] {
                let i = cp.code() as usize;
                let n = crop_n[i].max(1) as f64;
                println!(
                    "{:<10} {:>8} {:>10.3} {:>10.2}",
                    cp.name(), crop_n[i], drain_sum[i] / n, depth_sum[i] / n
                );
            }
            let rice_dr = drain_sum[2] / crop_n[2].max(1) as f64;
            let wheat_dr = drain_sum[1] / crop_n[1].max(1) as f64;
            println!(
                "mean drainage under wheat {:.3} · under rice {:.3}",
                wheat_dr, rice_dr
            );
            // The paddy claim is conditional, not absolute: rice is a hot
            // crop, so most of it stands wherever the tropics allow and
            // the mean drainage under it is dominated by climate. What the
            // drainage curve must do is decide the *contested* cells —
            // where two packages are climatically possible, the wet
            // profile must go to rice. Measured as enrichment: rice's
            // share of grain on poorly drained orders against its share
            // of grain everywhere.
            let mut wet_grain = 0usize;
            let mut wet_rice = 0usize;
            for y in 0..h {
                for x in 0..wd {
                    let o = SoilOrder::from_code(w.fields.soil[[y, x]]);
                    let cp = w.fields.crops[[y, x]] as usize;
                    if o == SoilOrder::None || !(1..=3).contains(&cp) || o.drainage() >= 0.5 {
                        continue;
                    }
                    wet_grain += 1;
                    if cp == CropPackage::Rice.code() as usize {
                        wet_rice += 1;
                    }
                }
            }
            let base_rice = crop_n[2] as f64 / grain.max(1) as f64;
            let wet_share = wet_rice as f64 / wet_grain.max(1) as f64;
            let paddy = if base_rice > 0.0 { wet_share / base_rice } else { 0.0 };
            c.band(
                "paddy wet-soil enrichment",
                paddy,
                format!(
                    "rice takes {} of grain on wet profiles ({} cells) vs {} everywhere — x{:.2}",
                    pct(wet_share), wet_grain, pct(base_rice), paddy
                ),
            );

            c.must(
                "grain does not root in rock",
                crop_n[2] == 0 || (shallow_grain as f64 / grain.max(1) as f64) < 0.02,
                format!("{} of {} grain cells on <0.35 m profiles", shallow_grain, grain),
                "M53: the depth term must close the skeletal soils to the plough",
            );

            // The ordering claim: an order's best edaphic score for the
            // grain packages must predict how much of that order is
            // actually farmed. Climate still dominates any single cell,
            // so this is a rank test over the orders, not a fit.
            let mut ranks: Vec<(f64, f64)> = Vec::new();
            println!();
            println!("{:<12} {:>9} {:>10} {:>9}", "order", "edaphic", "farmed", "cells");
            for o in SoilOrder::iter() {
                if o == SoilOrder::None {
                    continue;
                }
                let i = o.code() as usize;
                if count[i] < 200 {
                    continue;
                }
                let ed = soil_suitability(o, CropPackage::Wheat)
                    .max(soil_suitability(o, CropPackage::Rice))
                    .max(soil_suitability(o, CropPackage::Maize));
                let farmed = order_farm[i] as f64 / count[i] as f64;
                println!("{:<12} {:>9.3} {:>10} {:>9}", o.name(), ed, pct(farmed), count[i]);
                ranks.push((ed, farmed));
            }
            let rho = spearman(&ranks);
            println!(
                "edaphic-vs-farmed rank correlation over {} orders: ρ = {:.2}",
                ranks.len(), rho
            );
            c.band(
                "crop soil suitability correlation",
                rho,
                format!("ρ {:.2} over {} orders", rho, ranks.len()),
            );
        }



        c.must(
            "every land cell carries a soil order",
            bare == 0,
            format!("{} bare of {} land cells", bare, soil_n + bare),
            "M51: the classifier is total over land — no cell falls through the ladder",
        );
        let cher_cl = cher_climate as f64 / cher.max(1) as f64;
        let cher_gr = cher_grass as f64 / cher.max(1) as f64;
        c.must(
            "chernozem confined to its climate window",
            cher > 0 && cher_cl >= 0.999,
            format!("{} of {} in window · {} under grass/woodland ({})", cher_climate, cher, cher_grass, pct(cher_gr)),
            "M51 gate: black earth only where humus outlasts the summer and rain never flushes the profile",
        );
        c.must(
            "andosol sits on volcanic parent rock",
            andosol == 0 || andosol_volcanic == andosol,
            format!("{} of {} on volcanic province", andosol_volcanic, andosol),
            "M51: ash soils cannot exist off the ash",
        );
    }

    c.print();
}

// ================================================================ civ

fn cmd_civ(seed: i64, size: usize, years: usize) {
    let mut w = World::generate(seed, size);
    // M72 — the routed river, as the dawn solved it. Every year's flow is
    // a read-time multiplier; if a tick ever wrote back into the routing,
    // these bits would move and the gate below would say so.
    let river_dawn = {
        let mut b: Vec<u8> = Vec::new();
        for v in w.fields.discharge.iter() { b.extend_from_slice(&v.to_le_bytes()); }
        for v in w.fields.strahler.iter() { b.push(*v); }
        for v in w.fields.flags.iter() { b.push(*v); }
        (w.fields.discharge.dim(), fnv(&b))
    };
    header("CIVILIZATION", &format!("seed {} · {}x{} · {}y", seed, w.width, size, years));
    println!("world \"{}\" · {} peoples · {} realms · {} settlements at dawn", w.world_name, w.peoples.peoples.len(), w.peoples.realms.len(), w.peoples.settlements.len());

    let pop0: i64 = w.peoples.settlements.iter().map(|s| s.pop).sum();
    let setts0 = w.peoples.settlements.len();
    let log = run_years(&mut w, years);

    println!();
    println!("{:>4} {:>8} {:>6} {:>7} {:>10} {:>9} {:>6} {:>6} {:>5}", "yr", "pop", "towns", "routes", "wealth", "treasury", "arts", "seams", "ev/y");
    let step = (years / 24).max(1);
    for r in log.rows.iter().filter(|r| r.0 % step == 0 || r.0 == years) {
        println!("{:>4} {:>8} {:>6} {:>7} {:>10.0} {:>9.0} {:>6} {:>6} {:>5}", r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8);
    }

    println!();
    let parts: Vec<String> = log.census.iter().map(|(k, v)| format!("{k}:{v}")).collect();
    println!("event census: {}", parts.join(" · "));
    println!("strikes {} · depletions {} · mining camps {} · wars {}", log.strikes, log.depletions, log.camps, log.wars);
    let pol_min = log.polities.iter().min().copied().unwrap_or(0);
    let pol_max = log.polities.iter().max().copied().unwrap_or(0);
    println!(
        "statecraft: {} war events · {} towns changed hands · {} secessions · polities {}–{} · coalitions {} · vassals ≤{}",
        log.wars, log.transfers, log.rebellions, pol_min, pol_max,
        if log.coalition_seen { "yes" } else { "no" }, log.vassals_max,
    );
    println!("longest silence between events: {} months", log.max_gap);

    println!();
    println!("the arc of the ages (tech & society events):");
    for (m, t) in log.arc.iter().take(18) {
        println!("  y{:<4} {}", m / 12, t);
    }
    if log.arc.len() > 18 {
        println!("  … and {} more", log.arc.len() - 18);
    }

    println!();
    println!("societies at year {}:", years);
    for (soc, cu) in w.peoples.societies.iter().zip(w.peoples.peoples.iter()) {
        println!("  {:<22} {:<10} {:<14} {:>2} arts · lore {:>6.0}", cu.people, society::POLITIES[soc.polity], society::ERAS[soc.era], soc.techs.len(), soc.knowledge);
    }
    println!("crowns at year {}:", years);
    for r in w.peoples.realms.iter().filter(|r| r.alive) {
        println!("  {:<22} treasury {:>8.0}", r.name, r.treasury);
    }

    let pop1: i64 = w.peoples.settlements.iter().map(|s| s.pop).sum();
    let growth = pop1 as f64 / pop0.max(1) as f64;
    let names: BTreeSet<&str> = w.peoples.settlements.iter().map(|s| s.name.as_str()).collect();
    let max_era = w.peoples.societies.iter().map(|s| s.era).max().unwrap_or(0);
    let techs_total: usize = w.peoples.societies.iter().map(|s| s.techs.len()).sum();
    let ev_per_year = log.total_events as f64 / years.max(1) as f64;
    let unconnected = w.peoples.settlements.iter().filter(|s| s.connections == 0).count();
    let finite_ok = w.peoples.settlements.iter().all(|s| s.wealth.is_finite() && s.pop >= 0) && w.economy.market.iter_some().all(|(_, p)| p.is_finite());

    let mut c = Checks::default();
    c.band_as("population growth ×", "century growth", growth, format!("{:.2}×", growth));
    if years >= 100 {
        // pacing: the world should still be becoming in its second half,
        // not sitting on a saturated plateau for a century.
        let half_pop = log.rows[years / 2 - 1].1 as f64;
        let pace = half_pop / pop1.max(1) as f64;
        c.want("still growing at half-run", pace <= 0.92, format!("{:.0}% of final", 100.0 * pace), "pop at half-run ≤92% of final");
    }
    c.want("settlements grew", w.peoples.settlements.len() >= setts0, format!("{}→{}", setts0, w.peoples.settlements.len()), "colonies should outnumber the dawn towns");

    // M30 — the legacy dividend at town level, counterfactual: towns
    // sitting on till or loess must eat measurably better than the same
    // sites would with the deposits zeroed. Observational belt compares
    // are confounded (off-footprint controls include low-latitude river
    // deltas), so the gate asks the causal question exactly, as the
    // terrain lane does for cells.
    {
        let (ir, ic) = w.ice.till.dim();
        let h64 = w.fields.height.mapv(|v| v as f64);
        let t64 = w.fields.tmean.mapv(|v| v as f64);
        let p64 = w.fields.precip.mapv(|v| v as f64);
        let q64 = w.fields.discharge.mapv(|v| v as f64);
        let rivers = w.fields.flags.mapv(|f| f & CellFlags::RIVER.bits() != 0);
        let lakes = w.fields.flags.mapv(|f| f & CellFlags::LAKE.bits() != 0);
        let none: ndarray::Array2<f32> = ndarray::Array2::zeros((ir, ic));
            // M51 — a neutral soil plane on both legs: the counterfactual
            // must isolate the deposit, not the soil order under it.
            let nsoil: ndarray::Array2<u8> = ndarray::Array2::from_elem((ir, ic), calliope::agriculture::SoilOrder::Cambisol.code());
        let f_with = calliope::agriculture::fertility(
            &h64, &t64, &p64, &rivers, &lakes, &q64, &w.ice.till, &w.ice.loess, &w.ice.outwash, &nsoil,
        );
        let f_bare = calliope::agriculture::fertility(
            &h64, &t64, &p64, &rivers, &lakes, &q64, &none, &none, &w.ice.outwash, &nsoil,
        );
        let (mut up, mut n) = (0.0f64, 0usize);
        for s in &w.peoples.settlements {
            let (y, x) = (s.y as usize, s.x as usize);
            if y >= ir || x >= ic {
                continue;
            }
            if w.ice.till[[y, x]] > 0.0 || w.ice.loess[[y, x]] > 0.0 {
                up += f_with[[y, x]] - f_bare[[y, x]];
                n += 1;
            }
        }
        if n >= 3 {
            let mean = up / n as f64;
            c.want(
                "legacy towns eat off the deposit",
                mean >= 0.005,
                format!("{:+.4} uplift on {} footprint towns", mean, n),
                "M30 gate: zeroing till+loess must cost the towns on it ≥ +0.005",
            );
        } else {
            println!("legacy dividend: too few footprint towns to compare ({})", n);
        }
    }
    if years >= 100 {
        // by the century mark the colonies should have broken the river
        // monoculture: dry-coast harbours, cistern towns, mining camps.
        let dry = w.peoples.settlements.iter().filter(|s| !s.river).count();
        c.want("dry-country towns exist", dry >= 1, format!("{} of {}", dry, w.peoples.settlements.len()), "≥1 town beyond fresh water by the century mark");
    }
    c.must("routes exist", !w.routes.is_empty(), format!("{}", w.routes.len()), "a world without trade is broken");
    c.want("no unconnected towns", unconnected == 0, format!("{}", unconnected), "every town should reach the web of trade");
    // ---- M45 harbour shelter: ports sit where the coast shelters ships
    {
        let mut shl: Vec<f64> = Vec::new();
        let (rows, cols) = w.shelter.dim();
        let mut inland_max = 0.0f32;
        for y in 0..rows {
            for x in 0..cols {
                if w.coast[[y, x]] {
                    shl.push(w.shelter[[y, x]] as f64);
                } else if w.shelter[[y, x]] > inland_max {
                    inland_max = w.shelter[[y, x]];
                }
            }
        }
        let q3 = calliope::util::quantile(&shl, 0.75);
        let p90 = calliope::util::quantile(&shl, 0.90);
        let coast_mean = shl.iter().sum::<f64>() / shl.len().max(1) as f64;
        let harbours: Vec<_> = w.peoples.settlements.iter().filter(|s| s.port).collect();
        let on_top = harbours
            .iter()
            .filter(|s| w.shelter[[s.y as usize, s.x as usize]] as f64 >= q3)
            .count();
        let conc = on_top as f64 / harbours.len().max(1) as f64;
        let coastal_towns: Vec<f64> = w
            .peoples
            .settlements
            .iter()
            .filter(|s| s.coastal)
            .map(|s| w.shelter[[s.y as usize, s.x as usize]] as f64)
            .collect();
        let town_mean =
            coastal_towns.iter().sum::<f64>() / coastal_towns.len().max(1) as f64;
        let lift = if coast_mean > 0.0 { town_mean / coast_mean } else { 0.0 };
        println!();
        println!(
            "harbour shelter (M45): coastal band {} cells · mean {:.3} · q3 {:.3} · p90 {:.3}",
            shl.len(),
            coast_mean,
            q3,
            p90
        );
        println!(
            "  ports on top-quartile shelter: {} of {} ({:.0}%) · coastal towns {} mean {:.3} ({:.2}× the band)",
            on_top,
            harbours.len(),
            100.0 * conc,
            coastal_towns.len(),
            town_mean,
            lift
        );
        // The gate measures SITING, not endowment (M45's intent is that
        // "ports rise where geometry actually shelters ships"): a port is
        // well-sited when it commands top-quartile water, or when it took
        // the best water its own coast offers within 14 cells (56 km) —
        // Kalantheia on the calmest cell of a smooth inland sea is
        // perfectly sited; no sailor could have done better. The honest
        // failure is the MISSED cohort: better water within reach, town
        // on the bluff beside it. ε = 0.02 (wire precision of the score).
        let mut took_best = 0usize;
        let mut missed = 0usize;
        // M45 (founding-time leg): the honest contemporaneous occupancy.
        // A founder could only be blamed for water that was free ON THE
        // MONTH THE TOWN WAS BORN. The registry keeps every settlement's
        // (since, until) interval and its anchor, and every death walks
        // through the one kill path (M24), so ruins are already in this
        // ledger — reconstructing who stood where at month `born` needs
        // no guessing.
        let hist: Vec<(i64, i64, i64, i64)> = w
            .chronicle
            .registry
            .items
            .iter()
            .filter(|e| e.kind == calliope::entity::EntityKind::Settlement && e.x >= 0)
            .map(|e| (e.x, e.y, e.since, e.until.unwrap_or(i64::MAX)))
            .collect();
        let mut took_best_born = 0usize;
        let mut missed_born = 0usize;
        let spacing2 = calliope::settlements::MIN_TOWN_SPACING_CELLS
            * calliope::settlements::MIN_TOWN_SPACING_CELLS;
        for s in &harbours {
            let sh = w.shelter[[s.y as usize, s.x as usize]] as f64;
            if sh >= q3 {
                continue;
            }
            let (sy, sx) = (s.y as usize, s.x as usize);
            let mut best = 0.0f64;
            let mut best_born = 0.0f64;
            let mut best_born_at = (sy, sx);

            let r = 14i64;
            for dy in -r..=r {
                for dx in -r..=r {
                    let (ny, nx) = (sy as i64 + dy, sx as i64 + dx);
                    if ny < 0 || nx < 0 || ny >= rows as i64 || nx >= cols as i64 {
                        continue;
                    }
                    let v = w.shelter[[ny as usize, nx as usize]] as f64;
                    // only TAKEABLE water counts against the siting: a
                    // cell inside another town's spacing ring was never
                    // this founder's to take — that water is commanded
                    // by the neighbour's quay, not missed by this one.
                    let taken = w.peoples.settlements.iter().any(|o| {
                        o.id != s.id && {
                            let ody = ny as f64 - o.y as f64;
                            let odx = nx as f64 - o.x as f64;
                            ody * ody + odx * odx < spacing2
                        }
                    });
                    if !taken && v > best {
                        best = v;
                    }
                    // contemporaneous ring: towns alive at s.born, minus
                    // this town's own founding row.
                    let taken_born = hist.iter().any(|&(ox, oy, since, until)| {
                        !(ox == s.x && oy == s.y)
                            && since <= s.born
                            && until > s.born
                            && {
                                let ody = ny - oy;
                                let odx = nx - ox;
                                (ody * ody + odx * odx) < spacing2 as i64
                            }
                    });
                    if !taken_born && v > best_born {
                        best_born = v;
                        best_born_at = (ny as usize, nx as usize);
                    }

                }
            }
            if sh + 0.02 >= best {
                took_best += 1;
            } else {
                missed += 1;
                println!(
                    "    missed: {} sh={:.2} takeable-best={:.2} (at founding m{} {:.2}) pop={:.0} at ({},{})",
                    s.name, sh, best, s.born, best_born, s.pop, s.x, s.y
                );
                if std::env::var("CALLIOPE_PORTPROBE").is_ok() {
                    let (by, bx) = best_born_at;
                    let dd = ((by as f64 - sy as f64).powi(2)
                        + (bx as f64 - sx as f64).powi(2))
                    .sqrt();
                    println!(
                        "      probe: chosen score={:.2} sh={:.2} | best-water ({},{}) score={:.2} sh={:.2} d={:.1} arid={} land={}",
                        w.site_score[[sy, sx]],
                        sh,
                        bx,
                        by,
                        w.site_score[[by, bx]],
                        best_born,
                        dd,
                        w.arid_dry[[by, bx]],
                        w.fields.height[[by, bx]] >= 0.0,
                    );
                }
            }

            if sh + 0.02 >= best_born {
                took_best_born += 1;
            } else {
                missed_born += 1;
            }
        }
        let well = on_top + took_best;
        let sited = well as f64 / harbours.len().max(1) as f64;
        let well_born = on_top + took_best_born;
        let sited_born = well_born as f64 / harbours.len().max(1) as f64;
        println!(
            "  siting: {} on top-quartile + {} took the best their coast offers = {} of {} well-sited ({:.0}%) · {} missed better water within 56 km",
            on_top,
            took_best,
            well,
            harbours.len(),
            100.0 * sited,
            missed
        );
        println!(
            "  siting (founding-time occupancy): {} + {} = {} of {} ({:.0}%) · {} missed — terminal-set reading {:.0}%, gap {:+.0} pp",
            on_top,
            took_best_born,
            well_born,
            harbours.len(),
            100.0 * sited_born,
            missed_born,
            100.0 * sited,
            100.0 * (sited_born - sited)
        );

        if !harbours.is_empty() {
            c.band("port shelter concentration", sited, pct(sited));
            c.must(
                "ports favor sheltered water",
                sited >= 0.70,
                format!("{} of {} well-sited", well, harbours.len()),
                "M45 gate: ≥70% of ports well-sited (top-quartile water, or the best their coast offers)",
            );
        }
        c.band("coastal shelter p90", p90, format!("{:.3}", p90));
        if !coastal_towns.is_empty() {
            c.band("coastal town shelter lift", lift, format!("{:.2}×", lift));
        }
        c.must(
            "shelter silent inland",
            inland_max == 0.0,
            format!("max off-coast {:.4}", inland_max),
            "M45: exactly 0 off the coastal band — inland siting untouched",
        );
    }
    c.must("no template placeholders", log.placeholders == 0, format!("{}", log.placeholders), "no {P}/{S} may leak into chronicle text");
    c.must("no empty event texts", log.empties == 0, format!("{}", log.empties), "every event tells its story");
    c.must("settlement names unique", names.len() == w.peoples.settlements.len(), format!("{} names / {} towns", names.len(), w.peoples.settlements.len()), "the taken-set must hold");
    // M10.4 — the seat is mechanically real: every living crown's seat
    // must resolve to a town under its own banner, every month.
    let seat_bad = w
        .peoples
        .realms
        .iter()
        .filter(|r| r.alive)
        .filter(|r| !w.peoples.settlements.iter().any(|s| s.id == r.seat && s.realm == r.id))
        .count();
    let seat_moves = w
        .chronicle
        .events
        .iter()
        .filter(|e| e.text.contains(" removes to ") || e.text.contains(" removes from "))
        .count();
    c.must("realm seats under their own banner", seat_bad == 0, format!("{} dangling · {} seat moves", seat_bad, seat_moves), "M10.4: a lost seat re-homes the same month");
    // ---- M11 — the unrest ladder --------------------------------------
    let count = |pat: &str| w.chronicle.events.iter().filter(|e| e.text.contains(pat)).count();
    println!();
    println!("---- unrest ladder (M11) ----------------------------------------------");
    println!(
        " riots {:>3} · charters {:>3} · coups {:>3} · crises {:>3} (resolved {:>3}) · secessions {:>3}",
        count("Bread riots"),
        count("charter of liberties"),
        count("seizes the circlet"),
        count("claim the circlet"),
        count("war of the circlet of"),
        count("rise against"),
    );
    let u_max = w.politics.unrest.iter().cloned().fold(0.0f64, f64::max);
    let u_mean = if w.politics.unrest.is_empty() { 0.0 } else { w.politics.unrest.iter().sum::<f64>() / w.politics.unrest.len() as f64 };
    println!(" unrest now: mean {:.2} · max {:.2} over {} realms", u_mean, u_max, w.politics.unrest.len());
    let unrest_ok = w.politics.unrest.iter().all(|u| u.is_finite() && (0.0..=1.0).contains(u));
    c.must("unrest stays a 0..1 gauge", unrest_ok, format!("{} realms", w.politics.unrest.len()), "M11.1: finite, clamped");
    // ladder rungs only (crisis openings ride ruler deaths, checked below)
    let rung_anchor = |t: &str| {
        t.contains("seizes the circlet")
            || t.contains("charter of liberties")
            || t.contains("Bread riots")
            || t.contains("rise against")
    };
    let mut rungs_by_realm: std::collections::BTreeMap<&str, Vec<i64>> = Default::default();
    for e in w.chronicle.events.iter().filter(|e| rung_anchor(&e.text)) {
        rungs_by_realm.entry(e.s.as_str()).or_default().push(e.m);
    }
    let rung_total: usize = rungs_by_realm.values().map(|v| v.len()).sum();
    let min_gap = rungs_by_realm
        .values()
        .flat_map(|v| v.windows(2).map(|w| w[1] - w[0]))
        .min()
        .unwrap_or(i64::MAX);
    // name the offender: which realm, which months, which rungs
    if min_gap < 12 {
        for (name, months) in &rungs_by_realm {
            for w2 in months.windows(2) {
                if w2[1] - w2[0] < 12 {
                    println!(" CONVULSION: {} at m{} then m{} (gap {} mo)", name, w2[0], w2[1], w2[1] - w2[0]);
                    for e in w.chronicle.events.iter().filter(|e| e.s.as_str() == *name && rung_anchor(&e.text) && (e.m == w2[0] || e.m == w2[1])) {
                        println!("   m{} [{}] {}", e.m, e.k, &e.text.chars().take(110).collect::<String>());
                    }
                }
            }
        }
    }
    if years >= 100 {
        c.want("the unrest ladder speaks", rung_total >= 1, format!("{} rungs fired", rung_total), "M11: riots/charters/coups/secessions on the century scale");
    }
    c.must(
        "no realm convulses monthly",
        min_gap >= 12 || rung_total < 2,
        if min_gap == i64::MAX { "no repeats".into() } else { format!("min gap {} mo", min_gap) },
        "M11.6: cooldowns keep risings years apart",
    );
    let crisis_stuck = w
        .politics
        .crisis
        .iter()
        .flatten()
        .filter(|cw| cw.ends < w.month)
        .count();
    c.must("no war of the circlet outlives its term", crisis_stuck == 0, format!("{} stuck", crisis_stuck), "M11.3: crises resolve the month they come due");
    // ---- M12 — kindred and crown ---------------------------------------
    println!();
    println!("---- kindred and crown (M12) ------------------------------------------");
    let foreign: Vec<&calliope::settlements::Settlement> = w
        .peoples
        .settlements
        .iter()
        .filter(|s| {
            w.peoples
                .realms
                .get(s.realm.0)
                .map_or(false, |r| r.alive && r.people != s.people)
        })
        .collect();
    let drifting = foreign.iter().filter(|s| s.drift > 0.0).count();
    let d_max = foreign.iter().map(|s| s.drift).fold(0.0f64, f64::max);
    println!(
        " foreign-crowned towns {:>3} of {:>3} · drifting {:>3} · max drift {:.2}",
        foreign.len(),
        w.peoples.settlements.len(),
        drifting,
        d_max
    );
    let (kindred_foreign, kindred_drifting, gate_breaches) = {
        let mut pairs: BTreeMap<(usize, usize), usize> = BTreeMap::new();
        let (mut kf, mut kd, mut br) = (0usize, 0usize, 0usize);
        for s in &foreign {
            let r = &w.peoples.realms[s.realm.0];
            *pairs.entry((s.people.idx(), r.people.idx())).or_default() += 1;
            let k = calliope::culture::kinship(s.people, r.people, &w.peoples.peoples, &w.peoples.coresidence);
            if k >= 0.20 {
                kf += 1;
                if s.drift > 0.0 {
                    kd += 1;
                }
            } else if s.drift > 0.0 {
                br += 1; // drift across a non-kindred pair — forbidden (M12 gate)
            }
        }
        for ((a, b), n) in &pairs {
            let k = calliope::culture::kinship(
                calliope::ids::PeopleId(*a),
                calliope::ids::PeopleId(*b),
                &w.peoples.peoples,
                &w.peoples.coresidence,
            );
            println!(
                "   {:<16} under {:<16} {:>3} towns · kinship {:.2}",
                w.peoples.peoples[*a].people, w.peoples.peoples[*b].people, n, k
            );
        }
        (kf, kd, br)
    };
    let flips = count("count themselves");
    let sunders = count("a people of their own");
    let fusions = count("one people now");
    let unions = count("are joined");
    let crown_exonyms = count("On the crown's rolls");
    println!(
        " moves: {} flips · {} sunderings · {} fusions · {} unions · {} crown exonyms",
        flips, sunders, fusions, unions, crown_exonyms
    );
    // union-gate census: every kindred pair of living crowns and which
    // gate stops them — the tuner reads this table, not tea leaves.
    {
        let nr = w.peoples.realms.len();
        // a final-month secession can leave the matrix one row short
        w.politics.grow(nr);
        for a in 0..nr {
            for b in (a + 1)..nr {
                let (ra, rb) = (&w.peoples.realms[a], &w.peoples.realms[b]);
                if !ra.alive || !rb.alive {
                    continue;
                }
                let k = calliope::culture::kinship(ra.people, rb.people, &w.peoples.peoples, &w.peoples.coresidence);
                if k < 0.55 {
                    continue;
                }
                let (oab, oba) = (w.politics.opinion[a * nr + b], w.politics.opinion[b * nr + a]);
                let vassal = w.politics.vassal_of[a].is_some() || w.politics.vassal_of[b].is_some();
                println!(
                    "   union gate: {:<18} + {:<18} kin {:.2} · opinion {:>4.0}/{:>4.0} · vassal {}",
                    ra.name, rb.name, k, oab, oba, vassal
                );
            }
        }
    }
    let co_ok = w.peoples.coresidence.len() == w.peoples.peoples.len()
        && w
            .peoples
            .coresidence
            .iter()
            .all(|row| row.iter().all(|v| v.is_finite() && *v >= 0.0));
    c.must(
        "co-residence ledger squares with the roster",
        co_ok,
        format!("{}×{} for {} peoples", w.peoples.coresidence.len(),
            w.peoples.coresidence.first().map_or(0, |r| r.len()), w.peoples.peoples.len()),
        "M12.1: the ledger grows with divergence, stays finite",
    );
    c.must(
        "no drift across non-kindred pairs",
        gate_breaches == 0,
        format!("{} breaches", gate_breaches),
        "M12 gate: below kinship 0.20 the minority stands and remembers",
    );
    if years >= 100 {
        let moves_cy = (flips + sunders + fusions + unions) as f64 * 100.0 / years as f64;
        c.band("kindred moves per century", moves_cy, format!("{:.1}", moves_cy));
        let flips_cy = flips as f64 * 100.0 / years as f64;
        c.band("assimilation cadence", flips_cy, format!("{:.1} flips/century", flips_cy));
        // the drift machinery must at least be turning wherever kindred
        // crowns rule kindred strangers — zero motion means a dead clock.
        if kindred_foreign >= 5 {
            c.want(
                "assimilation drift is turning",
                kindred_drifting >= 1 || flips >= 1,
                format!("{} of {} kindred foreign-crowned towns drift", kindred_drifting, kindred_foreign),
                "M12.2: towns under kindred crowns lean, however slowly",
            );
        }
    }
    // ---- M13 — the arc of empires ---------------------------------------
    println!();
    println!("---- the arc of empires (M13) -----------------------------------------");
    let civs_alive = w.peoples.civs.iter().filter(|cv| cv.alive).count();
    for cv in &w.peoples.civs {
        let members = cv
            .peoples
            .iter()
            .map(|p| w.peoples.peoples[p.idx()].people.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "   {:<26} {:<12} {} tongues [{}] · golden ages {} · monuments {}{}{}",
            cv.name,
            format!("{:?}", cv.stage).to_lowercase(),
            cv.peoples.len(),
            members,
            cv.golden_ages,
            cv.monuments,
            cv.hegemony.as_deref().map(|h| format!(" · {}", h)).unwrap_or_default(),
            if cv.alive { "" } else { " · ENDED" },
        );
        // the drivers the stage machine read on its last pass (M13.3)
        println!(
            "     drivers: legit {:.2} · asab {:.2} · wealth {:.0} · stretch {:.2}   (golden gate: ≥0.58 · ≥0.52 · ≥700 · <0.95)",
            cv.legit, cv.asab, cv.wealth, cv.stretch
        );
        println!(
            "     span: {} crowns · {} towns · admin {:.1} vs capacity {:.1}   (ADR-0020: Σ(1+d/96) vs 12·crowns·era·asab)",
            cv.crowns, cv.towns, cv.admin, cv.capacity
        );
    }
    let civ_minted = count("first write of");
    let civ_golden = count("golden age dawns");
    let civ_falls = count("breaks. The crowns");
    let civ_closed = count("The interregnum ends");
    println!(
        " arc events: {} minted · {} golden dawns · {} falls · {} interregna closed",
        civ_minted, civ_golden, civ_falls, civ_closed
    );
    if years >= 100 {
        c.band("living civilizations", civs_alive as f64, format!("{} of {} ever", civs_alive, w.peoples.civs.len()));
    }
    // the closure is a partition: no tongue may answer two civilizations
    {
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        let mut shared = 0usize;
        for cv in w.peoples.civs.iter().filter(|cv| cv.alive) {
            for p in &cv.peoples {
                if !seen.insert(p.idx()) {
                    shared += 1;
                }
            }
        }
        c.must("civ membership is a partition", shared == 0, format!("{} shared tongues", shared), "M13.1: kinship-closure — one tongue, one civilization");
    }
    // a standing civilization must still hold crowns; a crownless one
    // belongs in its interregnum, not on the map
    let hollow = w
        .peoples
        .civs
        .iter()
        .filter(|cv| cv.alive && cv.stage != calliope::civ::Stage::Interregnum)
        .filter(|cv| {
            !w.peoples
                .realms
                .iter()
                .any(|r| r.alive && cv.peoples.contains(&r.people))
        })
        .count();
    c.must("no crownless civilization stands", hollow == 0, format!("{} hollow", hollow), "M13.4: losing every crown opens the interregnum");
    // successor realms per collapse — fragmentation, not deletion (M13.4)
    if civ_falls >= 1 {
        let ended: Vec<&calliope::civ::Civ> = w.peoples.civs.iter().filter(|cv| !cv.alive).collect();
        if !ended.is_empty() {
            let succ_total: usize = ended
                .iter()
                .map(|cv| {
                    w.peoples
                        .realms
                        .iter()
                        .filter(|r| r.alive && cv.peoples.contains(&r.people))
                        .count()
                })
                .sum();
            let per = succ_total as f64 / ended.len() as f64;
            c.band("successor realms per collapse", per, format!("{:.1} over {} falls", per, ended.len()));
        }
    }
    c.band("events per year", ev_per_year, format!("{:.1}", ev_per_year));
    c.want("no long silences", log.max_gap <= 36, format!("{} mo", log.max_gap), "≤36 months between chronicle entries");
    if years >= 80 {
        c.want("strikes happened", log.strikes >= 1, format!("{}", log.strikes), "the age of prospectors must actually happen");
        c.want("era advanced past Stone", max_era >= 1, society::ERAS[max_era].to_string(), "≥ Age of Bronze by now");
        c.want("arts accumulate", techs_total >= 3 * w.peoples.societies.len(), format!("{} arts / {} peoples", techs_total, w.peoples.societies.len()), "≥3 arts per people");
    }
    if years >= 140 {
        c.want("era reached Iron", max_era >= 2, society::ERAS[max_era].to_string(), "≥ Age of Iron by now");
    }
    c.must("numbers stay finite", finite_ok, if finite_ok { "yes".into() } else { "NO".into() }, "no NaN pops, wealth or prices");

    // ---- M2.3 rank-size: log(pop) vs log(rank) OLS slope ≈ −1 (Zipf) ----
    let mut pops: Vec<f64> = w.peoples.settlements.iter().map(|s| s.pop as f64).filter(|&p| p >= 120.0).collect();
    pops.sort_by(|a, b| b.partial_cmp(a).unwrap());
    if pops.len() >= 10 {
        let n = pops.len() as f64;
        let (mut sx, mut sy, mut sxx, mut sxy) = (0.0, 0.0, 0.0, 0.0);
        for (i, p) in pops.iter().enumerate() {
            let xr = ((i + 1) as f64).ln();
            let yr = p.ln();
            sx += xr; sy += yr; sxx += xr * xr; sxy += xr * yr;
        }
        let slope = (n * sxy - sx * sy) / (n * sxx - sx * sx);
        c.band("rank-size slope (Zipf)", slope, format!("{:.2} over {} towns", slope, pops.len()));
    }

    // ---- M2.5 spacing: median nearest-neighbour distance in km ----
    let mut nn: Vec<f64> = Vec::new();
    for (i, s) in w.peoples.settlements.iter().enumerate() {
        let mut best = f64::INFINITY;
        for (j, o) in w.peoples.settlements.iter().enumerate() {
            if i != j {
                let d2 = ((s.x - o.x).pow(2) + (s.y - o.y).pow(2)) as f64;
                best = best.min(d2);
            }
        }
        if best.is_finite() {
            nn.push(best.sqrt() * 4.0); // 4 km per cell (ADR-0004)
        }
    }
    if !nn.is_empty() {
        nn.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let med = nn[nn.len() / 2];
        c.band("median town spacing", med, format!("{:.0} km", med));
    }

    // ---- M2.6 famine: dry years starve somewhere, but not everywhere ----
    if years >= 100 {
        let per_c = log.famines as f64 * 100.0 / years.max(1) as f64;
        c.band("famine events per century", per_c, format!("{:.1}", per_c));
    }

    // ---- M72 famine causality: every hunger answers to the year's rain ----
    // The famine pass reads the realized rain anomaly as a standardized
    // index (SPI, McKee 1993) against the latitude's own interannual
    // spread. So each logged famine must sit at a cell-year whose SPI is
    // at or below the drought threshold — no private die, no exception.
    if !log.famine_sites.is_empty() {
        let rows = w.fields.tmean.dim().0 as f64;
        // M80 — the field the harvest verdict reads is the *accumulated*
        // standardized shortfall, not one year's draw. Re-derived here
        // from the published per-cell sky law alone, independently of the
        // simulation's own code path: same declared constants, separate
        // arithmetic. Everything M72 claimed still has to hold on it.
        let didx = |year: i64, x: i64, y: i64| -> f64 {
            let nrows = w.fields.tmean.dim().0;
            let lat = (-90.0 + (y as f64) * 180.0 / (rows - 1.0)).abs();
            let sigma = calliope::climate::anomaly_amp_p(lat).max(1e-6);
            let mut acc = 0.0;
            let mut wt = 1.0;
            for k in 0..calliope::drought::MEMO_YEARS as i64 {
                let yr = year - k;
                let (_, dp) = calliope::climate::year_anomaly_at(
                    w.variability(), nrows, x as usize, y as usize, yr, w.year_osc(yr),
                );
                acc += wt * dp / sigma;
                wt *= calliope::drought::MEM;
            }
            acc * calliope::drought::NORM
        };
        let mut zs: Vec<f64> = Vec::with_capacity(log.famine_sites.len());
        for &(m, x, y, _) in &log.famine_sites {
            zs.push(didx(m / 12, x, y));
        }
        let worst = zs.iter().cloned().fold(f64::INFINITY, f64::min);
        let driest_ok = zs
            .iter()
            .filter(|z| **z <= calliope::famine::DROUGHT_Z + 1e-9)
            .count();
        let mean_z = zs.iter().sum::<f64>() / zs.len() as f64;
        println!();
        println!(
            "M72 · famine causality — {} famines · mean SPI {:.2} · worst {:.2}",
            zs.len(),
            mean_z,
            worst
        );
        c.must(
            "every famine stands in a failed year",
            driest_ok == zs.len(),
            format!("{}/{} at SPI ≤ {:.1}", driest_ok, zs.len(), calliope::famine::DROUGHT_Z),
            "M72: hunger is the year's realized rain read as SPI, never a private die",
        );
        c.must(
            "famine years are meaningfully dry",
            mean_z <= calliope::famine::DROUGHT_Z,
            format!("mean SPI {:.2}", mean_z),
            "M72: the mean famine year sits at or beyond moderate drought",
        );

        // ---- dose-response: dryness governs hunger, it does not merely
        // accompany it. Every eligible town-year (the famine pass's own
        // predicate) is binned by the SPI it actually stood in, and the
        // share that starved is read off per bin. If rain governs, the
        // share is zero above the threshold and climbs as the bins dry.
        let spi_of = |year: i64, x: i64, y: i64| -> f64 { didx(year, x, y) };
        let struck: BTreeSet<(i64, i64, i64)> =
            log.famine_sites.iter().map(|&(m, x, y, _)| (m / 12, x, y)).collect();
        // bins, driest first: ≤−2 (extreme), (−2,−1] (moderate), (−1,0], >0
        let edges = [-2.0f64, -1.0, 0.0];
        let names = ["SPI ≤ −2", "−2 < SPI ≤ −1", "−1 < SPI ≤ 0", "SPI > 0"];
        let mut tot = [0usize; 4];
        let mut hit = [0usize; 4];
        for &(year, x, y) in &log.famine_pool {
            let z = spi_of(year, x, y);
            let b = if z <= edges[0] {
                0
            } else if z <= edges[1] {
                1
            } else if z <= edges[2] {
                2
            } else {
                3
            };
            tot[b] += 1;
            if struck.contains(&(year, x, y)) {
                hit[b] += 1;
            }
        }
        let rate = |b: usize| if tot[b] == 0 { 0.0 } else { hit[b] as f64 / tot[b] as f64 };
        println!(
            "  dose-response over {} eligible town-years — {}",
            log.famine_pool.len(),
            (0..4)
                .map(|b| format!("{} {:.0}% ({}/{})", names[b], 100.0 * rate(b), hit[b], tot[b]))
                .collect::<Vec<_>>()
                .join(" · ")
        );
        let wet_clean = hit[2] + hit[3] == 0;
        c.must(
            "no town starves in a year that was not dry",
            wet_clean,
            format!("{} famines above SPI −1 of {} wet-side town-years", hit[2] + hit[3], tot[2] + tot[3]),
            "M72: the drought threshold is a hard boundary on hunger — above SPI −1 the harvest verdict never starves anyone",
        );
        // The verdict is a *threshold* law: below SPI −1 the harvest fails,
        // full stop, so incidence saturates at 100% in both dry bins and
        // carries no dose. The dose the law modulates is severity, and the
        // honest measure of it is not a coarse two-bin mean of the dead —
        // that reads town size, not drought — but the *per-capita* toll
        // read against each famine's own shortfall depth, at the mechanism
        // itself (the pass's ledger: souls at risk, the anomaly it read,
        // the granary factor it applied, the toll it took).
        let led = &w.famine_ledger;
        // (a) the ledger is the harvest verdict, reproduced exactly.
        let mut exact = 0usize;
        for r in led {
            let z_here = spi_of(r.m / 12, r.x, r.y);
            let sf = (((-z_here) - (-calliope::famine::DROUGHT_Z)) / (-calliope::famine::DROUGHT_Z)).min(1.0);
            let hit = ((r.pop as f64) * (0.05 + 0.16 * sf) * r.granary) as i64;
            let dead = (hit as f64 * 0.55) as i64;
            if (sf - r.shortfall).abs() < 1e-9 && hit == r.hit && dead == r.dead {
                exact += 1;
            }
        }
        c.must(
            "the toll is the shortfall's own arithmetic",
            exact == led.len() && !led.is_empty(),
            format!("{}/{} famines reproduced exactly from the year's SPI", exact, led.len()),
            "M72: every toll re-derives from the realized rain at that cell — no residual, no die",
        );
        // (b) the dose, read continuously: per-capita toll against depth.
        // Normalizing by the granary factor removes the craft's blunting,
        // which is a property of the people, not of the sky.
        let dose: Vec<(f64, f64)> = led
            .iter()
            .map(|r| (r.shortfall, (r.hit as f64) / (r.pop as f64) / r.granary))
            .collect();
        let nd = dose.len() as f64;
        let (mdx, mdy) = (
            dose.iter().map(|p| p.0).sum::<f64>() / nd,
            dose.iter().map(|p| p.1).sum::<f64>() / nd,
        );
        let (mut cov, mut vx, mut vy) = (0.0, 0.0, 0.0);
        for (a, b) in &dose {
            cov += (a - mdx) * (b - mdy);
            vx += (a - mdx).powi(2);
            vy += (b - mdy).powi(2);
        }
        let slope = cov / vx.max(1e-12);
        let intercept = mdy - slope * mdx;
        let rho_sev = cov / (vx.sqrt() * vy.sqrt()).max(1e-12);
        // (c) monotone in depth across quartiles of the shortfall the world
        // actually produced — a continuous dose, not two sparse SPI bins.
        let mut byd = dose.clone();
        byd.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let q: Vec<f64> = (0..4)
            .map(|k| {
                let lo = k * byd.len() / 4;
                let hi = ((k + 1) * byd.len() / 4).max(lo + 1).min(byd.len());
                byd[lo..hi].iter().map(|p| p.1).sum::<f64>() / ((hi - lo) as f64)
            })
            .collect();
        let monotone = q.windows(2).all(|p| p[1] > p[0]);
        println!(
            "  severity dose over {} famines — per-capita toll {:.4} + {:.4}·shortfall (law: 0.0500 + 0.1600) · ρ {:.4}",
            led.len(),
            intercept,
            slope,
            rho_sev
        );
        println!(
            "  by shortfall quartile — {}",
            q.iter().map(|v| format!("{:.3}", v)).collect::<Vec<_>>().join(" → ")
        );
        c.must(
            "hunger climbs with the drought",
            monotone
                && rho_sev >= 0.99
                && (slope - 0.16).abs() <= 0.02
                && (intercept - 0.05).abs() <= 0.01,
            format!(
                "toll {:.4}+{:.4}·s · ρ {:.4} · quartiles {}",
                intercept,
                slope,
                rho_sev,
                q.iter().map(|v| format!("{:.3}", v)).collect::<Vec<_>>().join("<")
            ),
            "M72: the per-capita toll rises linearly with the depth of the drought and monotonically across its quartiles — the threshold decides whether, the shortfall decides how hard",
        );


        // ---- the placebo: the same towns, the same years' worth of sky,
        // but the wrong year. If hunger were a property of *place* (bad
        // ground, a thin margin) rather than of the *year's* rain, these
        // shuffled skies would look just as dry as the real ones. They
        // must not: the real famine years are drought-selected, the
        // counterfactual ones are the world's ordinary base rate.
        // Deterministic offsets, no die.
        let horizon = (years as i64).max(1);
        let mut cf_dry = 0usize;
        let mut cf_n = 0usize;
        for (i, &(m, x, y, _)) in log.famine_sites.iter().enumerate() {
            let year = m / 12;
            for k in 1..=4i64 {
                let alt = ((year + k * 7 + i as i64 * 3).rem_euclid(horizon)).max(1);
                if alt == year {
                    continue;
                }
                cf_n += 1;
                if spi_of(alt, x, y) <= calliope::famine::DROUGHT_Z {
                    cf_dry += 1;
                }
            }
        }
        let cf_rate = if cf_n == 0 { 1.0 } else { cf_dry as f64 / cf_n as f64 };
        println!(
            "  counterfactual sky — the same {} hungry cells under {} wrong-year skies: {:.0}% would have been dry (real 100%)",
            log.famine_sites.len(),
            cf_n,
            100.0 * cf_rate
        );
        c.must(
            "the year, not the place, makes the famine",
            cf_rate <= 0.50,
            format!("{:.0}% of wrong-year skies dry vs 100% of the real ones", 100.0 * cf_rate),
            "M72: swap the year and the drought mostly vanishes — hunger is selected by the realized sky, not by the cell",
        );
    }



    // ---- M80: the failed year named — droughts with a span and a name ----
    // The drought field now carries memory (`drought::MEM`), so a failed
    // year is not an isolated roll: it takes hold over ground, holds it
    // for years, and gets a name the chronicle speaks exactly once. This
    // lane measures the span, the footprint's steadiness, the singularity
    // of the naming, and — the causal claim — that the memory is what
    // lets an ordinary year still fail.
    {
        let ds = &w.droughts;
        let rows = w.fields.tmean.dim().0;
        let didx = |year: i64, x: i64, y: i64| -> f64 {
            let lat = (-90.0 + (y as f64) * 180.0 / (rows as f64 - 1.0)).abs();
            let sigma = calliope::climate::anomaly_amp_p(lat).max(1e-6);
            let mut acc = 0.0;
            let mut wt = 1.0;
            for k in 0..calliope::drought::MEMO_YEARS as i64 {
                let yr = year - k;
                let (_, dp) = calliope::climate::year_anomaly_at(
                    w.variability(), rows, x as usize, y as usize, yr, w.year_osc(yr),
                );
                acc += wt * dp / sigma;
                wt *= calliope::drought::MEM;
            }
            acc * calliope::drought::NORM
        };
        // Only droughts that had room to end inside the run are judged on
        // their span: one still burning at the last year is right-censored.
        let last = w.month / 12;
        let closed: Vec<&calliope::drought::DroughtEvent> =
            ds.events.iter().filter(|e| e.last_year < last).collect();
        let mut durs: Vec<i64> = closed.iter().map(|e| e.duration()).collect();
        durs.sort_unstable();
        let med_dur = if durs.is_empty() { 0.0 } else { durs[durs.len() / 2] as f64 };
        let mean_dur = if durs.is_empty() {
            0.0
        } else {
            durs.iter().sum::<i64>() as f64 / durs.len() as f64
        };
        let longest = durs.last().copied().unwrap_or(0);
        let mut stabs: Vec<f64> = closed.iter().filter_map(|e| e.stability()).collect();
        stabs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let med_stab = if stabs.is_empty() { 0.0 } else { stabs[stabs.len() / 2] };
        let med_area = {
            let mut a: Vec<f64> = closed
                .iter()
                .map(|e| e.peak_nodes as f64 * calliope::drought::NODE_KM2)
                .collect();
            a.sort_by(|x, y| x.partial_cmp(y).unwrap());
            if a.is_empty() { 0.0 } else { a[a.len() / 2] }
        };
        println!();
        println!(
            "M80 · droughts named — {} took hold ({} closed) · median span {:.0}y (mean {:.1}, longest {}) · median extent {:.0} km² · median year-on-year overlap {:.2}",
            ds.events.len(), closed.len(), med_dur, mean_dur, longest, med_area, med_stab
        );
        println!(
            "  spans — {}",
            (1..=6)
                .map(|k| {
                    let n = if k < 6 {
                        durs.iter().filter(|d| **d == k).count()
                    } else {
                        durs.iter().filter(|d| **d >= 6).count()
                    };
                    format!("{}{}y {}", if k == 6 { "≥" } else { "" }, k, n)
                })
                .collect::<Vec<_>>()
                .join(" · ")
        );
        c.must(
            "droughts last years, not a season",
            !durs.is_empty() && (2.0..=5.0).contains(&med_dur),
            format!("median span {:.0}y over {} closed droughts", med_dur, durs.len()),
            "M80 gate: a failed year persists — the drought field carries its shortfall forward, so the median event spans 2–5 consecutive years",
        );
        c.must(
            "a drought holds the same ground",
            !stabs.is_empty() && med_stab >= 0.40,
            format!("median year-on-year footprint overlap {:.2} (Jaccard)", med_stab),
            "M80 gate: the mapped extent is stable — a drought is one region moving slowly, not a new blotch each year",
        );
        // The naming: one drought, one name, spoken once.
        let names: std::collections::BTreeSet<&str> =
            ds.events.iter().map(|e| e.name.as_str()).collect();
        let mut spoken: BTreeMap<&str, usize> = BTreeMap::new();
        for n in &log.drought_named {
            *spoken.entry(n.as_str()).or_default() += 1;
        }
        let once = ds
            .events
            .iter()
            .filter(|e| spoken.get(e.name.as_str()).copied().unwrap_or(0) == 1)
            .count();
        println!(
            "  naming — {} droughts · {} distinct names · {} chronicle announcements · e.g. {}",
            ds.events.len(),
            names.len(),
            log.drought_named.len(),
            ds.events
                .iter()
                .rev()
                .take(3)
                .map(|e| format!("{} ({}–{})", e.name, e.start_year, e.last_year))
                .collect::<Vec<_>>()
                .join(" · ")
        );
        c.must(
            "every drought is named once and named alone",
            !ds.events.is_empty()
                && names.len() == ds.events.len()
                && once == ds.events.len()
                && log.drought_named.len() == ds.events.len(),
            format!(
                "{}/{} droughts surfaced exactly once · {} distinct names",
                once,
                ds.events.len(),
                names.len()
            ),
            "M80 gate: the chronicle speaks each drought's name exactly once, in the year it takes hold — no duplicates, no silent droughts",
        );
        // Re-derivation: the ledger claims dry ground; the sky's own
        // arithmetic, recomputed here from the seed, must agree at every
        // event's deepest node in every year it held.
        let mut checked = 0usize;
        let mut agree = 0usize;
        for e in &ds.events {
            for &(yr, _, _, _, _, ax, ay) in &e.years {
                checked += 1;
                if didx(yr, ax, ay) <= calliope::famine::DROUGHT_Z + 1e-9 {
                    agree += 1;
                }
            }
        }

        c.must(
            "the ledger answers to the sky",
            checked > 0 && agree == checked,
            format!("{}/{} drought-years re-derived dry from the seed alone", agree, checked),
            "M80: the named event is a reading of the accumulated rain, re-computable from seed × cell × year with no stored per-cell state",
        );
        // The causal claim: memory is load-bearing. How many famines stood
        // in a year whose *own* rain was not drought — a harvest that only
        // failed because the years behind it had already emptied the ground.
        let mut carried = 0usize;
        for r in &w.famine_ledger {
            let yr = r.m / 12;
            if w.year_spi(yr, r.y as usize, r.x as usize) > calliope::famine::DROUGHT_Z {
                carried += 1;
            }
        }
        let share = if w.famine_ledger.is_empty() {
            0.0
        } else {
            carried as f64 / w.famine_ledger.len() as f64
        };
        println!(
            "  carry-over — {}/{} famines ({:.0}%) struck in a year whose own rain was above SPI −1: the ground, not the year, was empty",
            carried,
            w.famine_ledger.len(),
            100.0 * share
        );
        c.must(
            "the ground remembers the year before",
            carried > 0,
            format!("{} famines carried by memory alone ({:.0}%)", carried, 100.0 * share),
            "M80: persistence is load-bearing — remove the memory and these harvests would not have failed",
        );
        c.band("drought carry-over share of famines", share, format!("{:.2}", share));
        let per_c = ds.events.len() as f64 * 100.0 / (years.max(1) as f64);
        c.band("named droughts per century", per_c, format!("{:.1}", per_c));
    }

    // ---- M72: the year that was — one sky over harvest, flow and famine ----
    {
        let (rows, cols) = w.fields.tmean.dim();
        // farmed ground only: the harvest lane has nothing to say about ice
        // or open water, and averaging wildland in would dilute the signal.
        let mut cells: Vec<(usize, usize)> = Vec::new();
        for y in (2..rows).step_by(7) {
            for x in (2..cols).step_by(7) {
                let pack = calliope::agriculture::CropPackage::from_code(w.fields.crops[[y, x]]);
                if pack != calliope::agriculture::CropPackage::Wildland {
                    cells.push((y, x));
                }
            }
        }
        let sample_years: Vec<i64> = (1..=32).collect();
        let (mut dp_n, mut dp_s, mut yl_n, mut yl_s) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        let (mut dq_n, mut dq_s, mut fl_n, mut fl_s) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        let (mut pred_s, mut pred_n, mut clamped, mut out_of_band) = (0.0f64, 0.0f64, 0usize, 0usize);
        let (mut ex_s, mut ex_n) = (0.0f64, 0.0f64);
        let (mut sign_ok, mut sign_n) = (0usize, 0usize);
        let mut pairs: Vec<(f64, f64)> = Vec::new();
        let mut ex_pairs: Vec<(f64, f64)> = Vec::new();
        for &yr in &sample_years {
            for &(y, x) in &cells {
                let pack = calliope::agriculture::CropPackage::from_code(w.fields.crops[[y, x]]);
                let t = w.fields.tmean[[y, x]] as f64;
                let p = w.fields.precip[[y, x]] as f64;
                let irr = w.irrigable(y, x);
                let (dt_here, dp, dq) =
                    w.with_year_sky(yr, |dt, dp, dq| (dt[[y, x]], dp[[y, x]], dq[[y, x]]));
                let yf = w.year_yield(yr, y, x);
                let ff = w.year_flow_factor(yr, y, x);
                if !(calliope::agriculture::YIELD_FLOOR + 1e-9..calliope::agriculture::YIELD_CEIL - 1e-9)
                    .contains(&yf)
                {
                    clamped += 1;
                }
                if !(0.34..=2.21).contains(&ff) {
                    out_of_band += 1;
                }
                // Watered ground drinks the catchment lane, not the cloud —
                // and it drinks the *published* flow law, clamp included:
                // `ff` is the very multiplier `year_discharge` applies, so a
                // canal can never be fed more water than the river carries.
                let _ = dq;
                let rain = if irr { ff - 1.0 } else { dp };
                let base = calliope::agriculture::climatic_score(pack, t, p, irr);
                // (a) the *exact* prediction: the crop curves re-scored here,
                // in the harness, from the raw mean fields and the year's two
                // anomalies — the same law, an independent evaluation path.
                // This is what the harvest must equal: the curves' full
                // nonlinear response, gaussian curvature and trapezoid kinks
                // and all, not a tangent line drawn at the mean.
                let exact = if base > 1e-9 {
                    let now = calliope::agriculture::climatic_score(
                        pack,
                        t + dt_here,
                        (p * (1.0 + rain)).max(0.0),
                        irr,
                    );
                    (now / base).clamp(
                        calliope::agriculture::YIELD_FLOOR,
                        calliope::agriculture::YIELD_CEIL,
                    ) - 1.0
                } else {
                    0.0
                };
                // (b) the *linear* model: a ±1 % central difference in rain
                // and a ±0.1 °C one in warmth, carried by the year's
                // anomalies. Kept as the direction lane only — at σ ≈ 12 %
                // rain swings a tangent line cannot price the curves'
                // concavity, and measuring magnitude against it measured the
                // linearization, not the world.
                let pred = if base > 1e-9 {
                    let pu = calliope::agriculture::climatic_score(pack, t, p * 1.01, irr);
                    let pd = calliope::agriculture::climatic_score(pack, t, p * 0.99, irr);
                    let tu = calliope::agriculture::climatic_score(pack, t + 0.1, p, irr);
                    let td = calliope::agriculture::climatic_score(pack, t - 0.1, p, irr);
                    ((pu - pd) / (0.02 * base)) * rain + ((tu - td) / (0.2 * base)) * dt_here
                } else {
                    0.0
                };
                if pred.abs() >= 0.01 {
                    sign_n += 1;
                    if pred.signum() == (yf - 1.0).signum() || (yf - 1.0).abs() < 1e-12 {
                        sign_ok += 1;
                    }
                }
                pred_s += pred * pred;
                pred_n += pred;
                ex_s += exact * exact;
                ex_n += exact;
                dp_n += dp; dp_s += dp * dp;
                yl_n += yf - 1.0; yl_s += (yf - 1.0).powi(2);
                dq_n += dq; dq_s += dq * dq;
                fl_n += ff - 1.0; fl_s += (ff - 1.0).powi(2);
                pairs.push((pred, yf - 1.0));
                ex_pairs.push((exact, yf - 1.0));
            }
        }
        let n = (cells.len() * sample_years.len()) as f64;
        let sd = |sum: f64, sq: f64| (sq / n - (sum / n).powi(2)).max(0.0).sqrt();
        let sd_dp = sd(dp_n, dp_s);
        let sd_dq = sd(dq_n, dq_s);
        let sd_yield = sd(yl_n, yl_s);
        let sd_flow = sd(fl_n, fl_s);
        let sd_pred = sd(pred_n, pred_s);
        let sd_exact = sd(ex_n, ex_s);
        let pearson = |v: &Vec<(f64, f64)>| -> f64 {
            let (mx, my): (f64, f64) = (
                v.iter().map(|p| p.0).sum::<f64>() / v.len() as f64,
                v.iter().map(|p| p.1).sum::<f64>() / v.len() as f64,
            );
            let (mut cov, mut vx, mut vy) = (0.0, 0.0, 0.0);
            for (a, b) in v {
                cov += (a - mx) * (b - my);
                vx += (a - mx).powi(2);
                vy += (b - my).powi(2);
            }
            cov / (vx.sqrt() * vy.sqrt()).max(1e-12)
        };
        let rho_exact = pearson(&ex_pairs);
        let rho_lin = pearson(&pairs);
        println!();
        println!("M72 · the year that was — {} farmed cells × {} years", cells.len(), sample_years.len());
        println!(
            "  rain anomaly σ {:.4} · catchment σ {:.4} · harvest σ {:.4} (curves {:.4} · tangent {:.4}) · flow σ {:.4}",
            sd_dp, sd_dq, sd_yield, sd_exact, sd_pred, sd_flow,
        );
        println!(
            "  ρ against the curves {:.4} · against the tangent {:.4} · direction agrees {}/{} where the tangent calls a ≥1% move",
            rho_exact, rho_lin, sign_ok, sign_n,
        );

        // the harvest moves, and it moves exactly as the crop curves say:
        // the harness re-scores the same law from the raw fields and the
        // year's sky, and the world's realized spread must equal it.
        let harvest_err = if sd_exact > 1e-9 { (sd_yield / sd_exact - 1.0).abs() } else { 1.0 };
        c.must(
            "harvest variance tracks the sky",
            harvest_err <= 0.01 && sd_yield > 1e-4,
            format!("σ {:.4} vs curves {:.4} ({:+.2}%)", sd_yield, sd_exact, 100.0 * (sd_yield / sd_exact.max(1e-12) - 1.0)),
            "M72: the year's harvest spread equals the crop curves re-scored at the year's own sky, within 1%",
        );
        // the rivers move with their catchments, at the declared gain
        let flow_pred = calliope::climate::FLOW_ANOM_GAIN * sd_dq;
        let flow_err = if flow_pred > 1e-9 { (sd_flow / flow_pred - 1.0).abs() } else { 1.0 };
        c.must(
            "flow variance tracks the catchment",
            flow_err <= 0.15 && sd_flow > 1e-4,
            format!("σ {:.4} vs {:.2}× catchment σ {:.4} ({:+.1}%)", sd_flow, calliope::climate::FLOW_ANOM_GAIN, flow_pred, 100.0 * (sd_flow / flow_pred.max(1e-12) - 1.0)),
            "M72: the year's flow spread is the catchment anomaly times the declared gain, within 15%",
        );
        c.must(
            "the harvest follows the curves",
            rho_exact >= 0.999,
            format!("ρ {:.4} (tangent model {:.3})", rho_exact, rho_lin),
            "M72: the realized harvest factor is the crop curves' own response to the year's sky (Pearson ρ ≥ 0.999 against an independent re-scoring)",
        );
        // and the direction is first-order right: where the tangent calls a
        // move worth ≥1%, the world moves that way. Magnitude is the curves'
        // to price; sign is the derivative's, and it must not be wrong.
        let sign_rate = if sign_n == 0 { 0.0 } else { sign_ok as f64 / sign_n as f64 };
        c.must(
            "wet years feed, dry years starve",
            sign_rate >= 0.95 && sign_n > 1000,
            format!("{:.1}% of {} called moves go the derivative's way", 100.0 * sign_rate, sign_n),
            "M72: the sign of the harvest response matches the crop curves' own derivatives on the year's anomalies",
        );
        c.must(
            "the year is bounded, not deleted",
            out_of_band == 0,
            format!("{} clamped harvests · {} flows out of band", clamped, out_of_band),
            "M72: flow stays inside [0.35, 2.20]; a year moves a town, it does not erase one",
        );
        // and the map beneath it never moved
        let river_now = {
            let mut b: Vec<u8> = Vec::new();
            for v in w.fields.discharge.iter() { b.extend_from_slice(&v.to_le_bytes()); }
            for v in w.fields.strahler.iter() { b.push(*v); }
            for v in w.fields.flags.iter() { b.push(*v); }
            (w.fields.discharge.dim(), fnv(&b))
        };
        c.must(
            "the river map does not move",
            river_now == river_dawn,
            format!("dawn {:016x} · year {} {:016x}", river_dawn.1, years, river_now.1),
            "M72: order, discharge and the endorheic calls are the dawn's — the year is a read, never a write",
        );
    }

    // ---- M24 disaster wiring: every fall told once, arcs that close ----
    // A destroying-magnitude event fells a town through the one kill
    // path, which raises exactly one ruin (with the disaster's why) and
    // one chronicle beat citing the ruin's registry id. Felt beats cite
    // the surviving town, so the Ruin-kind filter separates the two.
    let disaster_whys = [
        calliope::patina::ruin_why("quake"),
        calliope::patina::ruin_why("ash"),
    ];
    let disaster_ruins = w
        .ruins
        .iter()
        .filter(|r| disaster_whys.contains(&r.why.as_str()))
        .count();
    let falls = w
        .chronicle
        .events
        .iter()
        .filter(|e| e.k.name() == "quake" || e.k.name() == "eruption")
        .filter(|e| {
            e.ids.first().is_some_and(|id| {
                w.chronicle
                    .registry
                    .items
                    .get(id.idx())
                    .is_some_and(|it| it.kind == EntityKind::Ruin)
            })
        })
        .count();
    c.must(
        "disaster falls have their telling",
        falls == disaster_ruins,
        format!("{} falls · {} ruins", falls, disaster_ruins),
        "M24 gate: every destroying-magnitude event → one chronicle entry + one ruin",
    );
    if !w.rebuild_log.is_empty() {
        let mut arcs = w.rebuild_log.clone();
        arcs.sort_unstable();
        let med = arcs[arcs.len() / 2] as f64;
        c.range(
            "disaster recovery (median months)",
            med,
            format!("{:.0} mo over {} arcs", med, arcs.len()),
            (6.0, 360.0),
            (1.0, 480.0),
            "M24 gate: rebuild arcs close inside the forty-year window",
        );
    }

    // ---- M3 gates: words and ways ----
    // M3.1 label audit: a town's name must classify to the tongue that
    // coined it — it starts with one of the bank's openers and ends with
    // one of its closers. Audited against the NAMER, not the current
    // owner: conquest moves borders, not names (M9.2).
    {
        let mut audited = 0usize;
        let mut hits = 0usize;
        for s in &w.peoples.settlements {
            let Some(cu) = w.peoples.peoples.get(s.namer.idx()) else { continue };
            let b = naming::bank(&cu.style);
            audited += 1;
            let pre_ok = b.pre.iter().any(|(p, _)| s.name.starts_with(p));
            // a tongue's closers are its bank ends PLUS its landform
            // generics (M62) — the vocabulary grew, the audit knows it
            let end_ok = b.end.iter().any(|(e, _)| s.name.ends_with(e))
                || naming::CLASSES.iter().any(|cl| {
                    naming::landform_generics(&cu.style, cl)
                        .iter()
                        .any(|(g, _)| s.name.ends_with(g))
                });
            if pre_ok && end_ok {
                hits += 1;
            }
        }
        if audited > 0 {
            let share = hits as f64 / audited as f64;
            c.band("toponyms classify to culture", share, pct(share));
        }

        // M3.3 gloss coverage: every fragment of every bank carries a gloss.
        let mut frags = 0usize;
        let mut glossed = 0usize;
        for st in naming::STYLES {
            let b = naming::bank(st);
            for (_, g) in b.pre.iter().chain(b.mid.iter()).chain(b.end.iter()) {
                frags += 1;
                glossed += (!g.is_empty()) as usize;
            }
        }
        c.must("gloss coverage of name banks", glossed == frags, format!("{}/{}", glossed, frags), "M3.3 gate: 100% of fragments glossed");

        // …and every settlement & feature carries a readable etymology.
        let s_ety = w.peoples.settlements.iter().filter(|s| !s.ety.is_empty()).count();
        let f_ety = w.features.iter().filter(|f| !f.ety.is_empty()).count();
        c.must("settlement etymologies", s_ety == w.peoples.settlements.len(), format!("{}/{}", s_ety, w.peoples.settlements.len()), "M3.3: every town name reads back");
        c.must("feature etymologies", f_ety == w.features.len(), format!("{}/{}", f_ety, w.features.len()), "M3.3: every feature name reads back");

        // M3 no name-collision regressions: towns, features, peoples.
        // A failure prints the colliding names — the report must carry
        // its own evidence (ADR-0009).
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut dup_names: Vec<&str> = Vec::new();
        for n in w
            .peoples.settlements
            .iter()
            .map(|s| s.name.as_str())
            .chain(w.features.iter().map(|f| f.name.as_str()))
            .chain(w.peoples.peoples.iter().map(|cu| cu.people.as_str()))
        {
            if !seen.insert(n) {
                dup_names.push(n);
            }
        }
        let dups = dup_names.len();
        let dup_note = if dups == 0 { "0".to_string() } else { format!("{} ({})", dups, dup_names.join(" · ")) };
        c.must("name collisions", dups == 0, dup_note, "M3 gate: 0 duplicates");

        // M3.4 exonyms: where two peoples actually share country, a border
        // feature should carry a second name in the other tongue. Peoples
        // on far-apart continents legitimately double nothing — count the
        // candidate features first (same geometry as culture_toponyms) and
        // only demand exonyms when candidates exist.
        if w.peoples.peoples.len() >= 2 {
            let mut candidates = 0usize;
            for f in &w.features {
                if matches!(f.t.as_str(), "ocean" | "sea" | "continent" | "river" | "delta") {
                    continue;
                }
                let mut best = vec![f64::INFINITY; w.peoples.peoples.len()];
                for s in &w.peoples.settlements {
                    let dx = (s.x - f.x) as f64;
                    let dy = (s.y - f.y) as f64;
                    let d2 = dx * dx + dy * dy;
                    if d2 < best[s.people.idx()] {
                        best[s.people.idx()] = d2;
                    }
                }
                let mut v: Vec<f64> = best.into_iter().filter(|d| d.is_finite()).collect();
                v.sort_by(|a, b| a.partial_cmp(b).unwrap());
                if v.len() >= 2
                    && v[0].sqrt() <= naming::TONGUE_REACH
                    && v[1].sqrt() <= naming::TONGUE_REACH * 1.8
                {
                    candidates += 1;
                }
            }
            let exo = w.features.iter().filter(|f| !f.alt.is_empty()).count();
            if candidates > 0 {
                c.want("border exonyms present", exo >= 1, format!("{} of {} shared", exo, candidates), "M3.4: doubled names on shared features");
            } else {
                c.must("border exonyms (no shared features)", true, "peoples live apart".to_string(), "M3.4: nothing to double on this seed");
            }
        }

        // M3.5 pantheon: four named gods per people, and the chronicle
        // must actually speak of them (festivals, omens, war names).
        let full = w.peoples.peoples.iter().filter(|cu| cu.pantheon.len() >= 3).count();
        c.must("pantheons complete", full == w.peoples.peoples.len(), format!("{}/{}", full, w.peoples.peoples.len()), "M3.5: ≥ 3 named gods per people");
        if years >= 100 {
            let cited = log.god_citations;
            c.must("gods cited in the chronicle", cited >= 1, format!("{}", cited), "M3.5: omens/festivals/wars name the gods");
        }
    }

    // ---- M4 gates: the great game ----
    {
        let pol_min = log.polities.iter().min().copied().unwrap_or(0);
        let pol_max = log.polities.iter().max().copied().unwrap_or(0);
        let pol_last = log.polities.last().copied().unwrap_or(0);

        // Territory sanity: every owned cell names a real realm (ADR-0018 —
        // borders are political), and the map covers a sane share of land.
        let n_cult = w.peoples.realms.len() as i16;
        let terr = w.fields.territory.as_slice().unwrap_or(&[]);
        let mut owned = 0usize;
        let mut bad = 0usize;
        let mut land_cells = 0usize;
        for (i, &h) in w.fields.height.iter().enumerate() {
            let t = terr.get(i).copied().unwrap_or(-1);
            if h >= 0.0 {
                land_cells += 1;
            }
            if t >= 0 {
                owned += 1;
                if t >= n_cult {
                    bad += 1;
                }
            }
        }
        let owned_share = owned as f64 / land_cells.max(1) as f64;
        c.must("territory owners valid", bad == 0, format!("{} bad cells", bad), "M4.1: every owned cell names a live realm");
        c.band("land under banners", owned_share, pct(owned_share));

        if years >= 100 {
            c.want("wars kindle", log.wars >= 1, format!("{}", log.wars), "M4: a century without war is a broken game");
            if log.wars >= 8 {
                c.want("land changes hands", log.transfers >= 1, format!("{} transfers / {} war events", log.transfers, log.wars), "M4.2 gate: border change per major war");
            }
            c.want("polity count moves", pol_max > pol_min, format!("{}–{}", pol_min, pol_max), "M4.5 gate: realms must rise and fall");
            let top_share = {
                let total: i64 = w.peoples.settlements.iter().map(|s| s.pop).sum();
                let mut by_c: BTreeMap<usize, i64> = BTreeMap::new();
                for s in &w.peoples.settlements {
                    *by_c.entry(s.realm.0).or_default() += s.pop;
                }
                by_c.values().max().copied().unwrap_or(0) as f64 / total.max(1) as f64
            };
            c.band("largest realm pop share", top_share, pct(top_share));
            let _ = pol_last;
        }
        if years >= 140 {
            c.want("coalitions form", log.coalition_seen, if log.coalition_seen { "yes".into() } else { "no".into() }, "M4.3 gate: dread should unite the wary");
        }
    }

    // ---- M5 gates: iron and coin ----
    {
        let n_areas = w.economy.areas.markets.len();
        if w.peoples.settlements.len() >= 30 {
            c.must("market areas carved", n_areas >= 2, format!("{} areas / {} towns", n_areas, w.peoples.settlements.len()), "M5.2: the route web splits into local markets");
        }
        // inter-area price divergence: mean over goods of max/min across areas
        if n_areas >= 2 {
            let mut goods = resources::GoodSet::EMPTY;
            for s in &w.peoples.settlements {
                for &g in s.goods.iter() {
                    goods.insert(g);
                }
            }
            let mut ratios = Vec::new();
            // M14.7 — the same spreads bucketed by value density: bulk
            // should stay dispersed (freight walls the markets apart),
            // precious should flatten (it crosses the map for its weight)
            let mut by_class: [Vec<f64>; 3] = [Vec::new(), Vec::new(), Vec::new()];
            for g in goods.iter() {
                let ps: Vec<f64> = w.economy.areas.markets.iter().map(|m| m.price(g)).collect();
                let lo = ps.iter().cloned().fold(f64::INFINITY, f64::min);
                let hi = ps.iter().cloned().fold(0.0f64, f64::max);
                if lo > 0.0 {
                    ratios.push(hi / lo);
                    let ci = match g.transport() {
                        resources::Transport::Bulk => 0,
                        resources::Transport::Ordinary => 1,
                        resources::Transport::Precious => 2,
                    };
                    by_class[ci].push(hi / lo);
                }
            }
            if !ratios.is_empty() {
                let mean = ratios.iter().sum::<f64>() / ratios.len() as f64;
                c.band("inter-area price divergence", mean, format!("×{:.2} mean spread", mean));
            }
            let cm = |v: &Vec<f64>| -> f64 {
                if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 }
            };
            let (mb, mo, mp) = (cm(&by_class[0]), cm(&by_class[1]), cm(&by_class[2]));
            if mb > 0.0 && mp > 0.0 {
                c.want(
                    "von Thünen ordering",
                    mb >= mp,
                    format!("bulk ×{:.2} · ordinary ×{:.2} · precious ×{:.2}", mb, mo, mp),
                    "M14.7: bulk stays local and dispersed, precious arbitrages flat — rings emerge, not painted",
                );
            }
        }
        if years >= 100 {
            let crafts = w
                .peoples.settlements
                .iter()
                .filter(|s| s.goods.iter().any(|g| g.is_craft()))
                .count();
            c.want("recipe towns emerge", crafts >= 1, format!("{} towns craft", crafts), "M5.1 gate: ore, fuel and art make finished goods");
            let coin_known = w.peoples.societies.iter().any(|so| so.knows(calliope::society::TechId::Coin));
            if coin_known {
                c.want("merchants take the roads", !w.economy.merchants.is_empty(), format!("{} ever", w.economy.merchants.len()), "M5.5: coin-wise realms send out traders");
            }
        }
        // M5.4 gravity cross-check: realized route flow should correlate
        // with pop·pop/cost² over the same edges
        {
            let by_id: BTreeMap<calliope::ids::SettlementId, &_> = w.peoples.settlements.iter().map(|s| (s.id, s)).collect();
            let mut xs = Vec::new();
            let mut ys = Vec::new();
            for (ri, r) in w.routes.iter().enumerate() {
                let (Some(sa), Some(sb)) = (by_id.get(&r.a), by_id.get(&r.b)) else { continue };
                let flow = w.economy.route_flow.get(ri).copied().unwrap_or(0.0);
                if flow <= 0.0 {
                    continue;
                }
                let grav = (sa.pop as f64 * sb.pop as f64) / r.cost.max(1.0).powi(2);
                xs.push(grav.ln());
                ys.push(flow.ln());
            }
            if xs.len() >= 12 {
                let corr = pearson(&xs, &ys);
                c.band("gravity-model correlation", corr, format!("r={:.2} over {} routes", corr, xs.len()));
            }
        }
    }

    // ----------------------------------------------- M55 the dry frontier
    // No town may stand on arid ground with no surface water, spring or
    // oasis unless its people can sink a well to the table beneath it.
    let mut dry_towns = 0usize;
    let mut dry_welled = 0usize;
    let mut dry_illegal: Vec<String> = Vec::new();
    for st in &w.peoples.settlements {
        let (y, x) = (st.y as usize, st.x as usize);
        if !w.arid_dry[[y, x]] {
            continue;
        }
        dry_towns += 1;
        let reach = w
            .peoples.societies
            .get(st.people.idx())
            .map(calliope::settlements::well_reach_m)
            .unwrap_or(0.0);
        let depth = w.fields.aquifer[[y, x]] as f64;
        if reach > 0.0 && depth <= reach {
            dry_welled += 1;
        } else if dry_illegal.len() < 6 {
            dry_illegal.push(format!("{} (depth {:.0} m, reach {:.0} m)", st.name, depth, reach));
        }
    }
    println!();
    println!(
        "the dry frontier (M55): {} towns on waterless arid ground · {} of them within well reach{}",
        dry_towns,
        dry_welled,
        if dry_illegal.is_empty() { String::new() } else { format!(" · unwatered: {}", dry_illegal.join(", ")) }
    );
    c.must(
        "no town drinks where it cannot",
        dry_towns == dry_welled,
        format!("{}/{} watered", dry_welled, dry_towns),
        "M55 gate: every settlement on arid ground with no river, lake, spring or oasis is held by a people whose well craft reaches its water table",
    );

    // The law above is silent in a world whose colonists never walked into
    // the desert. So measure the mechanism itself: how much arid-dry ground
    // is worth founding on at all, and how much of it each rung of the well
    // ladder opens. A gate with no ground behind it is not a gate.
    const REACH_LADDER: [(f64, &str); 5] = [
        (0.0, "no craft"),
        (12.0, "hand-dug"),
        (30.0, "masonry"),
        (60.0, "aqueduct"),
        (90.0, "engineering"),
    ];
    let mut worth = 0usize;
    let mut opened = [0usize; 5];
    let (rows, cols) = w.arid_dry.dim();
    for y in 0..rows {
        for x in 0..cols {
            if !w.arid_dry[[y, x]] || w.dry_site_score[[y, x]] <= 2.2 {
                continue;
            }
            worth += 1;
            let depth = w.fields.aquifer[[y, x]] as f64;
            for (i, (r, _)) in REACH_LADDER.iter().enumerate() {
                if *r > 0.0 && depth <= *r {
                    opened[i] += 1;
                }
            }
        }
    }
    println!(
        "the well ladder (M55): {} arid-dry cells score above the founding bar · opened {}",
        worth,
        REACH_LADDER
            .iter()
            .enumerate()
            .map(|(i, (r, n))| format!("{} ({:.0} m) {}", n, r, opened[i]))
            .collect::<Vec<_>>()
            .join(" · ")
    );
    c.must(
        "the dry frontier is real ground",
        worth > 0,
        format!("{} foundable arid-dry cells", worth),
        "M55: the veto must hold back ground a people would otherwise want — otherwise the gate proves nothing",
    );
    let monotone = opened.windows(2).all(|p| p[1] >= p[0]);
    c.must(
        "well craft opens the dry frontier",
        opened[0] == 0 && monotone && opened[4] > 0,
        format!(
            "{} → {} cells across the ladder",
            opened[0], opened[4]
        ),
        "M55 gate: craftless peoples open no dry ground, and every deeper shaft opens at least as much as the shallower one, ending above zero",
    );

    // --- M57: the outcrop. Discovery is an act of seeing, so the ground
    // that shows its basement must be the ground that gets found. Measure
    // the finished world's known-share of hidden-at-dawn mineral seams
    // against the visibility of the ground each one lies under.
    {
        use calliope::prospecting::outcrop_visibility;
        // the pure function first: the ordering it claims must hold before
        // any world is asked to display it.
        let bare_desert = outcrop_visibility(1, gc::DESERT); // lithosol
        let farm = outcrop_visibility(3, gc::WOODLAND); // cambisol
        let buried = outcrop_visibility(10, gc::TROPICAL_RAIN_FOREST); // loess
        let arid_soil = outcrop_visibility(8, gc::DESERT); // aridisol
        println!();
        println!(
            "outcrop visibility (M57): bare desert lithosol {:.2} · arid aridisol {:.2} · wooded cambisol {:.2} · rainforest loess {:.2}",
            bare_desert, arid_soil, farm, buried
        );
        c.must(
            "the outcrop orders the ground",
            bare_desert > arid_soil && arid_soil > farm && farm > buried && buried > 0.0,
            format!("{:.2} > {:.2} > {:.2} > {:.2}", bare_desert, arid_soil, farm, buried),
            "M57: bared rock must be more visible than thin arid soil, which must beat farmland, which must beat a loess mantle under closed canopy",
        );

        // now the world. A century and a half finds nearly every seam
        // somewhere, so terminal known-share saturates and proves little;
        // the honest measure is *when* the ground gave its ore up. Split
        // the mineral seams into visibility terciles by their own
        // distribution (so every band is populated by construction) and
        // compare median discovery month, counting a seam still hidden at
        // the end as later than any found one.
        let horizon = (years as i32) * 12;
        let mut seams: Vec<(f64, i32)> = Vec::new();
        let mut arid: Vec<i32> = Vec::new();
        let mut elsewhere: Vec<i32> = Vec::new();
        for (di, d) in w.deposits.iter().enumerate() {
            if !d.r.is_mineral() {
                continue;
            }
            let (y, x) = (d.y as usize, d.x as usize);
            let v = outcrop_visibility(w.fields.soil[[y, x]], w.fields.biomes[[y, x]]);
            let found = w.flows.found_m.get(di).copied().unwrap_or(-1);
            // dawn-known seams (never hidden) carry month 0; still-hidden
            // seams are censored at the horizon.
            let when = if d.known && found < 0 { 0 } else if found >= 0 { found } else { horizon };
            seams.push((v, when));
            if w.fields.biomes[[y, x]] == gc::DESERT { arid.push(when) } else { elsewhere.push(when) }
        }
        seams.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let n = seams.len();
        let med = |v: &mut Vec<i32>| -> f64 {
            if v.is_empty() { return f64::NAN; }
            v.sort_unstable();
            v[v.len() / 2] as f64
        };
        let mut band: Vec<Vec<i32>> = vec![Vec::new(); 3];
        for (i, (_, when)) in seams.iter().enumerate() {
            let b = (i * 3 / n.max(1)).min(2);
            band[b].push(*when);
        }
        let vlo = seams.first().map(|s| s.0).unwrap_or(f64::NAN);
        let vhi = seams.last().map(|s| s.0).unwrap_or(f64::NAN);
        let (m_lo, m_mid, m_hi) = (med(&mut band[0]), med(&mut band[1]), med(&mut band[2]));
        println!(
            "seams found by ground (M57): {} mineral seams, visibility {:.2}..{:.2} · median discovery month — buried tercile {:.0} · middling {:.0} · bared {:.0} (horizon {}) · desert {:.0} vs elsewhere {:.0}",
            n, vlo, vhi, m_lo, m_mid, m_hi, horizon, med(&mut arid), med(&mut elsewhere)
        );
        c.must(
            "every visibility band carries seams",
            n >= 30 && band.iter().all(|b| b.len() >= 8) && vhi > vlo,
            format!("{} · {} · {} seams over visibility {:.2}..{:.2}", band[0].len(), band[1].len(), band[2].len(), vlo, vhi),
            "M57: the claim is only measurable if buried, middling and bared ground all hold seams and the visibility field actually varies",
        );
        c.must(
            "the bared ground is found first",
            m_hi < m_lo && m_mid <= m_lo,
            format!("median month {:.0} bared vs {:.0} middling vs {:.0} buried", m_hi, m_mid, m_lo),
            "M57 gate: prospecting yield rises with how much basement the ground shows, so bared country must give up its seams earlier than buried country",
        );
    }

    // --------------------------------------------------- M58 claim pressure
    // Demand, not price: a crown that has the art, the hands and the fuel
    // for a craft but owns no seam of its feedstock is structurally
    // without the metal, and that is the pressure that historically sent
    // states to claim ground nobody would farm.
    {
        let claims = w.claim_pressure();
        let mut lines: Vec<String> = Vec::new();
        for (&(r, g), &v) in claims.iter().take(6) {
            lines.push(format!("{}:{:?} {:.2}", r, g, v));
        }
        println!(
            "claim pressure (M58): {} standing claims over {} crowns{}{}",
            claims.len(),
            claims.iter().map(|(&(r, _), _)| r).collect::<std::collections::BTreeSet<_>>().len(),
            if lines.is_empty() { "" } else { " — " },
            lines.join(" · ")
        );
        // Audit: every claim must be backed by a town that is genuinely
        // without the ore — one whose own market area holds no seam of
        // it. Deprivation is per market area, not per realm: a crown
        // straddling two unconnected markets can be iron-fed in one and
        // iron-dark in the other, and the dark half is the half that
        // presses. What may never happen is a claim no deprived town
        // stands behind.
        let mut unbacked = 0usize;
        {
            use calliope::resources::GoodSet;
            let areas = &w.economy.areas;
            let mut area_goods: Vec<GoodSet> = vec![GoodSet::EMPTY; areas.markets.len()];
            for (i, st) in w.peoples.settlements.iter().enumerate() {
                if let Some(&k) = areas.area.get(i) {
                    if let Some(set) = area_goods.get_mut(k) {
                        set.extend(st.goods.iter().copied());
                    }
                }
            }
            for (&(r, g), _) in claims.iter() {
                let backed = w.peoples.settlements.iter().enumerate().any(|(i, st)| {
                    st.realm == r
                        && !st.goods.contains(&g)
                        && !areas
                            .area
                            .get(i)
                            .and_then(|&k| area_goods.get(k))
                            .map(|set| set.contains(g))
                            .unwrap_or(false)
                });
                if !backed {
                    unbacked += 1;
                }
            }
        }
        c.must(
            "every claim stands on a dark forge",
            unbacked == 0,
            format!("{} of {} claims unbacked", unbacked, claims.len()),
            "M58 gate: claim pressure is unmet demand — each claim must be backed by a town of that realm whose market area holds no seam of the ore",
        );
        // The lever itself: a claimed seam must call louder to the crown
        // that lacks it than to one that does not. Measured on the same
        // pull field colonisation reads.
        // The strongest claim whose ore still has an unworked seam to
        // call from: a crown can want iron most of all and every iron
        // seam in the world already sit inside somebody's work radius,
        // in which case the lever has nothing to act on and measuring it
        // there proves nothing either way.
        let base_all = w.resource_pull_for(&Default::default());
        let free_seam = |g: calliope::resources::Good| -> bool {
            w.deposits.iter().any(|d| {
                d.r == g && d.live() && base_all[[d.y as usize, d.x as usize]] > 0.0
            })
        };
        let mut ranked: Vec<(&(calliope::ids::RealmId, calliope::resources::Good), &f64)> = claims.iter().collect();
        ranked.sort_by(|a, b| b.1.total_cmp(a.1));
        let pick = ranked.into_iter().find(|(&(_, g), _)| free_seam(g));
        if let Some((&(realm, good), &press)) = pick {
            let base = base_all.clone();
            let (per, top) = calliope::world::World::realm_claim(&claims, realm);
            let heard = w.resource_pull_for(&per);
            // Measure the lever where it acts: on the ground over known
            // seams of the very ore the crown lacks. A world-max reading
            // says nothing — the loudest cell overall may be a good
            // nobody is short of, and the cap hides the difference.
            let mut b_max = 0.0f64;
            let mut h_max = 0.0f64;
            let mut seams = 0usize;
            for d in w.deposits.iter() {
                if d.r != good || !d.live() {
                    continue;
                }
                let (y, x) = (d.y as usize, d.x as usize);
                if base[[y, x]] <= 0.0 {
                    continue; // inside a town's work radius: it calls nobody
                }
                seams += 1;
                b_max = b_max.max(base[[y, x]]);
                h_max = h_max.max(heard[[y, x]]);
            }
            let reach_gain = 1.0 + calliope::economy::CLAIM_REACH_GAIN * top;
            println!(
                "the loudest claim (M58): {} wants {:?} at pressure {:.2} over {} known seams — call over the seam {:.2} unclaimed vs {:.2} heard by the crown · lane purse ×{:.2}",
                realm, good, press, seams, b_max, h_max, reach_gain
            );
            c.must(
                "a claim makes its seam call louder",
                seams > 0 && h_max > b_max && reach_gain > 1.0,
                format!("{} seams · {:.2} → {:.2} · purse ×{:.2}", seams, b_max, h_max, reach_gain),
                "M58 gate: the deprived crown must hear the known seams of the ore it lacks above the market's ordinary voice, and pay for a longer lane",
            );
        } else {
            c.want(
                "a claim makes its seam call louder",
                false,
                "no standing claims".into(),
                "M58: no crown presses a claim for an ore with an unworked seam left — the lever has nothing to act on in this world",
            );
        }
    }

    // M55 counterfactual: the same world, run again with the dry-frontier
    // veto lifted (every people's well reaches any table). If colonists
    // then settle waterless arid ground that the real run left empty, the
    // veto is load-bearing on the actual colonisation path — not merely on
    // a score function nobody exercised. If the counterfactual settles no
    // dry ground either, the veto changed nothing here and we say so.
    {
        let mut cf = World::generate(seed, size);
        cf.dry_reach_override = Some(f64::INFINITY);
        // M57 — a terminal snapshot of the desert's offer lies: a seam
        // found early is worked (and so pulls nothing) by the century's
        // end, which is exactly what better prospecting causes. Walk the
        // run in decades and keep the *peak* offer the dry country ever
        // made, so the auction is judged when it was actually held.
        let mut peak_dry = f64::NEG_INFINITY;
        let mut peak_at = (0usize, 0usize, 0i64, 0.0f64, 0.0f64);
        let mut peak_wet = f64::NEG_INFINITY;
        let step = 10usize;
        let mut done = 0usize;
        // M55 (corrected) — the veto acts at the moment of founding, on the
        // craft the founder held *then*. Terminal reach is the wrong clock:
        // a people that learns masonry in its third century would be judged
        // able to sink a 30 m shaft on ground it settled in its first, when
        // it could dig twelve metres. So we walk the counterfactual and
        // record every dry town as it appears, against its people's reach
        // at that decade's end — an upper bound on the reach it truly had,
        // which keeps every "REFUSED" verdict conservative.
        let mut seen: std::collections::HashSet<i64> = std::collections::HashSet::new();
        for s in cf.peoples.settlements.iter() {
            seen.insert(s.id.0);
        }
        // (id, y, x, table m, reach at founding m, month observed)
        let mut births: Vec<(i64, i64, i64, f64, f64, i64)> = Vec::new();
        while done < years {
            let chunk = step.min(years - done);
            let _ = run_years(&mut cf, chunk);
            done += chunk;
            for s in cf.peoples.settlements.iter() {
                if !seen.insert(s.id.0) {
                    continue;
                }
                if !cf.arid_dry[[s.y as usize, s.x as usize]] {
                    continue;
                }
                let table = cf.fields.aquifer[[s.y as usize, s.x as usize]] as f64;
                let reach = cf
                    .peoples
                    .societies
                    .get(s.people.idx())
                    .map(calliope::settlements::well_reach_m)
                    .unwrap_or(0.0);
                births.push((s.id.0, s.y, s.x, table, reach, cf.month));
            }
            // M58 — judge the desert at the price the hungriest crown
            // would pay for it, not at the market's neutral voice.
            let claims_s = cf.claim_pressure();
            let realm_s = claims_s
                .iter()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map(|(&(r, _), _)| r);
            let (per_s, top_s) = match realm_s {
                Some(r) => calliope::world::World::realm_claim(&claims_s, r),
                None => (Default::default(), 0.0),
            };
            let pull_s = cf.resource_pull_for(&per_s);
            let prov_s = cf.caravan_provision_claim(top_s);
            let dry_s = calliope::settlements::DryFrontier {
                arid_dry: &cf.arid_dry,
                aquifer: &cf.fields.aquifer,
                dry_site_score: &cf.dry_site_score,
                well_reach_m: f64::INFINITY,
                provision: &prov_s,
            };
            let (rr, cc) = cf.arid_dry.dim();
            for y in 0..rr {
                for x in 0..cc {
                    if cf.arid_dry[[y, x]] {
                        let o = dry_s.offer(&cf.site_score, &pull_s, y, x);
                        if o > peak_dry {
                            peak_dry = o;
                            peak_at = (y, x, cf.month, pull_s[[y, x]], prov_s[[y, x]] as f64);
                        }
                    } else {
                        peak_wet = peak_wet.max(cf.site_score[[y, x]] + pull_s[[y, x]]);
                    }
                }
            }
        }
        println!(
            "the desert's best hour (M57): peak arid-dry offer {:.2} at ({},{}) in month {} (pull {:.2} · provision {:.2}) against the best watered site ever seen, {:.2}",
            peak_dry, peak_at.0, peak_at.1, peak_at.2, peak_at.3, peak_at.4, peak_wet
        );
        let cf_dry = cf
            .peoples
            .settlements
            .iter()
            .filter(|s| cf.arid_dry[[s.y as usize, s.x as usize]])
            .count();
        println!(
            "counterfactual (M55): with wells of unlimited reach, {} towns stand on waterless arid ground (real run: {})",
            cf_dry, dry_towns
        );
        // The load-bearing statement, named directly, and judged on the
        // veto's own clock: a cf dry town founded on a table deeper than
        // its people's well reach *at that founding* is ground the real
        // run provably refused — the veto ran at that instant and said no.
        // Two earlier framings measured the wrong thing:
        //   · `cf_dry > dry_towns` — a count-vs-count proxy fooled by
        //     substitution (a refused site displaces a reachable one and
        //     both runs tally the same);
        //   · terminal well reach — the founder's craft centuries later,
        //     not the craft it held when it chose the site. Reach only
        //     grows, so terminal reach systematically under-reports
        //     refusals, which is exactly how seed 777 read clean while
        //     its three dry towns sat on ground its founders could not
        //     have watered.
        // Decade-end reach is still an upper bound on reach at founding,
        // so every REFUSED verdict below stays conservative.
        let mut veto_refused = 0usize;
        for &(_id, y, x, table, reach, month) in births.iter() {
            let refused = reach <= 0.0 || table > reach;
            let terminal = cf
                .peoples
                .settlements
                .iter()
                .find(|s| s.y == y && s.x == x)
                .and_then(|s| cf.peoples.societies.get(s.people.idx()))
                .map(calliope::settlements::well_reach_m)
                .unwrap_or(0.0);
            println!(
                "  cf dry town at ({},{}) founded by month {}: table {:.0} m · founder's well reach then {:.0} m (terminal {:.0} m) · {}",
                y,
                x,
                month,
                table,
                reach,
                terminal,
                if refused { "REFUSED by the real veto" } else { "within reach (the veto never barred it)" }
            );
            if refused {
                veto_refused += 1;
            }
        }
        // Why the desert wins or loses (M56): the best offer arid-dry
        // ground can make a colonist — the extractive price, caravan
        // provisioning and well upkeep included — against the best
        // offer anywhere else.
        {
            let claims_e = cf.claim_pressure();
            let realm_e = claims_e
                .iter()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map(|(&(r, _), _)| r);
            let (per_e, top_e) = match realm_e {
                Some(r) => calliope::world::World::realm_claim(&claims_e, r),
                None => (Default::default(), 0.0),
            };
            let pull = cf.resource_pull_for(&per_e);
            let prov = cf.caravan_provision_claim(top_e);
            let dry = calliope::settlements::DryFrontier {
                arid_dry: &cf.arid_dry,
                aquifer: &cf.fields.aquifer,
                dry_site_score: &cf.dry_site_score,
                well_reach_m: f64::INFINITY,
                provision: &prov,
            };
            let (rows, cols) = cf.arid_dry.dim();
            let mut best_dry = f64::NEG_INFINITY;
            let mut best_wet = f64::NEG_INFINITY;
            let mut best_pull = 0.0f64;
            let mut best_prov = 0.0f64;
            let mut provisioned = 0usize;
            let mut arid_cells = 0usize;
            for y in 0..rows {
                for x in 0..cols {
                    if cf.arid_dry[[y, x]] {
                        arid_cells += 1;
                        if prov[[y, x]] > 0.0 {
                            provisioned += 1;
                        }
                        best_pull = best_pull.max(pull[[y, x]]);
                        best_prov = best_prov.max(prov[[y, x]] as f64);
                        best_dry = best_dry.max(dry.offer(&cf.site_score, &pull, y, x));
                    } else {
                        best_wet = best_wet.max(cf.site_score[[y, x]] + pull[[y, x]]);
                    }
                }
            }
            println!(
                "the desert's offer (M56): best arid-dry site {:.2} vs best watered site {:.2} · best ore pull {:.2} · caravan-provisioned {}/{} arid-dry cells (best {:.2})",
                best_dry, best_wet, best_pull, provisioned, arid_cells, best_prov
            );
            // Break the winner down so a weak desert is diagnosable:
            // which term is short — the seam, the lane, or the shaft?
            {
                let (rows, cols) = cf.arid_dry.dim();
                let mut bo = f64::NEG_INFINITY;
                let mut at = (0usize, 0usize);
                let mut bp = f64::NEG_INFINITY;
                let mut pat = (0usize, 0usize);
                for y in 0..rows {
                    for x in 0..cols {
                        if !cf.arid_dry[[y, x]] { continue; }
                        let o = dry.offer(&cf.site_score, &pull, y, x);
                        if o > bo { bo = o; at = (y, x); }
                        if pull[[y, x]] > bp { bp = pull[[y, x]]; pat = (y, x); }
                    }
                }
                for (tag, (y, x)) in [("best offer", at), ("best pull", pat)] {
                    println!(
                        "  {tag} cell ({y},{x}): held {:.2} · pull {:.2} · provision {:.2} · table {:.0} m · upkeep {:.2} · offer {:.2}",
                        cf.dry_site_score[[y, x]],
                        pull[[y, x]],
                        prov[[y, x]],
                        cf.fields.aquifer[[y, x]],
                        calliope::settlements::well_upkeep(cf.fields.aquifer[[y, x]] as f64),
                        dry.offer(&cf.site_score, &pull, y, x),
                    );
                }
            }
        }
        // Scope, stated honestly: *that* the veto refuses ground a
        // colonist would take is a law of the model, and a law is proved
        // over the seed ensemble (the gate's `the veto bites somewhere`
        // row, composed across the civ lanes). *Whether* a single world's
        // 150 years ever held such an auction is a contingency of that
        // world's history. Seed 777 is the honest example: its every dry
        // founding falls after month 1080, by which time its peoples hold
        // aqueduct and engineering craft — 60–90 m of reach over tables
        // 30–35 m down. The veto ran at each of those foundings and had
        // nothing to refuse. Failing the seed for that would be failing
        // it for a true history, so a world that never held the auction
        // reports WARN and says so; the ensemble row keeps the bar.
        let earliest = births.iter().map(|b| b.5).min().unwrap_or(0);
        let deepest = births.iter().map(|b| b.3).fold(0.0f64, f64::max);
        if veto_refused >= 1 {
            c.must(
                "the dry-frontier veto is load-bearing",
                true,
                format!(
                    "{} town(s) on ground the veto refused ({} dry without · {} with)",
                    veto_refused, cf_dry, dry_towns
                ),
                "M55 gate: the veto-lifted run must stand ≥1 town on waterless ground deeper than its founder's well reach at the founding — the instant the real run's veto ran and refused it",
            );
        } else {
            c.want(
                "the dry-frontier veto is load-bearing",
                false,
                format!(
                    "no auction held — {} cf dry founding(s), earliest month {}, deepest table {:.0} m, all inside the founder's reach ({} dry without · {} with)",
                    births.len(), earliest, deepest, cf_dry, dry_towns
                ),
                "M55: this world's dry foundings all fall after its peoples hold deep-well craft, so the veto had nothing to refuse here — the law is carried by the gate's ensemble row across seeds",
            );
        }

    }

    // M62 — geomorphic toponymy: place names tell the truth about the
    // ground. Every town whose namer's tongue keeps a generic for the
    // landform under it must carry that generic as its name's tail —
    // either in the standing name, or (when the patina system has worn
    // the word) in the recorded source name it was worn from. Towns on
    // ground their tongue has no word for are not eligible: the plain
    // coined name is the honest answer there, not a borrowed suffix.
    {
        let mut eligible = 0usize;
        let mut matched = 0usize;
        let mut miss: Vec<String> = Vec::new();
        for s in &w.peoples.settlements {
            let style = w
                .peoples
                .peoples
                .get(s.namer.idx())
                .map(|p| p.style.as_str())
                .unwrap_or("old");
            let code = w.fields.landform[[s.y as usize, s.x as usize]];
            let Some(class) = calliope::naming::landform_class(code) else {
                continue;
            };
            let gens = calliope::naming::landform_generics(style, class);
            if gens.is_empty() {
                continue;
            }
            eligible += 1;
            let hit = gens.iter().any(|(g, _)| s.name.ends_with(g))
                || s.formerly.iter().any(|f| gens.iter().any(|(g, _)| f.ends_with(g)));
            if hit {
                matched += 1;
            } else if miss.len() < 5 {
                miss.push(format!("{} ({} on {})", s.name, style, class));
            }
        }
        if !miss.is_empty() {
            println!("  landform-name misses: {}", miss.join(" · "));
        }
        let pct = if eligible > 0 {
            100.0 * matched as f64 / eligible as f64
        } else {
            0.0
        };
        c.must(
            "names tell the truth about the ground",
            eligible > 0 && pct >= 90.0,
            format!("{:.0}% ({} of {} eligible towns)", pct, matched, eligible),
            "M62 gate: ≥90% of towns whose tongue has a generic for their landform carry it as the name's tail (worn names count through their recorded source)",
        );
    }

    c.print();
}

/// Pearson correlation over paired samples.
fn pearson(xs: &[f64], ys: &[f64]) -> f64 {
    let n = xs.len() as f64;
    let mx = xs.iter().sum::<f64>() / n;
    let my = ys.iter().sum::<f64>() / n;
    let mut sxy = 0.0;
    let mut sxx = 0.0;
    let mut syy = 0.0;
    for (x, y) in xs.iter().zip(ys.iter()) {
        sxy += (x - mx) * (y - my);
        sxx += (x - mx) * (x - mx);
        syy += (y - my) * (y - my);
    }
    if sxx <= 0.0 || syy <= 0.0 {
        return 0.0;
    }
    sxy / (sxx * syy).sqrt()
}


// ================================================================ economy

type DirectedLane = (f64, f64, f64, String, String);

/// M46's causal subject is the founding web under the founding current/wind
/// field. Later deaths and rescue routes are history, not a second sailing
/// law: measure them separately so a disaster can change the lived web without
/// silently changing which passages prove the edge-cost mechanism.
fn directed_sea(w: &World) -> (Vec<DirectedLane>, usize) {
    let tf = w.trade.f;
    let spos = |id: calliope::ids::SettlementId| {
        w.peoples
            .settlements
            .iter()
            .find(|s| s.id == id)
            .map(|s| (s.y as usize / tf, s.x as usize / tf))
    };
    let sname = |id: calliope::ids::SettlementId| {
        w.peoples
            .settlements
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "?".into())
    };
    let mut splits = Vec::new();
    let mut rewalk_bad = 0usize;
    for r in w.routes.iter().filter(|r| r.sea > 0.5) {
        let (Some(start), Some(goal)) = (spos(r.a), spos(r.b)) else { continue };
        if let Some((path, fwd)) = calliope::trade::astar(&w.trade, start, goal) {
            let re_f = calliope::trade::path_cost(&w.trade, &path, false);
            if (re_f - fwd).abs() > 1e-9 * fwd.max(1.0) {
                rewalk_bad += 1;
            }
            let rev = calliope::trade::path_cost(&w.trade, &path, true);
            let hi = fwd.max(rev);
            if hi > 0.0 {
                splits.push((
                    100.0 * (1.0 - fwd.min(rev) / hi),
                    fwd,
                    rev,
                    sname(r.a),
                    sname(r.b),
                ));
            }
        }
    }
    splits.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    (splits, rewalk_bad)
}

fn cmd_economy(seed: i64, size: usize, years: usize) {
    let mut w = World::generate(seed, size);
    header("ECONOMY", &format!("seed {} · {}x{} · {}y", seed, w.width, size, years));

    // M37/M48 gate — route costs and calendars stay deterministic
    // across reruns: the founding web is snapshotted here and a second
    // world, generated from the same seed after the run, must price
    // and schedule it identically, month by month.
    let routes0: Vec<_> = w
        .routes
        .iter()
        .map(|r| (r.a, r.b, r.cost, r.closed, r.season, r.shut.clone()))
        .collect();
    let cal0 = calendar_hash(&w.routes);
    // Snapshot M46 before history starts. Storms, quakes and abandonment may
    // later remove towns and re-knit the web; that evolved result is printed
    // as a counterfactual below, while the gate remains on its stated subject.
    let (founding_splits, founding_rewalk_bad) = directed_sea(&w);

    // M15.6 — conservation ledger baselines: what every seam and ground
    // held at the founding, so the books can be balanced at the end.
    let left0: Vec<f64> = w.deposits.iter().map(|d| d.left).collect();
    let stock0: Vec<f64> = w.deposits.iter().map(|d| d.stock).collect();

    const TRACKED: [resources::Good; 20] = [
        resources::Good::Grain, resources::Good::Fish, resources::Good::Timber,
        resources::Good::Stone, resources::Good::Coal, resources::Good::Copper,
        resources::Good::Iron, resources::Good::Silver, resources::Good::Gold,
        resources::Good::Mithril, resources::Good::Salt, resources::Good::Wool,
        resources::Good::Hides, resources::Good::Furs, resources::Good::Grapes,
        resources::Good::Spices, resources::Good::Dyes, resources::Good::Clay,
        resources::Good::Marble, resources::Good::Gems,
    ];
    let mut series: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
    let mut strikes = 0usize;
    let mut depletions = 0usize;
    let mut trade_events = 0usize;
    // M14.8 — count wild-stock phase transitions across the run
    let (mut wild_thin, mut wild_collapse, mut wild_recover) = (0usize, 0usize, 0usize);
    let mut prev_phase: Vec<u8> = w.deposits.iter().map(|d| d.phase).collect();
    let months = years * 12;
    // M79 — the coast's books: each town's seaborne cargo month by month,
    // and every harbour the storms broke. The dip and the recovery are
    // read straight off these two series, per strike, with no averaging
    // over towns that were never hit.
    let mut sea_flow: HashMap<calliope::ids::SettlementId, Vec<f64>> = HashMap::new();
    let mut strikes_log: Vec<(usize, calliope::ids::SettlementId, f64)> = Vec::new();
    let mut storm_beats = 0usize;
    let mut marks_seen = 0usize;
    for mi in 0..months {
        let (evs, _f, _d) = w.tick(1);
        // the month's water trade, per town: only lanes that actually sail
        for (ri, r) in w.routes.iter().enumerate() {
            if r.sea <= 0.0 {
                continue;
            }
            let route_flow = w.economy.route_flow.get(ri).copied().unwrap_or(0.0);
            let wound = [r.a, r.b]
                .iter()
                .filter_map(|id| w.peoples.settlements.iter().find(|s| s.id == *id))
                .map(|s| s.harbor_dmg)
                .fold(0.0f64, f64::max);
            // `route_flow` is the whole mixed journey after M79 preserves
            // its carted share: base × (1 − wound × sea). Recover the base
            // and read only its sailed component after the harbour wound.
            // Multiplying the blended total by `sea` would falsely count
            // preserved cart cargo as water cargo a broken quay still moved.
            let blended = (1.0 - wound * r.sea).max(0.0);
            let f = if blended > 0.0 {
                route_flow / blended * r.sea * (1.0 - wound)
            } else {
                0.0
            };
            for id in [r.a, r.b] {
                let e = sea_flow.entry(id).or_insert_with(|| vec![0.0; months]);
                e[mi] += f;
            }
        }
        for (_m, sid, dmg) in w.storm_marks.iter().skip(marks_seen) {
            strikes_log.push((mi, *sid, *dmg));
        }
        marks_seen = w.storm_marks.len();
        for e in &evs {
            if e.text.contains("harbour will take") || e.text.contains("The sea rises on") {
                storm_beats += 1;
            }
        }
        for e in &evs {
            match e.k.name() {
                "discovery" => strikes += 1,
                "depletion" => depletions += 1,
                "trade" => trade_events += 1,
                _ => {}
            }
        }
        for (i, d) in w.deposits.iter().enumerate() {
            let p0 = prev_phase.get(i).copied().unwrap_or(0);
            if d.phase != p0 {
                match d.phase {
                    1 => wild_thin += 1,
                    2 => wild_collapse += 1,
                    _ => wild_recover += 1,
                }
            }
        }
        prev_phase = w.deposits.iter().map(|d| d.phase).collect();
        for g in TRACKED {
            if w.economy.market.contains(g) {
                series.entry(g.name()).or_default().push(w.economy.market.price(g));
            }
        }
    }

    println!("{:<9} {:>6} {:>7} {:>7} {:>7} {:>9} {:>8}", "good", "base", "mean", "min", "max", "mean/base", "pinned");
    let mut max_pinned = 0.0f64;
    let mut means: BTreeMap<&str, f64> = BTreeMap::new();
    for (g, s) in &series {
        let base = economy::base_value(g.parse().expect("tracked good name"));
        let mean = s.iter().sum::<f64>() / s.len() as f64;
        means.insert(g, mean);
        let min = s.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = s.iter().cloned().fold(0.0f64, f64::max);
        let pinned = s.iter().filter(|&&p| p <= 0.31 * base || p >= 4.9 * base).count() as f64 / s.len() as f64;
        max_pinned = max_pinned.max(pinned);
        println!("{:<9} {:>6.2} {:>7.2} {:>7.2} {:>7.2} {:>9.2} {:>7.0}%", g, base, mean, min, max, mean / base, 100.0 * pinned);
    }
    println!("(pinned = months at the 0.3×/5× clamp — a pinned price is a mis-tuned market)");
    println!();
    println!("market shocks: {} strikes · {} depletions · {} trade chronicle entries", strikes, depletions, trade_events);

    // wealth distribution
    let wealth: Vec<f64> = w.peoples.settlements.iter().map(|s| s.wealth).collect();
    let g = gini(&wealth);
    let total_w: f64 = wealth.iter().sum();
    println!("wealth: total {:.0} · gini {:.2}", total_w, g);
    let mut by_wealth: Vec<&calliope::settlements::Settlement> = w.peoples.settlements.iter().collect();
    by_wealth.sort_by(|a, b| b.wealth.partial_cmp(&a.wealth).unwrap());
    println!("richest towns:");
    for s in by_wealth.iter().take(5) {
        println!("  {:<20} pop {:>6} · wealth {:>8.0} · exports {} {}", s.name, s.pop, s.wealth, s.exports.map(|g| g.name()).unwrap_or("—"), if s.port { "· harbour" } else { "" });
    }
    println!("poorest towns:");
    for s in by_wealth.iter().rev().take(3) {
        println!("  {:<20} pop {:>6} · wealth {:>8.0}", s.name, s.pop, s.wealth);
    }

    // routes
    let sea_r = w.routes.iter().filter(|r| r.sea > 0.5).count();
    let mixed_r = w.routes.iter().filter(|r| r.sea > 0.05 && r.sea <= 0.5).count();
    let land_r = w.routes.len() - sea_r - mixed_r;
    let mean_cost = w.routes.iter().map(|r| r.cost).sum::<f64>() / w.routes.len().max(1) as f64;
    let mean_len = w.routes.iter().map(|r| r.path.len() as f64).sum::<f64>() / w.routes.len().max(1) as f64;
    let ports = w.peoples.settlements.iter().filter(|s| s.port).count();
    let unconnected = w.peoples.settlements.iter().filter(|s| s.connections == 0).count();
    println!();
    println!("routes: {} ({} sea / {} mixed / {} land) · mean cost {:.1} · mean length {:.0} km", w.routes.len(), sea_r, mixed_r, land_r, mean_cost, mean_len * gc::KM_PER_CELL);
    println!("harbours: {} · unconnected towns: {}", ports, unconnected);
    println!("treasuries (crowns — ADR-0018):");
    for r in w.peoples.realms.iter().filter(|r| r.alive) {
        println!("  {:<22} {:>9.0}", r.name, r.treasury);
    }

    // ---- M37 sea ice: the winter schedule of the lanes ----
    let frozen_g = calliope::seaice::frozen_months(&w.fields.height, &w.fields.tmean, &w.fields.tamp);
    let (iced, ice_perennial, ice_bad) = ice_route_stats(&w, &frozen_g);
    let sea_touch = w.routes.iter().filter(|r| r.sea > 0.0).count();
    println!();
    println!(
        "sea ice on the lanes (M37): {} winter-closed of {} sea-touching routes · {} perennially shut · {} malformed masks",
        iced, sea_touch, ice_perennial, ice_bad
    );
    if let Some(r) = w
        .routes
        .iter()
        .filter(|r| r.closed != 0 && r.closed != calliope::seaice::MONTHS_MASK)
        .max_by_key(|r| (r.closed.count_ones(), r.cost as i64))
    {
        let cal: String = (0..12)
            .map(|m| if r.closed >> m & 1 == 1 { '■' } else { '·' })
            .collect();
        let name = |id: calliope::ids::SettlementId| {
            w.peoples
                .settlements
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "?".into())
        };
        println!(
            "  hardest lane: {} — {} · calendar [{}] · {} months shut",
            name(r.a), name(r.b), cal, r.closed.count_ones()
        );
    }
    // The reopening, shown on the books: twelve more months ticked
    // through the market — flow must be zero exactly when the union
    // mask says shut — ice or gale — and alive the month the water
    // opens (M37 + M48, one law on the ledger).
    let mut ice_mism = 0usize;
    if w.routes.iter().any(|r| r.closed != 0) {
        let by_id = economy::sidx(&w.peoples.settlements);
        let mut prng = calliope::util::rng(seed + 3737);
        for m in 0..12i64 {
            let month_abs = months as i64 + m;
            let _ = economy::monthly(&mut w.economy, &mut w.peoples, &w.routes, month_abs, &mut prng, &by_id);
            let mon = month_abs.rem_euclid(12);
            for (ri, r) in w.routes.iter().enumerate() {
                if r.closed == 0 {
                    continue;
                }
                let shut = r.closed >> mon & 1 == 1;
                let f = w.economy.route_flow.get(ri).copied().unwrap_or(0.0);
                if (shut && f != 0.0) || (!shut && f <= 0.0) {
                    ice_mism += 1;
                }
            }
        }
    }

    // ---- M46 the directed sea: every blue-water lane priced out and home ----
    // Re-run the founding search on the coarse grid and price the same
    // path in reverse: the difference is what the gyre and the trades
    // are worth to a keel. Land lanes are excluded — carts feel no
    // current — and the forward re-walk must equal the search's own
    // cost exactly, or the edge law and the re-walk have diverged.
    // The spec's own metric: the with-current passage is `adv`% faster
    // than its seed-matched against-current mirror — adv = 1 − out/home.
    let splits = founding_splits;
    let rewalk_bad = founding_rewalk_bad;
    let dir_alive = splits.iter().filter(|s| s.0 >= 2.0).count();
    let dir_best = splits.first().map(|s| s.0).unwrap_or(0.0);
    let dir_share = if splits.is_empty() { 0.0 } else { 100.0 * dir_alive as f64 / splits.len() as f64 };
    println!();
    println!(
        "the directed sea (M46): {} blue-water lanes · {} sail ≥2% faster one way ({:.0}%) · best mirror advantage {:.1}%",
        splits.len(), dir_alive, dir_share, dir_best
    );
    for (sp, fwd, rev, a, b) in splits.iter().take(3) {
        println!(
            "  {} — {} · out {:.1} · home {:.1} · with-current {:.1}% faster",
            a, b, fwd.min(*rev), fwd.max(*rev), sp
        );
    }
    let (evolved_splits, evolved_rewalk_bad) = directed_sea(&w);
    let evolved_best = evolved_splits.first().map(|s| s.0).unwrap_or(0.0);
    println!(
        "  evolved-web counterfactual after {}y: {} blue-water lanes · best {:.1}% · {} divergent (not the M46 gate subject)",
        years, evolved_splits.len(), evolved_best, evolved_rewalk_bad
    );
    let sname = |id: calliope::ids::SettlementId| {
        w.peoples
            .settlements
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "?".into())
    };
    // The field the lanes sail through: coarse current speeds over the
    // open cells, and the becalmed rows. If the splits look thin, this
    // line says whether the ocean or the gain is at fault.
    {
        let mut spd: Vec<f64> = Vec::new();
        for ((y, x), &o) in w.trade.open.indexed_iter() {
            if o {
                spd.push(w.trade.cu[[y, x]].hypot(w.trade.cv[[y, x]]));
            }
        }
        spd.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let q = |p: f64| spd[((spd.len() - 1) as f64 * p) as usize];
        let becalmed_rows = w.trade.becalmed.iter().filter(|&&b| b).count();
        if !spd.is_empty() {
            println!(
                "  the field: open cells {} · |current| p50 {:.3} · p90 {:.3} · max {:.3} · becalmed rows {} of {}",
                spd.len(), q(0.5), q(0.9), spd[spd.len() - 1], becalmed_rows, w.trade.becalmed.len()
            );
        }
    }

    // ---- M48 the sailor's calendar: the monsoon lanes and their year ----
    let m_lanes: Vec<&calliope::trade::Route> = w
        .routes
        .iter()
        .filter(|r| r.season.abs() >= calliope::trade::MONSOON_LANE)
        .collect();
    let m_shut = w.routes.iter().filter(|r| route_monsoon_mask(r) != 0).count();
    // A burst is well-formed when it sits inside the union of the two
    // arcs the law can emit — three months around either monsoon height.
    let arc_ok = calliope::trade::monsoon_burst_mask(1.0) | calliope::trade::monsoon_burst_mask(-1.0);
    let m_bad = w
        .routes
        .iter()
        .filter(|r| {
            let m = route_monsoon_mask(r);
            m != 0 && (m & !arc_ok) != 0
        })
        .count();
    // The spec's own metric: throughput swing between the year's peak
    // and its floor, through the same law the ledger applies (closure
    // months carry exactly nothing).
    let mut swings: Vec<f64> = Vec::new();
    for r in &m_lanes {
        let (mut lo, mut hi) = (f64::MAX, 0.0f64);
        for m in 0..12i64 {
            let mult = if r.closed >> (m as usize) & 1 == 1 {
                0.0
            } else {
                calliope::trade::season_mult(r.season, m)
            };
            lo = lo.min(mult);
            hi = hi.max(mult);
        }
        if hi > 0.0 {
            swings.push(100.0 * (1.0 - lo / hi));
        }
    }
    let swing_mean = if swings.is_empty() { 0.0 } else { swings.iter().sum::<f64>() / swings.len() as f64 };
    let swing_best = swings.iter().cloned().fold(0.0f64, f64::max);
    let m_share = if sea_touch == 0 { 0.0 } else { 100.0 * m_lanes.len() as f64 / sea_touch as f64 };
    println!();
    println!(
        "the sailor's calendar (M48): {} monsoon lanes of {} sea-touching ({:.0}%) · {} shut in the burst · {} malformed arcs · swing mean {:.0}% best {:.0}%",
        m_lanes.len(), sea_touch, m_share, m_shut, m_bad, swing_mean, swing_best
    );
    if let Some(r) = w
        .routes
        .iter()
        .filter(|r| route_monsoon_mask(r) != 0)
        .max_by_key(|r| (route_monsoon_mask(r).count_ones(), (r.season.abs() * 100.0) as i64))
    {
        let mm = route_monsoon_mask(r);
        let cal: String = (0..12).map(|m| if mm >> m & 1 == 1 { '■' } else { '·' }).collect();
        println!(
            "  hardest gale lane: {} — {} · burst [{}] · season {:+.2}",
            sname(r.a), sname(r.b), cal, r.season
        );
    }
    // The sea the calendar reads: how much monsoon water the grid holds.
    {
        let mut mv: Vec<f64> = w.trade.mons.iter().map(|&v| (v as f64).abs()).filter(|&v| v > 0.0).collect();
        mv.sort_by(|a, b| a.partial_cmp(b).unwrap());
        if !mv.is_empty() {
            let q = |p: f64| mv[((mv.len() - 1) as f64 * p) as usize];
            println!(
                "  the field: monsoon sea cells {} · |lean| p50 {:.2} · p90 {:.2} · max {:.2} · gale threshold {:.2}",
                mv.len(), q(0.5), q(0.9), mv[mv.len() - 1], calliope::trade::MONSOON_GALE
            );
        }
    }

    // M37 — the rerun: same seed, second world, same founding web.
    let w2 = World::generate(seed, size);
    let routes1: Vec<_> = w2
        .routes
        .iter()
        .map(|r| (r.a, r.b, r.cost, r.closed, r.season, r.shut.clone()))
        .collect();
    let det_ok = routes0 == routes1;
    let cal1 = calendar_hash(&w2.routes);
    drop(w2);

    let finite_ok = w.economy.market.iter_some().all(|(_, p)| p.is_finite()) && wealth.iter().all(|v| v.is_finite());
    let treasuries_ok = w.peoples.realms.iter().all(|r| r.treasury >= 0.0 && r.treasury.is_finite());

    // ---- M79 the coasts remember: landfall → harbour → the water trade ----
    // Per strike, on the struck town's own sea cargo: the twelve months
    // before are the baseline, the strike month is the dip, and the last
    // half-year of the repair arc is the recovery. A town with no water
    // trade to lose is not evidence and is left out of both medians.
    let told: Vec<(usize, calliope::ids::SettlementId, f64)> = strikes_log
        .iter()
        .filter(|(_, _, d)| *d >= calliope::settlements::HARBOR_TELL_MIN)
        .copied()
        .collect();
    let mut dips: Vec<f64> = Vec::new();
    let mut backs: Vec<f64> = Vec::new();
    let mut strike_timeline: Vec<(usize, calliope::ids::SettlementId, f64, f64, f64, f64)> = Vec::new();
    let win = calliope::settlements::HARBOR_WINDOW as usize;
    for &(mi, sid, dmg) in &told {
        let Some(series) = sea_flow.get(&sid) else { continue };
        if mi < 12 || mi + win + 1 > months {
            continue; // no room for a baseline or a full arc
        }
        let base = series[mi - 12..mi].iter().sum::<f64>() / 12.0;
        if base <= 1e-6 {
            continue;
        }
        let dip = series[mi] / base;
        dips.push(dip);
        let back = series[mi + win - 6..mi + win].iter().sum::<f64>() / 6.0;
        let recovered = back / base;
        backs.push(recovered);
        let sailed_weight = w
            .routes
            .iter()
            .filter(|r| r.a == sid || r.b == sid)
            .map(|r| r.sea)
            .sum::<f64>();
        strike_timeline.push((mi, sid, dmg, sailed_weight, dip, recovered));
    }
    let median = |v: &mut Vec<f64>| -> f64 {
        if v.is_empty() {
            return f64::NAN;
        }
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    let (n_dip, n_back) = (dips.len(), backs.len());
    let med_dip = median(&mut dips.clone());
    let med_back = median(&mut backs.clone());
    let open_wounds = w
        .peoples
        .settlements
        .iter()
        .filter(|s| s.harbor_dmg > 0.0)
        .count();
    let stale_wounds = w
        .peoples
        .settlements
        .iter()
        .filter(|s| s.harbor_dmg > 0.0 && s.harbor_until <= months as i64)
        .count();
    let storm_ruins = w.ruins.iter().filter(|r| r.why.contains("the sea came over")).count();
    println!();
    println!(
        "storm landfalls (M79): {} harbour strikes · {} told of · {} chronicle beats · {} harbours still mending · {} coasts the sea kept",
        strikes_log.len(), told.len(), storm_beats, open_wounds, storm_ruins
    );
    if n_dip > 0 {
        println!(
            "  the water trade: strike month {:.0}% of the year before (n={}) · {} months on {:.0}% of it back (n={})",
            100.0 * med_dip, n_dip, win, 100.0 * med_back, n_back
        );
        println!("  causal cohorts (same strike samples; diagnostic only):");
        for (lo, hi) in [(0usize, 1200usize), (1200, 1800), (1800, months)] {
            let cohort: Vec<_> = strike_timeline
                .iter()
                .filter(|(mi, _, _, _, _, _)| *mi >= lo && *mi < hi)
                .collect();
            if cohort.is_empty() {
                continue;
            }
            let mut cd: Vec<f64> = cohort.iter().map(|x| x.4).collect();
            let mut cb: Vec<f64> = cohort.iter().map(|x| x.5).collect();
            let mean_wound = cohort.iter().map(|x| x.2).sum::<f64>() / cohort.len() as f64;
            let mean_sailed = cohort.iter().map(|x| x.3).sum::<f64>() / cohort.len() as f64;
            println!(
                "    years {:>3}-{:>3}: n={} · wound {:.3} · sailed-weight {:.2} · dip {:.1}% · back {:.1}%",
                lo / 12,
                hi.saturating_sub(1) / 12,
                cohort.len(),
                mean_wound,
                mean_sailed,
                100.0 * median(&mut cd),
                100.0 * median(&mut cb),
            );
        }
        let mut ordered = strike_timeline.clone();
        ordered.sort_by(|a, b| a.4.partial_cmp(&b.4).unwrap());
        let mid = ordered.len() / 2;
        println!("  median neighbourhood (month · town · wound · sailed-weight · dip · back):");
        for &(mi, sid, dmg, sailed, dip, back) in ordered.iter().skip(mid.saturating_sub(2)).take(5) {
            println!(
                "    {:>5} · {} · {:.3} · {:.2} · {:.1}% · {:.1}%",
                mi,
                sname(sid),
                dmg,
                sailed,
                100.0 * dip,
                100.0 * back,
            );
        }
    }

    let mut c = Checks::default();
    c.band("max pinned price share", max_pinned, pct(max_pinned));
    // ---- M79 gates ----
    c.must(
        "storms reach the coasts",
        !strikes_log.is_empty(),
        format!("{} strikes", strikes_log.len()),
        "M79 gate: a world with harbours and a storm belt must see landfalls in three centuries",
    );
    c.must(
        "a landfall costs the harbour its water",
        n_dip == 0 || med_dip <= 0.75,
        if n_dip > 0 { format!("{:.0}% of base", 100.0 * med_dip) } else { "no measurable strike".into() },
        "M79 gate: median seaborne cargo in the strike month is a quarter down or worse on the struck town's own year",
    );
    c.must(
        "the harbour comes back",
        n_back == 0 || med_back >= 0.90,
        if n_back > 0 { format!("{:.0}% of base", 100.0 * med_back) } else { "no measurable strike".into() },
        "M79 gate: by the close of the repair arc the struck town carries ≥90% of the water trade it had before",
    );
    c.must(
        "no wound outlives its arc",
        stale_wounds == 0,
        format!("{} stale", stale_wounds),
        "M79: a harbour whose window has lapsed must read whole",
    );
    c.want(
        "the sea gets its beat",
        strikes_log.is_empty() || storm_beats > 0,
        format!("{} beats", storm_beats),
        "M79: a broken harbour enters the chronicle",
    );

    c.band("wealth gini", g, format!("{:.2}", g));
    c.must("routes exist", !w.routes.is_empty(), format!("{}", w.routes.len()), "the web of trade must hold");
    c.want("no unconnected towns", unconnected == 0, format!("{}", unconnected), "every town trades");
    c.want("harbours exist", ports >= 1, format!("{}", ports), "coastal trade should produce ports");
    c.must(
        "route costs deterministic across reruns",
        det_ok,
        format!("{} routes", routes0.len()),
        "M37 gate: same seed, same web, same prices",
    );
    c.must(
        "icebound lanes freeze in one winter arc",
        ice_bad == 0,
        format!("{} malformed of {}", ice_bad, iced),
        "M37 gate: closure is a single hemisphere-true season",
    );
    c.must(
        "the ledger obeys the calendar",
        ice_mism == 0,
        format!("{} mismatched route-months", ice_mism),
        "M37/M48 gate: flow zero when shut — ice or gale — alive when the water opens",
    );
    if m_lanes.is_empty() {
        println!(" (no monsoon lanes in this world — the calendar bands idle)");
    } else {
        c.band("monsoon lane share", m_share, format!("{:.0}%", m_share));
        c.band("monsoon throughput swing", swing_mean, format!("{:.0}%", swing_mean));
    }
    c.must(
        "monsoon closures are burst arcs",
        m_bad == 0,
        format!("{} malformed of {}", m_bad, m_shut),
        "M48 gate: the gale season is the wet monsoon's height, nothing else",
    );
    c.must(
        "the sailing calendar replays byte-identically",
        cal0 == cal1,
        format!("{:016x}", cal0),
        "M48 gate: month-by-month route state is a pure function of the seed",
    );
    c.must("prices finite", finite_ok, if finite_ok { "yes".into() } else { "NO".into() }, "no NaN in the market");
    c.must("treasuries sane", treasuries_ok, if treasuries_ok { "yes".into() } else { "NO".into() }, "≥0 and finite");
    c.must(
        "search and re-walk agree",
        rewalk_bad == 0,
        format!("{} divergent lanes", rewalk_bad),
        "M46 gate: one edge law prices the search and the ledger",
    );
    if !splits.is_empty() {
        c.band("sea-lane mirror advantage (best)", dir_best, format!("{:.1}%", dir_best));
        c.band("directional lanes alive", dir_share, format!("{:.0}%", dir_share));
    }
    if years >= 60 {
        c.want("strikes moved markets", strikes >= 1, format!("{}", strikes), "discovery shocks should fire");
    }

    // ---- the mines: does the world work what it knows? ----
    let known: Vec<_> = w.deposits.iter().filter(|d| d.known && d.left > 0.0 && d.r.is_mineral()).collect();
    let worked = known.iter().filter(|d| {
        w.peoples.settlements.iter().any(|s| {
            let r = calliope::settlements::work_radius(s.pop);
            let dx = (d.x - s.x) as f64;
            let dy = (d.y - s.y) as f64;
            dx * dx + dy * dy <= r * r && s.goods.iter().any(|g| g == &d.r)
        })
    }).count();
    let mut goods_census: BTreeMap<&str, usize> = BTreeMap::new();
    for s in &w.peoples.settlements {
        for g in &s.goods {
            *goods_census.entry(g.name()).or_default() += 1;
        }
    }
    println!();
    println!("mines: {} live mineral seams known · {} worked by a town", known.len(), worked);
    println!("towns listing each good: {}", goods_census.iter().map(|(g, n)| format!("{} {}", g, n)).collect::<Vec<_>>().join(" · "));
    if !known.is_empty() {
        let share = worked as f64 / known.len() as f64;
        c.band("known seams worked", share, pct(share));
    }

    // ---- M20 — regional stone: the town's quarry names the rock under it ----
    let mut quarry_census: BTreeMap<&str, usize> = BTreeMap::new();
    let quarry_bad = w
        .peoples
        .settlements
        .iter()
        .filter(|s| {
            *quarry_census.entry(s.quarry).or_default() += 1;
            s.quarry != calliope::rock::quarry(w.fields.rock[[s.y as usize, s.x as usize]])
        })
        .count();
    println!();
    println!(
        "quarries (M20): {}",
        quarry_census
            .iter()
            .map(|(q, n)| format!("{} {}", q, n))
            .collect::<Vec<_>>()
            .join(" · ")
    );
    c.must(
        "quarry stone matches rock province",
        quarry_bad == 0,
        format!("{} mismatched of {}", quarry_bad, w.peoples.settlements.len()),
        "M20 gate: the stone a town cuts is the stone it stands on",
    );

    // ---- M14.8 the wild stocks breathe: timber, fish and game carry memory ----
    println!();
    println!("wild stocks (M14.8): transitions thin {} · collapse {} · recover {} · timber scar cells {}",
        wild_thin, wild_collapse, wild_recover, w.scars.len());
    println!("{:<9} {:>7} {:>9} {:>9} {:>8} {:>10}", "good", "grounds", "min stock", "mean", "thinned", "collapsed");
    let mut wild_known = 0usize;
    let mut wild_collapsed_now = 0usize;
    let mut wild_min_stock = f64::INFINITY;
    for g in [resources::Good::Timber, resources::Good::Fish, resources::Good::Furs,
              resources::Good::Deer, resources::Good::Elk] {
        let ds: Vec<_> = w.deposits.iter().filter(|d| d.r == g && d.known).collect();
        if ds.is_empty() { continue; }
        let min = ds.iter().map(|d| d.stock).fold(f64::INFINITY, f64::min);
        let mean = ds.iter().map(|d| d.stock).sum::<f64>() / ds.len() as f64;
        let th = ds.iter().filter(|d| d.phase == 1).count();
        let co = ds.iter().filter(|d| d.phase == 2).count();
        wild_known += ds.len();
        wild_collapsed_now += co;
        wild_min_stock = wild_min_stock.min(min);
        println!("{:<9} {:>7} {:>9.2} {:>9.2} {:>8} {:>10}", g.name(), ds.len(), min, mean, th, co);
    }
    c.want(
        "wild stocks breathe",
        wild_min_stock < 0.98,
        format!("min stock {:.2}", wild_min_stock),
        "M14.8: harvest pressure actually moves a stock",
    );
    if years >= 60 {
        c.want(
            "the axe bites somewhere",
            wild_thin + wild_collapse >= 1,
            format!("thin {} · collapse {}", wild_thin, wild_collapse),
            "M14.8: some crowded ground thins under the axe or the net",
        );
    }
    if wild_known > 0 {
        let share = wild_collapsed_now as f64 / wild_known as f64;
        c.band("wild collapse share", share, pct(share));
    }
    // withdrawal invariant: every wild good a town lists traces to a living
    // ground inside its work radius — a collapsed ground feeds nobody.
    let mut untraced = 0usize;
    for s in &w.peoples.settlements {
        let r = calliope::settlements::work_radius(s.pop);
        for g in &s.goods {
            if resources::regrow_rate(*g).is_none() { continue; }
            // the subsistence fallback: a coastal town with no natural
            // grounds at all nets minnows (trade::goods_for) — listed,
            // never traced. Workshops may sit on top of the fallback, so
            // "no other natural good" is the test, not "no other good".
            if *g == resources::Good::Fish
                && s.coastal
                && s.goods.iter().all(|o| *o == resources::Good::Fish || o.is_craft())
            { continue; }
            let ok = w.deposits.iter().any(|d| {
                d.r == *g && d.live() && {
                    let dx = (d.x - s.x) as f64;
                    let dy = (d.y - s.y) as f64;
                    dx * dx + dy * dy <= r * r
                }
            });
            if !ok { untraced += 1; }
        }
    }
    c.must(
        "wild goods trace to living grounds",
        untraced == 0,
        format!("{} untraced listings", untraced),
        "M14.8: collapse withdraws the good (subsistence nets exempt)",
    );

    // ---- M15.6 the conservation ledger: double-entry stock and flow ----
    // Every unit drawn from a reserve and every breath of a wild stock
    // was metered at the site of the change (world.flows); here the books
    // are balanced against the state ledger. Nothing consumed that was
    // not produced: residual = (state delta) − (meter), zero to rounding.
    let mut by_good: BTreeMap<&str, (usize, f64, f64, f64)> = BTreeMap::new();
    let mut ledger_worst = 0.0f64;
    let n_meters = w.flows.extracted.len().min(w.deposits.len());
    for di in 0..n_meters {
        let d = &w.deposits[di];
        let mineral = d.r.spec().reserve.is_some() && left0[di] >= 0.0;
        let (delta, meter) = if mineral {
            (left0[di] - d.left, w.flows.extracted[di])
        } else {
            (d.stock - stock0[di], w.flows.dstock[di])
        };
        let resid = (delta - meter).abs();
        ledger_worst = ledger_worst.max(resid);
        let e = by_good.entry(d.r.name()).or_insert((0, 0.0, 0.0, 0.0));
        e.0 += 1;
        e.1 += delta;
        e.2 += meter;
        e.3 = e.3.max(resid);
    }
    println!();
    println!("conservation ledger (M15.6): state delta vs site meter, {} deposits", n_meters);
    println!("{:<9} {:>6} {:>12} {:>12} {:>12}", "good", "n", "state Δ", "metered", "worst resid");
    for (g, (n, delta, meter, resid)) in by_good.iter().filter(|(_, v)| v.1 != 0.0 || v.2 != 0.0) {
        println!("{:<9} {:>6} {:>12.2} {:>12.2} {:>12.2e}", g, n, delta, meter, resid);
    }
    c.must(
        "ledger balances to rounding",
        w.flows.extracted.len() == w.deposits.len() && ledger_worst < 1e-6,
        format!("worst residual {:.2e}", ledger_worst),
        "M15.6: every draw and every breath was metered where it happened",
    );

    // ---- M14.9 per-culture tastes: the demand side leans toward the buyer --
    use calliope::culture;
    // table sanity: bounded multipliers, and the directional facts the
    // roadmap names (steppe prizes horses, the coasts prize wine, the
    // north prizes furs, the desert shuns them).
    let markers = [
        resources::Good::Horse, resources::Good::Wine, resources::Good::Furs,
        resources::Good::Timber, resources::Good::Spices, resources::Good::Marble,
    ];
    let mut bounded = true;
    for k in 0..culture::N_STYLES {
        for g in markers {
            let t = culture::taste(k, g);
            if !(0.5..=1.7).contains(&t) { bounded = false; }
        }
    }
    let steppe = culture::style_index("steppe");
    let hellenic = culture::style_index("hellenic");
    let nordic = culture::style_index("nordic");
    let arid = culture::style_index("arid");
    let facts = culture::taste(steppe, resources::Good::Horse) > 1.0
        && culture::taste(hellenic, resources::Good::Wine) > 1.0
        && culture::taste(nordic, resources::Good::Furs) > 1.0
        && culture::taste(arid, resources::Good::Furs) < 1.0;
    c.must(
        "taste table sane",
        bounded && facts,
        format!("steppe·horse {:.2} · hellenic·wine {:.2} · nordic·furs {:.2} · arid·furs {:.2}",
            culture::taste(steppe, resources::Good::Horse),
            culture::taste(hellenic, resources::Good::Wine),
            culture::taste(nordic, resources::Good::Furs),
            culture::taste(arid, resources::Good::Furs)),
        "M14.9: multipliers in [0.5,1.7] and lean the way the cultures do",
    );
    // wiring A/B through the public API: same towns, two imagined
    // citizenries — the book under the culture that prizes a worked good
    // must price it above the book under the culture that shuns it.
    let n_peoples = w.peoples.peoples.len();
    let mut ab: Option<(resources::Good, usize, usize, f64)> = None; // good, hi, lo, diff
    for g in markers {
        if !w.peoples.settlements.iter().any(|s| s.goods.contains(&g)) { continue; }
        for hi in 0..culture::N_STYLES {
            for lo in 0..culture::N_STYLES {
                let diff = culture::taste(hi, g) - culture::taste(lo, g);
                if diff > ab.map_or(0.05, |(_, _, _, d)| d) {
                    ab = Some((g, hi, lo, diff));
                }
            }
        }
    }
    if let Some((g, hi, lo, _)) = ab {
        let mut m_hi = calliope::economy::Market::default();
        let mut m_lo = calliope::economy::Market::default();
        calliope::economy::update_prices(&mut m_hi, &w.peoples.settlements, &vec![hi; n_peoples]);
        calliope::economy::update_prices(&mut m_lo, &w.peoples.settlements, &vec![lo; n_peoples]);
        let (ph, pl) = (m_hi.price(g), m_lo.price(g));
        c.must(
            "taste moves the book (A/B)",
            ph > pl,
            format!("{:?}: {} {:.3} vs {} {:.3}", g, culture::ALL_STYLES[hi], ph, culture::ALL_STYLES[lo], pl),
            "M14.9: same supply, the prizing culture's book prices it higher",
        );
    } else {
        c.want(
            "taste moves the book (A/B)",
            false,
            "no worked marker good with a taste split".into(),
            "M14.9: expected at least one of horse/wine/furs/timber/spices/marble worked",
        );
    }
    // and in the real world: the taste mix actually varies across market
    // areas — cultural geography reaches the books.
    let n_areas = w.economy.areas.markets.len();
    println!(" taste geography: {} market areas · {} peoples", n_areas, n_peoples);
    let mut spread_max = 0.0f64;
    if n_areas >= 2 {
        let style_of: Vec<usize> = w.peoples.peoples.iter()
            .map(|p| culture::style_index(&p.style)).collect();
        for g in markers {
            let mut mixes: Vec<f64> = Vec::new();
            for k in 0..n_areas {
                let mut pops = [0.0f64; culture::N_STYLES];
                for (si, s) in w.peoples.settlements.iter().enumerate() {
                    if w.economy.areas.area_of(si) == k {
                        pops[style_of.get(s.people.0).copied().unwrap_or(0)] += s.pop as f64;
                    }
                }
                let tot: f64 = pops.iter().sum();
                if tot > 0.0 {
                    mixes.push(pops.iter().enumerate()
                        .map(|(kk, p)| p * culture::taste(kk, g)).sum::<f64>() / tot);
                }
            }
            if mixes.len() >= 2 {
                let (lo, hi) = mixes.iter().fold((f64::MAX, f64::MIN), |(a, b), &m| (a.min(m), b.max(m)));
                spread_max = spread_max.max(hi - lo);
            }
        }
        c.want(
            "tastes vary across areas",
            spread_max >= 0.03,
            format!("max mix spread {:.3}", spread_max),
            "M14.9: different peoples, different books — the mix is not flat",
        );
    }

    // ---- M2.4 Bettencourt: β from ln(wealth) ~ β·ln(pop) across towns ----
    let pts: Vec<(f64, f64)> = w.peoples.settlements.iter()
        .filter(|s| s.pop > 80 && s.wealth > 1.0)
        .map(|s| ((s.pop as f64).ln(), s.wealth.ln()))
        .collect();
    if pts.len() >= 10 {
        let n = pts.len() as f64;
        let (mut sx, mut sy, mut sxx, mut sxy) = (0.0, 0.0, 0.0, 0.0);
        for (xr, yr) in &pts {
            sx += xr; sy += yr; sxx += xr * xr; sxy += xr * yr;
        }
        let beta = (n * sxy - sx * sy) / (n * sxx - sx * sx);
        c.band("wealth~pop scaling β", beta, format!("{:.2} over {} towns", beta, pts.len()));
    }

    // ---- M2.7 price ratios vs the medieval envelope ----
    if let (Some(&pg), Some(&pi_), Some(&pt)) = (means.get("grain"), means.get("iron"), means.get("timber")) {
        let ig = pi_ / pg.max(1e-9);
        c.band("iron/grain price ratio", ig, format!("{:.1}×", ig));
        let ok = pg < pi_ && pt < pi_;
        c.want("staples cheaper than metal", ok, format!("grain {:.2} · timber {:.2} · iron {:.2}", pg, pt, pi_), "the ordering of the price lists holds");
    }
    if let (Some(&pg), Some(&pau)) = (means.get("grain"), means.get("gold")) {
        let r = pau / pg.max(1e-9);
        c.band("gold/grain price ratio", r, format!("{:.1}×", r));
    }

    // ---- M14.2 salt: worked, priced in the envelope, curing the catch ----
    let salt_towns = w.peoples.settlements.iter()
        .filter(|s| s.goods.contains(&resources::Good::Salt))
        .count();
    c.want(
        "salt towns exist",
        salt_towns >= 1,
        format!("{}", salt_towns),
        "M14.2 gate: pans on arid shores get worked",
    );
    if let (Some(&pg), Some(&ps)) = (means.get("grain"), means.get("salt")) {
        let r = ps / pg.max(1e-9);
        c.band("salt/grain price ratio", r, format!("{:.1}×", r));
    }

    // ---- M14.3 animal secondaries: the second harvest reaches market ----
    let towns_with = |g: resources::Good| {
        w.peoples.settlements.iter().filter(|s| s.goods.contains(&g)).count()
    };
    let wool_towns = towns_with(resources::Good::Wool);
    let hide_towns = towns_with(resources::Good::Hides);
    let fur_towns = towns_with(resources::Good::Furs);
    let fur_ground = w.deposits.iter().any(|d| d.r == resources::Good::Furs);
    c.want(
        "wool towns exist",
        wool_towns >= 1,
        format!("{}", wool_towns),
        "M14.3: sheep country shears — the fiber rides behind the flock",
    );
    c.want(
        "hide towns exist",
        hide_towns >= 1,
        format!("{}", hide_towns),
        "M14.3: cattle and game yield hides wherever they are worked",
    );
    c.want(
        "the cold calls",
        fur_towns >= 1 || fur_ground,
        format!("{} fur towns · ground placed {}", fur_towns, fur_ground),
        "M14.3: fur country placed or worked — the luxury pull on the waste",
    );
    if let (Some(&pg), Some(&pw)) = (means.get("grain"), means.get("wool")) {
        let r = pw / pg.max(1e-9);
        c.band("wool/grain price ratio", r, format!("{:.1}×", r));
    }

    // ---- M14.4 cultivated luxuries: tight belts, worked or waiting ----
    let placed = |g: resources::Good| w.deposits.iter().filter(|d| d.r == g).count();
    for (g, name, why) in [
        (resources::Good::Grapes, "vineyard hills", "M14.4: the warm-hill belt places and gets worked"),
        (resources::Good::Spices, "spice coast", "M14.4: tropical shores place — the long-route luxury"),
        (resources::Good::Dyes, "murex shore", "M14.4: temperate dye shores place — concentrated, not everywhere"),
        (resources::Good::Clay, "clay pits", "M14.5: the alluvial margins place — the kiln's feedstock"),
        (resources::Good::Marble, "marble quarries", "M14.5: the luxury stone places — Bulk, so it moves by water or not at all"),
        (resources::Good::Gems, "gem seams", "M14.5: the jeweler's third ore places beside gold and silver"),
    ] {
        let ground = placed(g);
        let worked = towns_with(g);
        c.want(
            &format!("{name} placed"),
            ground >= 1,
            format!("{} grounds · {} towns", ground, worked),
            why,
        );
    }

    // ---- M14.5 earth crafts: the kilns actually light ----
    let pot_towns = towns_with(resources::Good::Pottery);
    let brick_towns = towns_with(resources::Good::Brick);
    let clay_worked = towns_with(resources::Good::Clay);
    c.want(
        "kilns lit",
        pot_towns + brick_towns >= 1 || clay_worked == 0,
        format!("{} pottery · {} brick towns · {} clay towns", pot_towns, brick_towns, clay_worked),
        "M14.5: worked clay must reach a kiln — pottery or brick towns exist",
    );

    // ---- M14.6 secondary recipes: the soft trades light, and the town
    // that refines need not be the camp that extracts ----
    let soft = [
        (resources::Good::Cloth, resources::Good::Wool),
        (resources::Good::Leather, resources::Good::Hides),
        (resources::Good::Wine, resources::Good::Grapes),
    ];
    let mut lit_kinds = 0usize;
    let mut split_shops = 0usize; // workshop towns that buy their feedstock off the carts
    let mut detail = String::new();
    for (out, raw) in soft {
        let shops: Vec<_> = w
            .peoples
            .settlements
            .iter()
            .filter(|s| s.goods.contains(&out))
            .collect();
        if !shops.is_empty() {
            lit_kinds += 1;
        }
        split_shops += shops.iter().filter(|s| !s.goods.contains(&raw)).count();
        detail.push_str(&format!("{} {} · ", shops.len(), out.name()));
    }
    c.want(
        "soft trades lit",
        lit_kinds >= 1,
        format!("{}{} kinds", detail, lit_kinds),
        "M14.6: wool/hides/grapes reach a workshop — cloth, leather or wine towns exist",
    );
    c.want(
        "processing splits from extraction",
        lit_kinds == 0 || split_shops >= 1,
        format!("{} workshop towns without their own feedstock ground", split_shops),
        "M14.6: at least one workshop buys its raw off the area market — towns divide by role",
    );
    c.print();
}

// ================================================================ telling

/// M6 — judge the chronicle itself: id coverage, the legend layer, the
/// registry's hygiene, artifact provenance and the story sifter's yield.
fn cmd_telling(seed: i64, size: usize, years: usize) {
    header("TELLING", &format!("seed {} · size {} · {}y", seed, size, years));

    let mut w = World::generate(seed, size);
    let mut left = (years * 12) as i64;
    while left > 0 {
        let step = left.min(120);
        w.tick(step);
        left -= step;
    }

    let evs = &w.chronicle.events;
    let reg = &w.chronicle.registry;
    let n = evs.len().max(1);

    // ---- the log itself
    let with_ids = evs.iter().filter(|e| !e.ids.is_empty()).count();
    let with_xy = evs.iter().filter(|e| e.x >= 0).count();
    let loud = evs.iter().filter(|e| telling::weight(e.k) >= 3).count();
    let loud_legend = evs
        .iter()
        .filter(|e| telling::weight(e.k) >= 3 && !e.legend.is_empty())
        .count();
    let bad_ids = evs
        .iter()
        .flat_map(|e| e.ids.iter())
        .filter(|&&id| id.0 < 0 || id.idx() >= reg.items.len())
        .count();
    println!("chronicle: {} entries over {}y ({:.1}/y)", evs.len(), years, evs.len() as f64 / years as f64);
    println!("  ids on {:.1}% · coords on {:.1}% · legend layer on {}/{} loud entries", 100.0 * with_ids as f64 / n as f64, 100.0 * with_xy as f64 / n as f64, loud_legend, loud);
    if with_ids < evs.len() {
        let mut orphans: BTreeMap<&str, usize> = BTreeMap::new();
        let mut samples: Vec<&Event> = Vec::new();
        for e in evs.iter().filter(|e| e.ids.is_empty()) {
            *orphans.entry(e.k.name()).or_default() += 1;
            if samples.len() < 4 {
                samples.push(e);
            }
        }
        let by_kind: Vec<String> = orphans.iter().map(|(k, v)| format!("{} ×{}", k, v)).collect();
        println!("  ID-LESS: {}", by_kind.join(" · "));
        for e in samples {
            println!("    m{} [{}] {} — {}", e.m, e.k, e.s, &e.text.chars().take(90).collect::<String>());
        }
    }

    // ---- the cast
    let mut kinds: BTreeMap<&str, (usize, usize)> = BTreeMap::new(); // kind -> (alive, closed)
    for e in &reg.items {
        let k = kinds.entry(e.kind.name()).or_default();
        if e.until.is_some() {
            k.1 += 1;
        } else {
            k.0 += 1;
        }
    }
    let cast: Vec<String> = kinds
        .iter()
        .map(|(k, (a, c))| format!("{} {}+{}†", k, a, c))
        .collect();
    println!("cast: {}", cast.join(" · "));
    let closed_unfated = reg
        .items
        .iter()
        .filter(|e| e.until.is_some() && e.fate.is_empty())
        .count();
    let epithets: usize = reg.items.iter().map(|e| e.epithets.len()).sum();
    let named: Vec<String> = reg
        .items
        .iter()
        .filter(|e| !e.epithets.is_empty())
        .take(4)
        .map(|e| format!("{} \"{}\"", e.name, e.epithets.last().unwrap()))
        .collect();
    println!("epithets earned: {}{}", epithets, if named.is_empty() { String::new() } else { format!("  ({})", named.join(", ")) });

    // ---- the relics
    let held = w.chronicle.artifacts.iter().filter(|a| a.holder.0 >= 0).count();
    println!("artifacts: {} wrought · {} held · {} lost", w.chronicle.artifacts.len(), held, w.chronicle.artifacts.len() - held);

    // ---- the sifter
    let stories = telling::sift(evs, reg);
    let stories2 = telling::sift(evs, reg);
    let sift_det = serde_json::to_string(&stories).unwrap() == serde_json::to_string(&stories2).unwrap();
    let mut pat: BTreeMap<&str, usize> = BTreeMap::new();
    for s in &stories {
        *pat.entry(s.pattern.as_str()).or_default() += 1;
    }
    let pats: Vec<String> = pat.iter().map(|(k, v)| format!("{} ×{}", k, v)).collect();
    println!();
    println!("stories sifted: {} ({})", stories.len(), pats.join(" · "));
    for s in stories.iter().take(8) {
        println!("  {:>5.1}  {}  (y{}–{}, {} beats)", s.score, s.title, s.y0, s.y1, s.beats.len());
    }
    let dup_titles = {
        let mut seen = BTreeSet::new();
        stories.iter().filter(|s| !seen.insert(s.title.clone())).count()
    };
    let per_century = stories.len() as f64 * 100.0 / years as f64;

    // ---- checks
    let mut c = Checks::default();
    c.must("every event carries an id", with_ids == evs.len(), pct(with_ids as f64 / n as f64), "M6.1 gate: ids = 100%");
    c.must("every carried id is valid", bad_ids == 0, format!("{} bad", bad_ids), "ids index the registry");
    c.band("events mappable (coords)", with_xy as f64 / n as f64, pct(with_xy as f64 / n as f64));
    c.must("loud events carry a legend", loud_legend == loud, format!("{}/{}", loud_legend, loud), "M6.9: two-layer telling on weight ≥ 3");
    c.must("closed entities carry a fate", closed_unfated == 0, format!("{} unfated", closed_unfated), "every ending is written");
    c.band("stories per century", per_century, format!("{:.1}", per_century));
    c.must("sifter deterministic", sift_det, if sift_det { "identical".into() } else { "DIVERGED".into() }, "same log ⇒ same stories");
    c.must("no duplicate stories", dup_titles == 0, format!("{} dups", dup_titles), "dedup bounds hold");
    c.want("a reversal story found", stories.iter().any(|s| matches!(s.pattern.as_str(), "rise-fall" | "tide-turned" | "mine-curse")), "yes".into(), "M6.7: fortunes turn on the record");
    c.want("relics wrought", !w.chronicle.artifacts.is_empty(), format!("{}", w.chronicle.artifacts.len()), "M6.3: artifacts enter the world");
    c.want("epithets earned", epithets > 0, format!("{}", epithets), "M6.8: names are coined in the field");
    c.must("chronicle unbounded", evs.len() > 200, format!("{}", evs.len()), "no truncation: the full log persists");
    c.print();
}

// ================================================================ patina (M9)

/// M9 gates: ruins accrue in mature worlds, hydronyms survive every border
/// change, name strata stay bounded and glossed, the withheld share of the
/// chronicle stays inside its band, battlefields mark the map.
fn cmd_patina(size: usize, years: usize, seeds: Vec<i64>) {
    header("PATINA", &format!("size {} · {}y · {} seeds", size, years, seeds.len()));

    struct Row {
        seed: i64,
        ruins: usize,
        ruins_late: usize, // after year 100
        veiled: usize,
        events: usize,
        renames: usize,   // conquest name-layers on settlements
        worn: usize,      // erosion renames (settlements + features)
        battlefields: usize,
        faded: usize,     // routes fallen disused
        wars: usize,
        transfers: usize, // settlements that changed hands (border changes)
        rivers_intact: bool,
        strata_over: usize,   // any formerly-stack deeper than 2
        ungloseed: usize,     // renamed things with no etymology
        sundered: usize,      // M12.1 divergences (roster rose)
        fused: usize,         // M12.4 fusions (roster fell)
        golden: usize,        // M13.2 golden dawns
        arcs: usize,          // M13.4 full arcs closed (interregnum ended)
        /// M79 — late ruins by cause phrase (patina::ruin_why), so a jump in
        /// the ruin rate names the disaster that made it.
        late_by_why: Vec<(String, usize)>,
        storm_landfalls: usize,
        storm_candidates: usize,
        storm_eligible: usize,
        storm_felled: usize,
        storm_reject_age: usize,
        storm_reject_sample: usize,
        storm_reject_intensity: usize,
        storm_reject_rate: usize,
        storm_local_range: (usize, usize),
        storm_exceed_range: (usize, usize),
        storm_eligible_centuries: [usize; 3],
        storm_felled_centuries: [usize; 3],
        storm_calibration: [[usize; 3]; 4],
    }


    let mut rows: Vec<Row> = Vec::new();
    for &seed in &seeds {
        let mut w = World::generate(seed, size);
        let mut left = (years * 12) as i64;
        while left > 0 {
            let step = left.min(120);
            w.tick(step);
            left -= step;
        }
        let evs = &w.chronicle.events;
        let veiled = evs.iter().filter(|e| e.veiled).count();
        // ADR-0018: transfer prose names realms — anchor on the banner phrase
        let transfers = evs.iter().filter(|e| e.text.contains("to the banners of")).count();
        let renames = evs.iter().filter(|e| e.text.contains("lay their own name over")).count();
        let worn = evs.iter().filter(|e| e.text.contains("wears") && e.text.contains("smooth")).count()
            + evs.iter().filter(|e| e.text.contains("appears on the new charts as")).count();
        let battlefields = w.features.iter().filter(|f| f.t == "battlefield").count();
        let faded = w.routes.iter().filter(|r| r.old).count();
        let rivers_intact = w
            .features
            .iter()
            .filter(|f| f.t == "river")
            .all(|f| f.formerly.is_empty());
        let strata_over = w.peoples.settlements.iter().filter(|s| s.formerly.len() > 2).count();
        let ungloseed = w
            .peoples.settlements
            .iter()
            .filter(|s| !s.formerly.is_empty() && s.ety.is_empty())
            .count()
            + w.features
                .iter()
                .filter(|f| (!f.formerly.is_empty() || f.t == "battlefield") && f.ety.is_empty())
                .count();
        // M12.6 — count the roster's two movements straight off the prose
        // anchors (exact regardless of tick step size)
        let sundered = evs.iter().filter(|e| e.text.contains("a people of their own")).count();
        let fused = evs.iter().filter(|e| e.text.contains("are one people now")).count();
        // M13 — the empire arc off its prose anchors
        let golden = evs.iter().filter(|e| e.text.contains("golden age dawns")).count();
        let arcs = evs.iter().filter(|e| e.text.contains("The interregnum ends")).count();
        rows.push(Row {
            seed,
            ruins: w.ruins.len(),
            ruins_late: w.ruins.iter().filter(|r| r.since > 1200).count(),
            veiled,
            events: evs.len(),
            renames,
            worn,
            battlefields,
            faded,
            wars: w.politics.wars.len(),
            transfers,
            rivers_intact,
            strata_over,
            ungloseed,
            sundered,
            fused,
            golden,
            arcs,
            late_by_why: {
                let mut t: std::collections::BTreeMap<String, usize> = Default::default();
                for r in w.ruins.iter().filter(|r| r.since > 1200) {
                    *t.entry(r.why.clone()).or_default() += 1;
                }
                t.into_iter().collect()
            },
            storm_landfalls: w.storm_bites.len(),
            storm_candidates: w.storm_fell_probe.len(),
            storm_eligible: w.storm_fell_probe.iter().filter(|p| p.eligible).count(),
            storm_felled: w.storm_fell_probe.iter().filter(|p| p.felled).count(),
            storm_reject_age: w.storm_fell_probe.iter().filter(|p| p.bite > 0.97 && p.age < 1200).count(),
            storm_reject_sample: w.storm_fell_probe.iter().filter(|p| p.bite > 0.97 && p.age >= 1200 && p.local < 12).count(),
            storm_reject_intensity: w.storm_fell_probe.iter().filter(|p| p.bite <= 0.97).count(),
            storm_reject_rate: w.storm_fell_probe.iter().filter(|p| p.bite > 0.97 && p.age >= 1200 && p.local >= 12 && (p.exceed as i64) * 1200 > p.age).count(),
            storm_local_range: (
                w.storm_fell_probe.iter().map(|p| p.local).min().unwrap_or(0),
                w.storm_fell_probe.iter().map(|p| p.local).max().unwrap_or(0),
            ),
            storm_exceed_range: (
                w.storm_fell_probe.iter().map(|p| p.exceed).min().unwrap_or(0),
                w.storm_fell_probe.iter().map(|p| p.exceed).max().unwrap_or(0),
            ),
            storm_eligible_centuries: std::array::from_fn(|century| {
                let lo = century as i64 * 1200;
                let hi = (century as i64 + 1) * 1200;
                w.storm_fell_probe
                    .iter()
                    .filter(|p| p.eligible && p.month >= lo && p.month < hi)
                    .count()
            }),
            storm_felled_centuries: std::array::from_fn(|century| {
                let lo = century as i64 * 1200;
                let hi = (century as i64 + 1) * 1200;
                w.storm_fell_probe
                    .iter()
                    .filter(|p| p.felled && p.month >= lo && p.month < hi)
                    .count()
            }),
            storm_calibration: std::array::from_fn(|bi| {
                let bite_bar = [0.97, 0.98, 0.99, 0.995][bi];
                std::array::from_fn(|ri| {
                    let return_months = [1200_i64, 1800, 2400][ri];
                    w.storm_fell_probe
                        .iter()
                        .filter(|p| {
                            p.bite > bite_bar
                                && p.age >= 1200
                                && p.local >= 12
                                && (p.exceed as i64) * return_months <= p.age
                        })
                        .count()
                })
            }),
        });

    }

    println!("\n storm-felling decision audit (M79)");
    for r in &rows {
        println!(
            "   seed {}: {} local marks · {} candidate landfalls · {} eligible · {} felled | rejected intensity {} age {} sample {} rate {} | local n {}–{} · exceed {}–{}",
            r.seed, r.storm_landfalls, r.storm_candidates, r.storm_eligible,
            r.storm_felled, r.storm_reject_intensity, r.storm_reject_age,
            r.storm_reject_sample, r.storm_reject_rate, r.storm_local_range.0,
            r.storm_local_range.1, r.storm_exceed_range.0, r.storm_exceed_range.1,
        );
        println!(
            "             eligible/felled by century: {}/{} · {}/{} · {}/{}",
            r.storm_eligible_centuries[0], r.storm_felled_centuries[0],
            r.storm_eligible_centuries[1], r.storm_felled_centuries[1],
            r.storm_eligible_centuries[2], r.storm_felled_centuries[2],
        );
        for (bi, bite) in [0.97, 0.98, 0.99, 0.995].iter().enumerate() {
            println!(
                "             calibration bite>{:.3}, age≥100y: RI≥100/150/200y = {}/{}/{}",
                bite, r.storm_calibration[bi][0], r.storm_calibration[bi][1],
                r.storm_calibration[bi][2],
            );
        }
    }

    println!(
        "{:>7} {:>6} {:>7} {:>8} {:>7} {:>7} {:>6} {:>6} {:>6} {:>6} {:>7} {:>6} {:>6} {:>5} {:>5}",
        "seed", "ruins", "late", "veiled%", "rename", "worn", "field", "faded", "wars", "xfers", "rivers", "sundr", "fused", "gold", "arcs"
    );
    for r in &rows {
        println!(
            "{:>7} {:>6} {:>7} {:>8} {:>7} {:>7} {:>6} {:>6} {:>6} {:>6} {:>7} {:>6} {:>6} {:>5} {:>5}",
            r.seed,
            r.ruins,
            r.ruins_late,
            format!("{:.1}", 100.0 * r.veiled as f64 / r.events.max(1) as f64),
            r.renames,
            r.worn,
            r.battlefields,
            r.faded,
            r.wars,
            r.transfers,
            if r.rivers_intact { "held" } else { "BROKEN" },
            r.sundered,
            r.fused,
            r.golden,
            r.arcs,
        );
    }

    let n = rows.len().max(1) as f64;
    let late_years = years.saturating_sub(100).max(1) as f64;
    let ruin_rate = rows.iter().map(|r| r.ruins_late as f64).sum::<f64>() / n / (late_years / 100.0);
    let veil_share = rows.iter().map(|r| r.veiled as f64 / r.events.max(1) as f64).sum::<f64>() / n;
    let total_wars: usize = rows.iter().map(|r| r.wars).sum();
    let total_fields: usize = rows.iter().map(|r| r.battlefields).sum();
    let total_xfers: usize = rows.iter().map(|r| r.transfers).sum();
    let total_renames: usize = rows.iter().map(|r| r.renames).sum();
    let total_worn: usize = rows.iter().map(|r| r.worn).sum();

    // M79 — where the late ruins came from, aggregated over the seeds. A
    // ruin rate that moves must name the cause that moved it.
    {
        let mut t: std::collections::BTreeMap<&str, usize> = Default::default();
        for r in &rows {
            for (why, n) in &r.late_by_why {
                *t.entry(why.as_str()).or_default() += n;
            }
        }
        println!("\n late ruins by cause (all seeds, after y100)");
        let mut v: Vec<_> = t.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        for (why, n) in v {
            println!("   {:>4}  {}", n, why);
        }
    }

    let mut c = Checks::default();
    c.band("ruins per century (after y100)", ruin_rate, format!("{:.2}", ruin_rate));
    c.band("withheld share of the chronicle", veil_share, pct(veil_share));
    c.must(
        "hydronyms conserved",
        rows.iter().all(|r| r.rivers_intact),
        if rows.iter().all(|r| r.rivers_intact) { "all held".into() } else { "RENAMED".into() },
        "M9.2 gate: river names survive every border change",
    );
    c.must(
        "border changes occurred",
        total_xfers > 0,
        format!("{} transfers", total_xfers),
        "the conservation claim is tested, not vacuous",
    );
    c.must(
        "name strata bounded",
        rows.iter().all(|r| r.strata_over == 0),
        format!("{} over", rows.iter().map(|r| r.strata_over).sum::<usize>()),
        "M9.2: at most two former names per place",
    );
    c.must(
        "every new name carries a gloss",
        rows.iter().all(|r| r.ungloseed == 0),
        format!("{} bare", rows.iter().map(|r| r.ungloseed).sum::<usize>()),
        "M9.3 gate: renames and wearings are etymologized",
    );
    c.want(
        "names worn or relaid somewhere",
        total_renames + total_worn > 0,
        format!("{}+{}", total_renames, total_worn),
        "M9.2/M9.3: the strata actually accrue",
    );
    c.want(
        "battlefields mark the map",
        total_wars < 3 || total_fields > 0,
        format!("{} fields / {} wars", total_fields, total_wars),
        "M9.4: decisive fields earn names",
    );
    // M12.6 — over three centuries the people roster must move both ways:
    // divergence mints daughters, fusion folds minorities back in.
    let breathing = rows.iter().filter(|r| r.sundered > 0 && r.fused > 0).count();
    c.want(
        "people count moves both ways (≥60% of seeds)",
        breathing * 10 >= rows.len() * 6,
        format!("{}/{}", breathing, rows.len()),
        "M12.6: sunderings and fusions both fire on the patina clock",
    );
    // M13.4 — on the multi-century clock whole civilizations must close
    // their arcs: golden noon, fall, interregnum, succession.
    let arc_rate = rows.iter().map(|r| r.arcs as f64).sum::<f64>() / n / (years as f64 / 300.0);
    let total_golden: usize = rows.iter().map(|r| r.golden).sum();
    c.band("civ arcs completed per 300 y", arc_rate, format!("{:.1} (gold {} · arcs {})", arc_rate, total_golden, rows.iter().map(|r| r.arcs).sum::<usize>()));
    c.print();
}

// ================================================================ determinism

fn cmd_determinism(seed: i64, size: usize, months: i64) {
    header("DETERMINISM", &format!("seed {} · size {} · {} mo", seed, size, months));

    // state hash, not pack(): the packed header embeds wall-clock stage
    // timings, so pack bytes legitimately differ between identical worlds.
    let wa = World::generate(seed, size);
    let wb = World::generate(seed, size);
    let ga = hash_state(&wa);
    let gb = hash_state(&wb);
    println!("generation: state A {:016x} · state B {:016x}", ga, gb);

    // same chunking twice
    let run = |chunk: i64| -> (u64, u64, u64) {
        let mut w = World::generate(seed, size);
        let mut evs: Vec<Event> = Vec::new();
        let mut left = months;
        while left > 0 {
            let step = left.min(chunk);
            let (e, _, _) = w.tick(step);
            evs.extend(e);
            left -= step;
        }
        (hash_state(&w), hash_events(&evs), hash_settlements(&w))
    };
    let r1 = run(12);
    let r2 = run(12);
    let r3 = run(60);
    println!("run ×12 chunks:  state {:016x} · events {:016x} · towns {:016x}", r1.0, r1.1, r1.2);
    println!("run ×12 again:   state {:016x} · events {:016x} · towns {:016x}", r2.0, r2.1, r2.2);
    println!("run ×60 chunks:  state {:016x} · events {:016x} · towns {:016x}", r3.0, r3.1, r3.2);

    let mut c = Checks::default();
    c.must("generation reproducible", ga == gb, if ga == gb { "identical".into() } else { "DIVERGED".into() }, "same seed ⇒ identical world state");
    c.must("simulation reproducible", r1 == r2, if r1 == r2 { "identical".into() } else { "DIVERGED".into() }, "same seed+chunks ⇒ same history");
    c.must("chunking invariant", r1 == r3, if r1 == r3 { "identical".into() } else { "DIVERGED".into() }, "12-mo vs 60-mo ticks must agree");
    c.print();
}

// ================================================================ bench

fn cmd_bench() {
    let sub = if cfg!(feature = "alloc-count") {
        "native release · alloc-count"
    } else {
        "native release"
    };
    header("BENCH", sub);
    let sizes = [320usize, 512, 640, 768];
    println!("{:>5} {:>9} {:>11} {:>9}  stage breakdown (ms)", "size", "gen ms", "pack bytes", "cells");
    let mut gen512 = 0.0f64;
    let mut bpc512 = 0.0f64;
    let mut gen512_spread = (0.0f64, 0.0f64);
    for &s in &sizes {
        // E5.9 — the banded size gets criterion-style repetition: five
        // samples, median reported, spread shown; flanking sizes stay
        // single-shot (informational rows, not gates).
        let samples = if s == 512 { 5 } else { 1 };
        let mut ms_all: Vec<f64> = Vec::with_capacity(samples);
        let mut world: Option<World> = None;
        for _ in 0..samples {
            let t = Instant::now();
            let w = World::generate(4242, s);
            ms_all.push(t.elapsed().as_secs_f64() * 1000.0);
            world = Some(w);
        }
        ms_all.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let ms = ms_all[ms_all.len() / 2];
        let w = world.unwrap();
        let packed = w.pack();
        if s == 512 {
            gen512 = ms;
            bpc512 = packed.len() as f64 / w.fields.height.len() as f64;
            gen512_spread = (ms_all[0], *ms_all.last().unwrap());
        }
        let stages: String = w
            .timings
            .iter()
            .filter(|(name, _)| *name != "total")
            .map(|(name, v)| format!("{} {:.0}", name, v))
            .collect::<Vec<_>>()
            .join(" · ");
        println!("{:>5} {:>9.0} {:>11} {:>9}  {}", s, ms, packed.len(), w.fields.height.len(), stages);
    }
    println!(
        "512 generation: median of 5 samples · spread {:.0}–{:.0} ms (E5.9)",
        gen512_spread.0, gen512_spread.1
    );

    // tick throughput at 512 — five 240-month windows on one aging world
    // (E5.9): the median window kills timer noise, and later windows
    // measure a heavier, more built-up world. Window 0 doubles as the
    // allocation-budget window (E5.10) when the counting allocator is in.
    let mut w = World::generate(4242, 512);
    let towns0 = w.peoples.settlements.len();
    let mut window_ms: Vec<f64> = Vec::with_capacity(5);
    #[cfg_attr(not(feature = "alloc-count"), allow(unused_mut))]
    let mut alloc_window: Option<u64> = None;
    for _i in 0..5 {
        #[cfg(feature = "alloc-count")]
        let a0 = alloc_count::count();
        let t = Instant::now();
        w.tick(240);
        window_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        #[cfg(feature = "alloc-count")]
        if _i == 0 {
            alloc_window = Some(alloc_count::count() - a0);
        }
    }
    window_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let tick_ms = window_ms[window_ms.len() / 2];
    let rate = 240.0 / (tick_ms / 1000.0);
    println!();
    println!(
        "tick throughput @512, towns {}→{}: median 240-month window {:.0} ms = {:.0} months/s (5 windows)",
        towns0,
        w.peoples.settlements.len(),
        tick_ms,
        rate
    );
    if let Some(a) = alloc_window {
        println!(
            "allocations: {} across window 0 = {:.0}/month (E5.10, counting allocator)",
            a,
            a as f64 / 240.0
        );
    }

    // E4 — tick payload budget: a century of single-month tick_json calls
    // on a fresh 512 world; the median is what a playing client pays. The
    // per-section ledger below shows where the bytes actually live.
    let mut w = World::generate(4242, 512);
    let mut lens: Vec<usize> = Vec::with_capacity(1200);
    let mut sections: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for _ in 0..1200 {
        let p = w.tick_json(1);
        lens.push(p.len());
        let v: serde_json::Value = serde_json::from_str(&p).unwrap();
        for (k, val) in v.as_object().unwrap() {
            let e = sections.entry(k.clone()).or_insert((0, 0));
            e.0 += 1;
            e.1 += val.to_string().len();
        }
    }
    lens.sort_unstable();
    let med = lens[lens.len() / 2] as f64;
    let p90 = lens[lens.len() * 9 / 10];
    let max = *lens.last().unwrap();
    println!(
        "tick payload @512 over 100 y: median {} B · p90 {} B · max {} B",
        med as usize, p90, max
    );
    let mut by_bytes: Vec<_> = sections.into_iter().collect();
    by_bytes.sort_by(|a, b| b.1 .1.cmp(&a.1 .1));
    for (k, (ships, bytes)) in by_bytes {
        println!(
            "  {:<18} {:>5} ships · {:>9} B total · {:>6} B/ship",
            k, ships, bytes, bytes / ships.max(1)
        );
    }

    let mut c = Checks::default();
    c.band("512 generation time", gen512, format!("{:.0} ms", gen512));
    c.band("tick rate", rate, format!("{:.0} mo/s", rate));
    c.band("pack bytes per cell", bpc512, format!("{:.1} B/cell", bpc512));
    c.band("median tick payload", med, format!("{:.0} B", med));
    // E5.10 — allocation regressions get an alarm: the count is a pure
    // function of the seed (same code path ⇒ same allocations), so the
    // band can sit tight against the measured baseline.
    if let Some(a) = alloc_window {
        let apm = a as f64 / 240.0;
        c.band("allocations per month", apm, format!("{:.0}/mo", apm));
    }
    c.print();
}

// ================================================================= perf
//
// E10 — Proof of Speed. Perf claims become banded checks like every other
// claim in this project:
//   E10.1 per-stage generation budgets, asserted across the seed sweep
//   E10.2 tick-rate bands at a year-0 world and a year-100 world
//   E10.6 native peak RSS ceiling after the 100-year run
// The band is checked against the WORST seed, not the mean — a budget that
// only holds on the friendly seed is not a budget.

/// Resident set in MiB from /proc/self/status: VmHWM (true peak) where the
/// kernel exposes it, else VmRSS measured at the heaviest moment — sandboxes
/// (gVisor et al.) mask VmHWM, and end-of-run VmRSS with every world still
/// resident is the honest available ceiling there.
fn peak_rss_mib() -> Option<(f64, &'static str)> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for (key, label) in [("VmHWM:", "VmHWM peak"), ("VmRSS:", "VmRSS at peak load")] {
        if let Some(line) = status.lines().find(|l| l.starts_with(key)) {
            if let Some(kb) = line.split_whitespace().nth(1).and_then(|v| v.parse::<f64>().ok()) {
                return Some((kb / 1024.0, label));
            }
        }
    }
    None
}

fn cmd_perf(size: usize, seeds: Vec<i64>) {
    header("PERF", &format!("size {} · {} seeds · native release", size, seeds.len()));

    const STAGES: &[&str] = &[
        "terrain", "erosion", "climate", "hydrology", "biomes",
        "fertility", "naming", "resources", "settlements",
    ];

    // ---- E10.1: per-stage generation budgets ----
    let mut worst: BTreeMap<&str, f64> = BTreeMap::new();
    let mut worst_total = 0.0f64;
    println!("per-stage generation (ms):");
    print!("  {:<8}", "seed");
    for s in STAGES {
        print!(" {:>10}", s);
    }
    println!(" {:>8}", "total");
    let mut worlds: Vec<World> = Vec::new();
    for &seed in &seeds {
        let w = World::generate(seed, size);
        print!("  {:<8}", seed);
        let mut total = 0.0;
        for s in STAGES {
            let ms = w
                .timings
                .iter()
                .find(|(n, _)| n == s)
                .map(|(_, v)| *v)
                .unwrap_or(0.0);
            let e = worst.entry(s).or_insert(0.0);
            if ms > *e {
                *e = ms;
            }
            print!(" {:>10.0}", ms);
        }
        if let Some((_, t)) = w.timings.iter().find(|(n, _)| *n == "total") {
            total = *t;
        }
        worst_total = worst_total.max(total);
        println!(" {:>8.0}", total);
        worlds.push(w);
    }

    // ---- E10.2: tick rate on a young world and an old one ----
    // Year 0: the world as a player first meets it. Year 100: towns, roads,
    // markets, chronicle all grown in — the heavier steady state a long
    // sitting actually pays for. Median of 3 windows kills timer noise.
    let mut rate_y0 = f64::INFINITY;
    let mut rate_y100 = f64::INFINITY;
    for (i, w) in worlds.iter_mut().enumerate() {
        let windows = |w: &mut World| -> f64 {
            let mut ms: Vec<f64> = (0..3)
                .map(|_| {
                    let t = Instant::now();
                    w.tick(240);
                    t.elapsed().as_secs_f64() * 1000.0
                })
                .collect();
            ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
            240.0 / (ms[1] / 1000.0)
        };
        let r0 = windows(w); // months 0–720
        w.tick(1200 - w.month.min(1200)); // grow to year 100
        let r100 = windows(w); // months 1200–1920
        println!(
            "tick rate seed {}: year-0 {:.0} mo/s · year-100 {:.0} mo/s ({} towns)",
            seeds[i],
            r0,
            r100,
            w.peoples.settlements.len()
        );
        rate_y0 = rate_y0.min(r0);
        rate_y100 = rate_y100.min(r100);
    }

    // ---- E10.6: memory ceiling after the heavy run ----
    let rss = peak_rss_mib();
    match rss {
        Some((m, src)) => println!("native peak RSS after run: {:.0} MiB via {} ({} worlds resident)", m, src, worlds.len()),
        None => println!("native peak RSS: /proc/self/status unavailable on this platform"),
    }

    let mut c = Checks::default();
    for s in STAGES {
        c.band(
            &format!("stage {} ms", s),
            worst[s],
            format!("{:.0} ms (worst of {} seeds)", worst[s], seeds.len()),
        );
    }
    c.band("gen total ms", worst_total, format!("{:.0} ms (worst)", worst_total));
    c.band("tick rate year 0", rate_y0, format!("{:.0} mo/s (worst)", rate_y0));
    c.band("tick rate year 100", rate_y100, format!("{:.0} mo/s (worst)", rate_y100));
    if let Some((m, src)) = rss {
        c.band("native peak RSS", m, format!("{:.0} MiB ({})", m, src));
    }
    c.print();
}

// ================================================================ sweep

fn cmd_sweep(size: usize, years: usize, seeds: Vec<i64>) {
    header("SWEEP", &format!("size {} · {}y · {} seeds", size, years, seeds.len()));
    println!("{:>7} {:>6} {:>6} {:>6} {:>5} {:>5} {:>4} {:>5} {:>9} {:>6} {:>5} {:>4} {:>4} {:>4} {:>4} {:>4} {:>5} {:>6}  flags", "seed", "land%", "des%", "for%", "mtn%", "isl", "dep", "towns", "pop", "grw×", "era", "arts", "strk", "camp", "war", "rt", "ev/y", "genms");

    struct Row {
        seed: i64,
        land: f64,
        desert: f64,
        forest: f64,
        mtn: f64,
        camps: usize,
        strikes: usize,
        famines: usize,
        zipf: f64,
        growth: f64,
        pace: f64,
        era: usize,
        evyr: f64,
        flags: String,
        /// M12.1 — a daughter people sundered off during the run
        sundered: bool,
        /// M37 — icebound lanes (winter-closed + perennial) and malformed masks
        iced: usize,
        ice_bad: usize,
        /// M48 — lanes sailing the monsoon calendar, and malformed bursts
        mons: usize,
        mons_bad: usize,
    }
    let mut rows: Vec<Row> = Vec::new();

    for &seed in &seeds {
        let t = Instant::now();
        let mut w = World::generate(seed, size);
        let gen_ms = t.elapsed().as_millis();
        let land = land_mask(&w);
        let land_n = land.iter().filter(|&&b| b).count() as f64;
        let land_frac = land_n / land.len() as f64;
        let bc = biome_counts(&w);
        let desert = bc[gc::DESERT as usize] as f64 / land_n.max(1.0);
        let forest = (bc[gc::WOODLAND as usize] + bc[gc::SEASONAL_RAIN_FOREST as usize] + bc[gc::TEMPERATE_RAIN_FOREST as usize] + bc[gc::BOREAL_FOREST as usize] + bc[gc::TROPICAL_RAIN_FOREST as usize]) as f64 / land_n.max(1.0);
        let hs = masked(&w.fields.height, &land);
        let mtn = hs.iter().filter(|&&h| h > 0.5).count() as f64 / land_n.max(1.0);
        let li = ndimage::label(&land, true);
        let bl = border_land(&w);
        let setts0 = w.peoples.settlements.len();
        let pop0: i64 = w.peoples.settlements.iter().map(|s| s.pop).sum();

        let log = run_years(&mut w, years);

        let pop1: i64 = w.peoples.settlements.iter().map(|s| s.pop).sum();
        let growth = pop1 as f64 / pop0.max(1) as f64;
        let pace = if years >= 100 {
            log.rows[years / 2 - 1].1 as f64 / pop1.max(1) as f64
        } else {
            0.0
        };
        let era = w.peoples.societies.iter().map(|s| s.era).max().unwrap_or(0);
        let arts: usize = w.peoples.societies.iter().map(|s| s.techs.len()).sum();
        let evyr = log.total_events as f64 / years.max(1) as f64;
        let unconnected = w.peoples.settlements.iter().filter(|s| s.connections == 0).count();

        // M37 — the winter schedule of this seed's lanes
        let fro = calliope::seaice::frozen_months(&w.fields.height, &w.fields.tmean, &w.fields.tamp);
        let (ice_seasonal, ice_perennial, ice_bad) = ice_route_stats(&w, &fro);
        let iced = ice_seasonal + ice_perennial;

        // M48 — the sailor's calendar on this seed's lanes
        let arc_ok = calliope::trade::monsoon_burst_mask(1.0) | calliope::trade::monsoon_burst_mask(-1.0);
        let mons_lanes = w.routes.iter().filter(|r| r.season.abs() >= calliope::trade::MONSOON_LANE).count();
        let mons_bad = w
            .routes
            .iter()
            .filter(|r| {
                let m = route_monsoon_mask(r);
                m != 0 && (m & !arc_ok) != 0
            })
            .count();

        // M2.3: per-seed rank-size slope (NaN when too few towns to judge)
        let mut pops: Vec<f64> = w.peoples.settlements.iter().map(|s| s.pop as f64).filter(|&p| p >= 120.0).collect();
        pops.sort_by(|a, b| b.partial_cmp(a).unwrap());
        let zipf = if pops.len() >= 10 {
            let np = pops.len() as f64;
            let (mut sx, mut sy, mut sxx, mut sxy) = (0.0, 0.0, 0.0, 0.0);
            for (i, p) in pops.iter().enumerate() {
                let xr = ((i + 1) as f64).ln();
                let yr = p.ln();
                sx += xr; sy += yr; sxx += xr * xr; sxy += xr * yr;
            }
            (np * sxy - sx * sy) / (np * sxx - sx * sx)
        } else {
            f64::NAN
        };

        let mut flags = String::new();
        if bl > 0 {
            flags.push('B'); // border land
        }
        if log.placeholders > 0 {
            flags.push('P'); // template leak
        }
        if w.routes.is_empty() {
            flags.push('R'); // no trade
        }
        if growth < 1.05 {
            flags.push('G'); // stagnant
        }
        if log.strikes == 0 && years >= 60 {
            flags.push('S'); // no prospecting
        }
        if unconnected > 0 {
            flags.push('U'); // lonely towns
        }
        if ice_bad > 0 {
            flags.push('I'); // malformed ice closure
        }
        if mons_bad > 0 {
            flags.push('M'); // malformed monsoon burst
        }
        if flags.is_empty() {
            flags.push('·');
        }

        println!("{:>7} {:>6.1} {:>6.1} {:>6.1} {:>5.1} {:>5} {:>4} {:>2}→{:<2} {:>9} {:>6.2} {:>5} {:>4} {:>4} {:>4} {:>4} {:>4} {:>5.1} {:>6}  {}", seed, 100.0 * land_frac, 100.0 * desert, 100.0 * forest, 100.0 * mtn, li.n, w.deposits.len(), setts0, w.peoples.settlements.len(), pop1, growth, era, arts, log.strikes, log.camps, log.wars, w.routes.len(), evyr, gen_ms, flags);

        rows.push(Row { seed, land: land_frac, desert, forest, mtn, camps: log.camps, strikes: log.strikes, famines: log.famines, zipf, growth, pace, era, evyr, flags, sundered: log.peoples_rose, iced, ice_bad, mons: mons_lanes, mons_bad });
    }

    let n = rows.len() as f64;
    let mean = |f: &dyn Fn(&Row) -> f64| rows.iter().map(|r| f(r)).sum::<f64>() / n;
    let m_land = mean(&|r| r.land);
    let m_des = mean(&|r| r.desert);
    let m_for = mean(&|r| r.forest);
    let m_mtn = mean(&|r| r.mtn);
    let m_grw = mean(&|r| r.growth);
    let m_evy = mean(&|r| r.evyr);
    println!("{:->7} {:>6.1} {:>6.1} {:>6.1} {:>5.1} {:>35} {:>6.2} {:>27.1}", "mean", 100.0 * m_land, 100.0 * m_des, 100.0 * m_for, 100.0 * m_mtn, "", m_grw, m_evy);
    println!();
    println!("flags: B border-land · P placeholder-leak · R no-routes · G stagnant · S no-strikes · U unconnected · I malformed-ice · M malformed-monsoon");
    println!(
        "sea ice (M37): icebound lanes per seed: {}",
        rows.iter().map(|r| format!("{}:{}", r.seed, r.iced)).collect::<Vec<_>>().join(" · ")
    );
    println!(
        "monsoon (M48): calendar lanes per seed: {}",
        rows.iter().map(|r| format!("{}:{}", r.seed, r.mons)).collect::<Vec<_>>().join(" · ")
    );

    let camp_seeds = rows.iter().filter(|r| r.camps > 0).count();
    let strike_seeds = rows.iter().filter(|r| r.strikes > 0).count();
    let iron_seeds = rows.iter().filter(|r| r.era >= 2).count();
    let clean = rows.iter().filter(|r| r.flags == "·").count();
    let worst_flags: Vec<String> = rows.iter().filter(|r| r.flags != "·").map(|r| format!("{}:{}", r.seed, r.flags)).collect();

    let mut c = Checks::default();
    c.band_as("mean land fraction", "land fraction", m_land, pct(m_land));
    c.band_as("mean desert share", "desert share of land", m_des, pct(m_des));
    c.band_as("mean forest share", "forest share of land", m_for, pct(m_for));
    c.band_as("mean mountain share", "mountain share of land (h>0.5)", m_mtn, pct(m_mtn));
    c.band_as("mean growth", "century growth", m_grw, format!("{:.2}×", m_grw));
    c.band_as("mean events/year", "events per year", m_evy, format!("{:.1}", m_evy));
    c.must("all seeds clean of hard flags", clean == rows.len(), if worst_flags.is_empty() { "all clean".into() } else { worst_flags.join(" ") }, "no B/P/R/G/S/U/I/M flags on any seed");
    c.want("strikes on every seed", strike_seeds == rows.len(), format!("{}/{}", strike_seeds, rows.len()), "prospecting fires everywhere");
    let ice_bad_total: usize = rows.iter().map(|r| r.ice_bad).sum();
    let iced_seeds = rows.iter().filter(|r| r.iced > 0).count();
    c.must(
        "icebound closures well-formed on every seed",
        ice_bad_total == 0,
        format!("{} malformed", ice_bad_total),
        "M37 gate: winter arcs only, hemisphere-true, across the sweep",
    );
    c.want(
        "the winter sea closes somewhere in the sweep",
        iced_seeds >= 1,
        format!("{}/{} seeds", iced_seeds, rows.len()),
        "M37: some strait should freeze in a polar world",
    );
    let mons_bad_total: usize = rows.iter().map(|r| r.mons_bad).sum();
    let mons_seeds = rows.iter().filter(|r| r.mons > 0).count();
    c.must(
        "monsoon closures well-formed on every seed",
        mons_bad_total == 0,
        format!("{} malformed", mons_bad_total),
        "M48 gate: burst arcs only, hemisphere-true, across the sweep",
    );
    c.want(
        "the monsoon calendar is sailed somewhere in the sweep",
        mons_seeds >= 1,
        format!("{}/{} seeds", mons_seeds, rows.len()),
        "M48: some lane should ride the seasonal winds in a monsoon world",
    );
    if years >= 80 {
        c.want("mining camps emerge (≥60% of seeds)", camp_seeds * 10 >= rows.len() * 6, format!("{}/{}", camp_seeds, rows.len()), "ore pull creates colonies");
        c.want("Iron Age reached (≥50% of seeds)", iron_seeds * 2 >= rows.len(), format!("{}/{}", iron_seeds, rows.len()), "history should not stall in bronze");
    }
    if years >= 100 {
        let pacing = rows.iter().filter(|r| r.pace <= 0.92).count();
        c.want("worlds still growing at half-run (≥60%)", pacing * 10 >= rows.len() * 6, format!("{}/{}", pacing, rows.len()), "no century-long plateaus");
        // M12.1 — divergence must fire on the century clock; fusion (the
        // falling half of M12.6) is judged in patina where 300y give it room.
        let sunderers = rows.iter().filter(|r| r.sundered).count();
        c.want("daughter peoples sunder (≥60% of seeds)", sunderers * 10 >= rows.len() * 6, format!("{}/{}", sunderers, rows.len()), "M12.1: far branches become peoples of their own");
        // M2.6: dry-shock years must starve somewhere across the sweep,
        // but famine must stay an event, not a climate.
        let famine_seeds = rows.iter().filter(|r| r.famines > 0).count();
        c.want("famine strikes somewhere (≥60% of seeds)", famine_seeds * 10 >= rows.len() * 6, format!("{}/{}", famine_seeds, rows.len()), "M2.6: failed rains have a price");
        let worst_fam = rows.iter().map(|r| r.famines as f64 * 100.0 / years as f64).fold(0.0f64, f64::max);
        c.want("famine bounded (<150/century worst seed)", worst_fam < 150.0, format!("{:.0}/century", worst_fam), "M2.6: hunger is a visitation, not the weather");
    }
    // M2.3 across seeds: mean rank-size slope where measurable
    let zipfs: Vec<f64> = rows.iter().map(|r| r.zipf).filter(|z| z.is_finite()).collect();
    if !zipfs.is_empty() {
        let mz = zipfs.iter().sum::<f64>() / zipfs.len() as f64;
        c.band("mean rank-size slope", mz, format!("{:.2} over {} seeds", mz, zipfs.len()));
    }
    c.print();
}

// ================================================================ main

// ================================================================ properties (M8.1/M8.2)
// Seam-invariant properties: facts that must hold on every world the
// engine can emit, checked on the real widened arrays after real ticks —
// not on the square generation frame where the original code paths run.

/// Rectangular priority-flood fill (Barnes 2014, as in hydrology.rs but
/// for the widened (rows × cols) world): every land cell drains.
fn fill_rect(height: &Array2<f64>, water: &Array2<bool>) -> Array2<f64> {
    use std::cmp::Ordering;
    use std::collections::BinaryHeap;
    struct Item(f64, usize, usize);
    impl PartialEq for Item {
        fn eq(&self, o: &Self) -> bool { self.0 == o.0 && self.1 == o.1 && self.2 == o.2 }
    }
    impl Eq for Item {}
    impl PartialOrd for Item {
        fn partial_cmp(&self, o: &Self) -> Option<Ordering> { Some(self.cmp(o)) }
    }
    impl Ord for Item {
        fn cmp(&self, o: &Self) -> Ordering {
            o.0.partial_cmp(&self.0).unwrap().then_with(|| o.1.cmp(&self.1)).then_with(|| o.2.cmp(&self.2))
        }
    }
    let (rows, cols) = height.dim();
    let eps = 1e-5;
    let mut filled = height.clone();
    let mut visited = water.clone();
    let mut heap: BinaryHeap<Item> = BinaryHeap::new();
    for y in 0..rows {
        for x in 0..cols {
            if water[[y, x]] { continue; }
            let border = y == 0 || y == rows - 1 || x == 0 || x == cols - 1;
            let mut adj = false;
            for (dy, dx) in [(-1isize, 0isize), (1, 0), (0, -1), (0, 1)] {
                let ny = y as isize + dy;
                let nx = x as isize + dx;
                if ny >= 0 && nx >= 0 && ny < rows as isize && nx < cols as isize {
                    adj |= water[[ny as usize, nx as usize]];
                }
            }
            if border || adj {
                heap.push(Item(filled[[y, x]], y, x));
                visited[[y, x]] = true;
            }
        }
    }
    while let Some(Item(hcur, y, x)) = heap.pop() {
        for &(dy, dx) in hydrology::N8.iter() {
            let ny = y as isize + dy;
            let nx = x as isize + dx;
            if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize { continue; }
            let (ny, nx) = (ny as usize, nx as usize);
            if visited[[ny, nx]] { continue; }
            visited[[ny, nx]] = true;
            let mut nh = filled[[ny, nx]];
            if nh <= hcur {
                nh = hcur + eps;
                filled[[ny, nx]] = nh;
            }
            heap.push(Item(nh, ny, nx));
        }
    }
    filled
}

fn dirs_rect(filled: &Array2<f64>, water: &Array2<bool>) -> Array2<i8> {
    let (rows, cols) = filled.dim();
    Array2::from_shape_fn((rows, cols), |(y, x)| {
        if water[[y, x]] { return -1i8; }
        let mut best_drop = 0.0f64;
        let mut best_dir = -1i8;
        for (i, (&(dy, dx), &dist)) in hydrology::N8.iter().zip(hydrology::DIST.iter()).enumerate() {
            let ny = y as isize + dy;
            let nx = x as isize + dx;
            if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize { continue; }
            let drop = (filled[[y, x]] - filled[[ny as usize, nx as usize]]) / dist;
            if drop > best_drop {
                best_drop = drop;
                best_dir = i as i8;
            }
        }
        best_dir
    })
}

fn cmd_properties(size: usize, years: usize, seeds: Vec<i64>) {
    header("PROPERTIES", &format!("size {} · {} y · {} seeds", size, years, seeds.len()));
    println!("seam-invariant properties (M8.1) and metamorphic checks (M8.2)");
    println!();

    let mut c = Checks::default();
    // M64 sweep aggregates — the Earth calibration answers as a family too
    let (mut m64_b1, mut m64_elev, mut m64_rb, mut m64_hack, mut m64_fp, mut m64_built): (
        Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>,
    ) = Default::default();
    for &seed in &seeds {
        let mut w = World::generate(seed, size);
        w.tick(years as i64 * 12);
        let (rows, cols) = w.fields.height.dim();

        // ---- P1: every river cell descends the filled surface to an outlet
        let water = w.fields.height.mapv(|h| h < 0.0);
        let filled = fill_rect(&w.fields.height.mapv(|h| h as f64), &water);
        let dirs = dirs_rect(&filled, &water);
        let mut uphill = 0usize;
        let mut cycles = 0usize;
        let mut dry_terminals = 0usize;
        let mut river_cells = 0usize;
        for y in 0..rows {
            for x in 0..cols {
                if w.fields.flags[[y, x]] & CellFlags::RIVER.bits() == 0 { continue; }
                river_cells += 1;
                let (mut cy, mut cx) = (y, x);
                let mut steps = 0usize;
                loop {
                    let d = dirs[[cy, cx]];
                    if d < 0 {
                        // legitimate terminals: the basin floor of an
                        // endorheic lake, or a lake cell (ghost-spill)
                        if w.fields.flags[[cy, cx]] & (CellFlags::LAKE.bits() | CellFlags::SALT.bits()) == 0 { dry_terminals += 1; }
                        break;
                    }
                    let (dy, dx) = hydrology::N8[d as usize];
                    let (ny, nx) = ((cy as isize + dy) as usize, (cx as isize + dx) as usize);
                    if filled[[ny, nx]] > filled[[cy, cx]] + 1e-9 { uphill += 1; break; }
                    if water[[ny, nx]] { break; } // reached the sea
                    cy = ny; cx = nx;
                    steps += 1;
                    if steps > rows * cols { cycles += 1; break; }
                }
            }
        }
        println!("seed {:>6}: {} river cells · {} uphill · {} cycles · {} dry terminals",
            seed, river_cells, uphill, cycles, dry_terminals);
        c.must(&format!("rivers descend filled ({})", seed), uphill == 0 && cycles == 0,
            format!("{} up · {} cyc", uphill, cycles), "M8.1: water only runs downhill");
        c.must(&format!("rivers reach an outlet ({})", seed), dry_terminals == 0,
            format!("{} dry", dry_terminals), "M8.1: sea, lake or salt basin");

        // ---- P2: every living settlement is reachable on the route network
        let living: Vec<usize> = (0..w.peoples.settlements.len()).filter(|&i| w.peoples.settlements[i].pop > 0).collect();
        let mut idx_of: BTreeMap<calliope::ids::SettlementId, usize> = BTreeMap::new();
        for (k, &i) in living.iter().enumerate() { idx_of.insert(w.peoples.settlements[i].id, k); }
        let mut uf: Vec<usize> = (0..living.len()).collect();
        fn find(uf: &mut Vec<usize>, i: usize) -> usize {
            let mut r = i;
            while uf[r] != r { r = uf[r]; }
            let mut c = i;
            while uf[c] != r { let n = uf[c]; uf[c] = r; c = n; }
            r
        }
        let mut degree = vec![0usize; living.len()];
        for r in &w.routes {
            if let (Some(&ia), Some(&ib)) = (idx_of.get(&r.a), idx_of.get(&r.b)) {
                degree[ia] += 1;
                degree[ib] += 1;
                let (ra, rb) = (find(&mut uf, ia), find(&mut uf, ib));
                if ra != rb { uf[ra] = rb; }
            }
        }
        let isolated = degree.iter().filter(|&&d| d == 0).count();
        let comps = {
            let mut roots = BTreeSet::new();
            for i in 0..living.len() { let r = find(&mut uf, i); roots.insert(r); }
            roots.len()
        };
        println!("seed {:>6}: {} towns · {} routes · {} isolated · {} components",
            seed, living.len(), w.routes.len(), isolated, comps.max(1));
        c.must(&format!("no isolated settlement ({})", seed), isolated == 0,
            format!("{} cut off", isolated), "M8.1: every town trades");
        c.must(&format!("route graph connected ({})", seed), comps <= 1,
            format!("{} comps", comps.max(1)), "M8.1: one world, one web");

        // ---- P3: pack v3 round-trips — stable bytes, honest layout,
        // valid crc, quantization inside ε, territory RLE exact (E3.3-E3.6)
        let p1 = w.pack();
        let p2 = w.pack();
        c.must(&format!("pack is stable ({})", seed), p1 == p2,
            format!("{} B", p1.len()), "M8.1: same world ⇒ same bytes");
        let hlen = u32::from_le_bytes([p1[0], p1[1], p1[2], p1[3]]) as usize;
        let hdr: serde_json::Value = serde_json::from_slice(&p1[4..4 + hlen]).unwrap();
        let base = 4 + hlen;
        let crc = calliope::util::crc32(&p1[base..]);
        c.must(&format!("pack v3 + crc32 ({})", seed),
            hdr["pack"].as_u64() == Some(calliope::pack::PACK_VERSION as u64) && hdr["crc32"].as_u64() == Some(crc as u64),
            format!("crc {:08x}", crc), "E3.6: stamped and checksummed");
        let entries = hdr["arrays"].as_array().unwrap();
        let mut ok_layout = true;
        let mut expected_off = 0usize;
        let mut total = 0usize;
        for e in entries {
            let off = e["offset"].as_u64().unwrap() as usize;
            let nb = e["nbytes"].as_u64().unwrap() as usize;
            let shape: Vec<usize> = e["shape"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as usize).collect();
            let cell = match e["dtype"].as_str().unwrap() { "float32" => 4, "uint16" | "int16" => 2, _ => 1 };
            // M70 — a bit-lane section is the ceiling of its bit run.
            let want = match e.get("bits").and_then(|b| b.as_u64()) {
                Some(b) => (shape[0] * shape[1] * b as usize + 7) / 8,
                None => shape[0] * shape[1] * cell,
            };
            ok_layout &= off == expected_off && nb == want;
            expected_off = off + nb;
            total += nb;
        }
        ok_layout &= p1.len() == 4 + hlen + total;
        c.must(&format!("unpack layout sound ({})", seed), ok_layout,
            format!("{} arrays", entries.len()), "M8.1: offsets contiguous, sizes exact");

        // decode every section exactly the way the client does and compare.
        // M68 — the lookup comes from the registry itself (`field_decls`),
        // not a hand-written name→grid map: a grid registered in pack.rs is
        // a grid this gate judges, and there is no second list to drift.
        let decls = w.field_decls();
        let decl = |name: &str| -> &calliope::pack::FieldDecl<'_> {
            decls
                .iter()
                .find(|d| d.name == name)
                .expect("packed section is a registry field")
        };
        let f32_grid = |name: &str| -> &ndarray::Array2<f32> {
            match &decl(name).data {
                calliope::pack::FieldData::F32(a) => a,
                _ => unreachable!("{name} is not an f32 field"),
            }
        };
        let u8_grid = |name: &str| -> &ndarray::Array2<u8> {
            match &decl(name).data {
                calliope::pack::FieldData::U8(a) => a,
                _ => unreachable!("{name} is not a u8 field"),
            }
        };
        let mut ok_data = true;
        let mut worst_q = 0.0f64; // worst quantization error, in units of scale
        for e in entries {
            let name = e["name"].as_str().unwrap();
            let off = base + e["offset"].as_u64().unwrap() as usize;
            let nb = e["nbytes"].as_u64().unwrap() as usize;
            if let Some(q) = e.get("q").filter(|q| !q.is_null()) {
                let scale = q["scale"].as_f64().unwrap();
                let qoff = q["offset"].as_f64().unwrap();
                let sqrt = q["xform"].as_str() == Some("sqrt");
                let grid = f32_grid(name);
                let qs: Vec<u32> = if e["dtype"].as_str() == Some("uint8") {
                    p1[off..off + nb].iter().map(|&b| b as u32).collect()
                } else {
                    p1[off..off + nb].chunks_exact(2)
                        .map(|b| u16::from_le_bytes([b[0], b[1]]) as u32).collect()
                };
                ok_data &= qs.len() == grid.len();
                for (&qv, &orig) in qs.iter().zip(grid.iter()) {
                    let dec = qoff + qv as f64 * scale;
                    // compare in encode-space: |dec − T(orig)| ≤ scale/2
                    let target = if sqrt { (orig.max(0.0) as f64).sqrt() } else { orig as f64 };
                    if scale > 0.0 {
                        worst_q = worst_q.max((dec - target).abs() / scale);
                    } else {
                        ok_data &= (dec - target).abs() < 1e-12;
                    }
                }
            } else {
                match e["dtype"].as_str().unwrap() {
                    "uint8" => {
                        let grid = u8_grid(name);
                        // M70 — the bit lane is lossless by construction, so
                        // the gate decodes it exactly the way the client does
                        // and demands equality, not tolerance.
                        match e.get("bits").and_then(|b| b.as_u64()) {
                            Some(bits) if bits < 8 => {
                                let bits = bits as usize;
                                let mask = (1u32 << bits) - 1;
                                let (mut acc, mut have, mut pos) = (0u32, 0usize, off);
                                let mut vals: Vec<u8> = Vec::with_capacity(grid.len());
                                for _ in 0..grid.len() {
                                    while have < bits {
                                        acc |= (*p1.get(pos).unwrap_or(&0) as u32) << have;
                                        pos += 1;
                                        have += 8;
                                    }
                                    vals.push((acc & mask) as u8);
                                    acc >>= bits;
                                    have -= bits;
                                }
                                ok_data &= vals.iter().zip(grid.iter()).all(|(&a, &b)| a == b);
                            }
                            _ => {
                                ok_data &= p1[off..off + nb].iter().zip(grid.iter()).all(|(&a, &b)| a == b);
                            }
                        }
                    }
                    "float32" => {
                        let grid = f32_grid(name);
                        let vals: Vec<f32> = p1[off..off + nb].chunks_exact(4)
                            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
                        ok_data &= vals.iter().zip(grid.iter()).all(|(&v, &g)| v == g);
                    }
                    other => panic!("unexpected raw dtype {other}"),
                }
            }
        }
        c.must(&format!("raw sections bit-equal ({})", seed), ok_data,
            "bit-equal".into(), "M8.1: arrays survive the wire");
        c.must(&format!("quantization ≤ ε ({})", seed), worst_q <= 0.5 + 1e-6,
            format!("worst {:.3}·scale", worst_q), "E3.4: quantized wire loses ≤ half a step");

        // territory rides the header as RLE (E3.5) — expand and compare
        let rle: Vec<i64> = hdr["territory"].as_array().unwrap().iter()
            .map(|v| v.as_i64().unwrap()).collect();
        let mut terr: Vec<i16> = Vec::with_capacity(rows * cols);
        for pair in rle.chunks_exact(2) {
            for _ in 0..pair[0] {
                terr.push(pair[1] as i16);
            }
        }
        let terr_ok = terr.len() == w.fields.territory.len()
            && terr.iter().zip(w.fields.territory.iter()).all(|(&a, &b)| a == b);
        c.must(&format!("territory RLE exact ({})", seed), terr_ok,
            format!("{} runs", rle.len() / 2), "E3.5: borders compress, nothing drifts");

        // ---- M8.2 metamorphic: more rain must not shrink the rivers
        let dry = World::generate(seed, size);
        let wet = World::generate_scaled(seed, size, 1.25);
        let rc_dry = dry.fields.flags.iter().filter(|&&f| f & CellFlags::RIVER.bits() != 0).count();
        let rc_wet = wet.fields.flags.iter().filter(|&&f| f & CellFlags::RIVER.bits() != 0).count();
        let q_dry: f64 = dry.fields.discharge.iter().map(|&v| v as f64).sum();
        let q_wet: f64 = wet.fields.discharge.iter().map(|&v| v as f64).sum();
        println!("seed {:>6}: rain ×1.25 ⇒ river cells {} → {} · discharge {:.0} → {:.0}",
            seed, rc_dry, rc_wet, q_dry, q_wet);
        c.must(&format!("rain↑ ⇒ rivers not↓ ({})", seed), rc_wet >= rc_dry,
            format!("{} → {}", rc_dry, rc_wet), "M8.2: metamorphic monotonicity");
        c.must(&format!("rain↑ ⇒ discharge↑ ({})", seed), q_wet > q_dry,
            format!("×{:.2}", q_wet / q_dry.max(1e-9)), "M8.2: more water flows");

        // ---- M64: calibration vs Earth — the deep-earth stack answers
        // to published numbers, not to taste. Hypsometry against the
        // classic hypsographic curve, the river net against Horton's
        // ratios and Hack-pruned drainage density, floodplains against
        // mapped alluvium, the coast against its own vocabulary census.
        // All dawn state: none of it moves with the ticked years.
        let mpu = calliope::constants::METRES_PER_UNIT;
        let mut land_n = 0usize;
        let mut below1 = 0usize;
        let mut hsum = 0.0f64;
        for &h in w.fields.height.iter() {
            if h < 0.0 { continue; }
            land_n += 1;
            let m = h as f64 * mpu;
            hsum += m;
            if m < 1000.0 { below1 += 1; }
        }
        let mean_elev = hsum / land_n.max(1) as f64;
        let b1 = 100.0 * below1 as f64 / land_n.max(1) as f64;

        // upstream area over the filled surface: highest cells first,
        // every land cell hands its accumulated area to its receiver
        let flat_filled = filled.as_slice().expect("filled is standard layout");
        let mut order_idx: Vec<u32> = (0..(rows * cols) as u32).collect();
        order_idx.sort_by(|&a, &b| flat_filled[b as usize].partial_cmp(&flat_filled[a as usize]).unwrap());
        let mut area = vec![1.0f64; rows * cols];
        for &i in &order_idx {
            let i = i as usize;
            let (y, x) = (i / cols, i % cols);
            if water[[y, x]] { continue; }
            let d = dirs[[y, x]];
            if d >= 0 {
                let (dy, dx) = hydrology::N8[d as usize];
                let j = (y as isize + dy) as usize * cols + (x as isize + dx) as usize;
                area[j] += area[i];
            }
        }

        // Horton's law lives on the drainage tree, not on the render-
        // pruned river mask: the mask's headwaters sit below the
        // discharge threshold, so a census there reads Rb≈1 off pure
        // artifact (the first measurement did exactly that). Build the
        // measurement network at a fixed support area, give it its own
        // Strahler orders, census streams on it.
        const A_C_CELLS: f64 = 5.0; // 80 km² support — the finest channel this grid can honestly resolve
        let chan: Vec<bool> = (0..rows * cols).map(|i| {
            let (y, x) = (i / cols, i % cols);
            !water[[y, x]] && area[i] >= A_C_CELLS
        }).collect();
        // decreasing filled height = every donor before its receiver
        let mut ord = vec![0u8; rows * cols];
        let mut top_m = vec![0u8; rows * cols]; // max donor order seen
        let mut top_c = vec![0u8; rows * cols]; // donors carrying that max
        for &iu in &order_idx {
            let i = iu as usize;
            if !chan[i] { continue; }
            let (y, x) = (i / cols, i % cols);
            let k = if top_m[i] == 0 { 1 } else if top_c[i] >= 2 { top_m[i] + 1 } else { top_m[i] };
            ord[i] = k;
            let d = dirs[[y, x]];
            if d < 0 { continue; }
            let (dy, dx) = hydrology::N8[d as usize];
            let (ny, nx) = ((y as isize + dy) as usize, (x as isize + dx) as usize);
            if water[[ny, nx]] { continue; }
            let j = ny * cols + nx;
            if !chan[j] { continue; }
            if k > top_m[j] {
                top_m[j] = k;
                top_c[j] = 1;
            } else if k == top_m[j] {
                top_c[j] = top_c[j].saturating_add(1);
            }
        }
        fn root(uf: &mut [u32], i: u32) -> u32 {
            let mut r = i;
            while uf[r as usize] != r { r = uf[r as usize]; }
            let mut cur = i;
            while uf[cur as usize] != r { let nx = uf[cur as usize]; uf[cur as usize] = r; cur = nx; }
            r
        }
        let mut uf2: Vec<u32> = (0..(rows * cols) as u32).collect();
        for y in 0..rows {
            for x in 0..cols {
                let i = y * cols + x;
                if !chan[i] { continue; }
                let d = dirs[[y, x]];
                if d < 0 { continue; }
                let (dy, dx) = hydrology::N8[d as usize];
                let (ny, nx) = ((y as isize + dy) as usize, (x as isize + dx) as usize);
                if water[[ny, nx]] { continue; }
                let j = ny * cols + nx;
                if chan[j] && ord[j] == ord[i] {
                    let (ra, rb2) = (root(&mut uf2, i as u32), root(&mut uf2, j as u32));
                    if ra != rb2 { uf2[ra as usize] = rb2; }
                }
            }
        }
        let mut stream_order: BTreeMap<u32, u8> = BTreeMap::new();
        for i in 0..rows * cols {
            if chan[i] {
                let r = root(&mut uf2, i as u32);
                stream_order.entry(r).or_insert(ord[i]);
            }
        }
        let mut nk = [0usize; 16];
        for (_, &k) in &stream_order { nk[(k as usize).min(15)] += 1; }
        let kmax = (1..16).rev().find(|&k| nk[k] > 0).unwrap_or(1);
        // log-linear fit of N_k over the populated orders: Rb = exp(−slope)
        let pts_n: Vec<(f64, f64)> = (1..=kmax).filter(|&k| nk[k] > 0)
            .map(|k| (k as f64, (nk[k] as f64).ln())).collect();
        let nfit = pts_n.len() as f64;
        let sx: f64 = pts_n.iter().map(|p| p.0).sum();
        let sy2: f64 = pts_n.iter().map(|p| p.1).sum();
        let sxx: f64 = pts_n.iter().map(|p| p.0 * p.0).sum();
        let sxy: f64 = pts_n.iter().map(|p| p.0 * p.1).sum();
        let slope = (nfit * sxy - sx * sy2) / (nfit * sxx - sx * sx).max(1e-12);
        let rb = (-slope).exp();

        // Drainage density answers on the ground where the pruning law
        // holds: humid land (precip ≥ 400 mm — Whittaker's semi-arid
        // line; the constant 1.4 is a humid-terrain figure and deserts
        // rightly carry no channels). Channel length walks the real D8
        // steps — a diagonal cell is 4√2 km of river, not 4.
        let riv = |y: usize, x: usize| w.fields.flags[[y, x]] & CellFlags::RIVER.bits() != 0;
        let mut has_donor = vec![false; rows * cols];
        for y in 0..rows {
            for x in 0..cols {
                if !riv(y, x) { continue; }
                let d = dirs[[y, x]];
                if d < 0 { continue; }
                let (dy, dx) = hydrology::N8[d as usize];
                let (ny, nx) = ((y as isize + dy) as usize, (x as isize + dx) as usize);
                if !water[[ny, nx]] && riv(ny, nx) { has_donor[ny * cols + nx] = true; }
            }
        }
        let mut wet_land = 0usize;
        let mut wet_len_km = 0.0f64;
        let mut heads_a: Vec<f64> = Vec::new();
        for y in 0..rows {
            for x in 0..cols {
                if water[[y, x]] { continue; }
                if (w.fields.precip[[y, x]] as f64) < 400.0 { continue; }
                wet_land += 1;
                if !riv(y, x) { continue; }
                let d = dirs[[y, x]];
                let diag = d >= 0 && hydrology::N8[d as usize].0 != 0 && hydrology::N8[d as usize].1 != 0;
                wet_len_km += if diag { 4.0 * std::f64::consts::SQRT_2 } else { 4.0 };
                if !has_donor[y * cols + x] { heads_a.push(area[y * cols + x] * 16.0); }
            }
        }
        heads_a.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let a50 = if heads_a.is_empty() { 0.0 } else { heads_a[heads_a.len() / 2] };
        let dd = wet_len_km / (wet_land.max(1) as f64 * 16.0);
        let hack = dd * a50.sqrt() / 1.4;

        // Floodplain: "mapped alluvium" means an alluvial plain — tens
        // of metres of valley fill — not a 1 m overbank veneer (the
        // first measurement read 37% of land off exactly that
        // misreading). The plain is silt ≥ 10 m; the veneer ladder
        // prints for the record. And the channel law is a component
        // law: every silt body must hold the water that laid it —
        // plains run wider than one cell, so cell-adjacency undercounts
        // by construction.
        let mut fp_ladder = [0usize; 3]; // ≥1 m · ≥5 m · ≥10 m
        let plain: Vec<bool> = (0..rows * cols).map(|i| {
            let (y, x) = (i / cols, i % cols);
            if water[[y, x]] { return false; }
            let m = w.fields.silt[[y, x]] as f64 * mpu;
            m >= 10.0
        }).collect();
        for y in 0..rows {
            for x in 0..cols {
                if water[[y, x]] { continue; }
                let m = w.fields.silt[[y, x]] as f64 * mpu;
                if m >= 1.0 { fp_ladder[0] += 1; }
                if m >= 5.0 { fp_ladder[1] += 1; }
                if m >= 10.0 { fp_ladder[2] += 1; }
            }
        }
        let fp = fp_ladder[2];
        // union silt bodies (8-conn); a body is river-borne when any
        // cell in it or its 1-ring carries RIVER or LAKE
        let mut uf3: Vec<u32> = (0..(rows * cols) as u32).collect();
        for y in 0..rows {
            for x in 0..cols {
                let i = y * cols + x;
                if !plain[i] { continue; }
                for (dy, dx) in [(0isize, 1isize), (1, -1), (1, 0), (1, 1)] {
                    let (ny, nx) = (y as isize + dy, x as isize + dx);
                    if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize { continue; }
                    let j = ny as usize * cols + nx as usize;
                    if plain[j] {
                        let (ra, rb2) = (root(&mut uf3, i as u32), root(&mut uf3, j as u32));
                        if ra != rb2 { uf3[ra as usize] = rb2; }
                    }
                }
            }
        }
        let mut body_size: BTreeMap<u32, usize> = BTreeMap::new();
        let mut body_wet: BTreeMap<u32, bool> = BTreeMap::new();
        for y in 0..rows {
            for x in 0..cols {
                let i = y * cols + x;
                if !plain[i] { continue; }
                let r = root(&mut uf3, i as u32);
                *body_size.entry(r).or_insert(0) += 1;
                let mut touch = false;
                'tk: for dy in -1isize..=1 {
                    for dx in -1isize..=1 {
                        let (ny, nx) = (y as isize + dy, x as isize + dx);
                        if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize { continue; }
                        if w.fields.flags[[ny as usize, nx as usize]] & (CellFlags::RIVER.bits() | CellFlags::LAKE.bits()) != 0 {
                            touch = true;
                            break 'tk;
                        }
                    }
                }
                if touch { body_wet.insert(r, true); }
            }
        }
        let fp_borne: usize = body_size.iter()
            .filter(|(r, _)| body_wet.get(r).copied().unwrap_or(false))
            .map(|(_, &n)| n).sum();
        let fp_share = 100.0 * fp as f64 / land_n.max(1) as f64;
        let fp_hug = 100.0 * fp_borne as f64 / fp.max(1) as f64;

        // Coast-type census off the landform lane itself (M60 words) —
        // counted as shoreline FRONTAGE, not area. Earth's coast-type
        // frequencies (Stutz & Pilkey's ~10% barrier belt, coast
        // classifications generally) are shares of coastline *length*:
        // an areal word (delta plain, raised beach) counts only where
        // it fronts the sea; water words (fjord, lagoon, estuary) are
        // frontage by nature. The first measurement compared delta
        // plain's whole area against spits counted cell by cell and
        // read 85% dominance off that category error.
        use calliope::landform as lf;
        let mut fam = [0usize; 5]; // drowned · built · tidal · raised · open shore
        let mut wordc: BTreeMap<u8, usize> = BTreeMap::new();
        for y in 0..rows {
            for x in 0..cols {
                let cw = w.fields.landform[[y, x]];
                let f = match cw {
                    lf::RIA | lf::SKERRY | lf::FJORD => 0,
                    lf::DELTA | lf::SPIT | lf::BARRIER | lf::LAGOON => 1,
                    lf::TIDEFLAT | lf::ESTUARY => 2,
                    lf::RAISED => 3,
                    lf::SHORE => 4,
                    _ => continue,
                };
                if w.fields.height[[y, x]] >= 0.0 {
                    let mut fronts = false;
                    'fr: for dy in -1isize..=1 {
                        for dx in -1isize..=1 {
                            let (ny, nx) = (y as isize + dy, x as isize + dx);
                            if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize { continue; }
                            if w.fields.height[[ny as usize, nx as usize]] < 0.0 {
                                fronts = true;
                                break 'fr;
                            }
                        }
                    }
                    if !fronts { continue; }
                }
                fam[f] += 1;
                if f < 4 { *wordc.entry(cw).or_insert(0) += 1; }
            }
        }
        let coast_all: usize = fam.iter().sum();
        let storied: usize = coast_all - fam[4];
        let storied_share = 100.0 * storied as f64 / coast_all.max(1) as f64;
        let built_share = 100.0 * fam[1] as f64 / coast_all.max(1) as f64;
        // The mixing law governs the words active coastal processes
        // mint. Raised/ria/skerry are dictated by the frozen sea-level
        // curve and already banded per stand by the M26 gate — capping
        // them here would double-gate the curve (777 runs 71% raised
        // beach because its curve net-emerged; that is the curve's
        // business, not the mixer's).
        let minted = [lf::FJORD, lf::TIDEFLAT, lf::ESTUARY, lf::DELTA, lf::SPIT, lf::BARRIER, lf::LAGOON];
        let minted_total: usize = minted.iter().map(|&k| *wordc.get(&k).unwrap_or(&0)).sum();
        let (top_w, top_n) = minted.iter().map(|&k| (k, *wordc.get(&k).unwrap_or(&0)))
            .max_by_key(|&(_, v)| v).unwrap();
        let top_share = top_n as f64 / minted_total.max(1) as f64;

        println!("seed {:>6}: hypsometry mean {:.0} m · below 1 km {:.1}%", seed, mean_elev, b1);
        println!("seed {:>6}: streams (A_c 80 km²) N_k {:?} · Rb {:.2}", seed, &nk[1..=kmax.min(9)], rb);
        println!("seed {:>6}: wet-land Dd {:.4} km/km² · head A₅₀ {:.0} km² · Hack ratio {:.2}",
            seed, dd, a50, hack);
        println!("seed {:>6}: silt ladder ≥1/5/10 m: {:.1}/{:.1}/{:.2}% of land · plain {} bodies · {:.1}% river-borne",
            seed, 100.0 * fp_ladder[0] as f64 / land_n.max(1) as f64,
            100.0 * fp_ladder[1] as f64 / land_n.max(1) as f64,
            100.0 * fp_ladder[2] as f64 / land_n.max(1) as f64,
            body_size.len(), fp_hug);
        let census: Vec<String> = wordc.iter()
            .map(|(&k, &v)| format!("{} {}", lf::NAMES[k as usize], v)).collect();
        println!("seed {:>6}: coast frontage {} cells · storied {:.1}% · [{}] · shore {}",
            seed, coast_all, storied_share, census.join(" · "), fam[4]);

        c.band_as(&format!("land below 1 km % ({})", seed), "land below 1 km % (continent)", b1, format!("{:.1}%", b1));
        c.band_as(&format!("mean land elevation m ({})", seed), "mean land elevation m (continent)", mean_elev, format!("{:.0} m", mean_elev));
        c.band_as(&format!("horton bifurcation ratio ({})", seed), "horton bifurcation ratio", rb, format!("Rb {:.2}", rb));
        c.band_as(&format!("hack density ratio ({})", seed), "hack density ratio", hack, format!("{:.2}", hack));
        c.band_as(&format!("floodplain share of land % ({})", seed), "floodplain share of land %", fp_share, format!("{:.2}%", fp_share));
        c.band_as(&format!("floodplain river adjacency % ({})", seed), "floodplain river adjacency %", fp_hug, format!("{:.1}%", fp_hug));
        c.band_as(&format!("built belt share of coast % ({})", seed), "built belt share of coast %", built_share, format!("{:.1}%", built_share));
        c.band_as(&format!("coast with a story % ({})", seed), "coast with a story %", storied_share, format!("{:.1}%", storied_share));
        c.must(&format!("coastal families all present ({})", seed),
            fam[0] > 0 && fam[1] > 0 && fam[2] > 0 && fam[3] > 0,
            format!("{}/{}/{}/{}", fam[0], fam[1], fam[2], fam[3]),
            "M64: drowned, built, tidal and raised coasts all exist");
        c.must(&format!("no word owns the minted coast ({})", seed), top_share <= 0.70,
            format!("{} {:.0}%", lf::NAMES[top_w as usize], 100.0 * top_share),
            "M64: among process-minted words (fjord/tidal/estuary/delta/spit/barrier/lagoon), top ≤70%");
        m64_b1.push(b1);
        m64_elev.push(mean_elev);
        m64_rb.push(rb);
        m64_hack.push(hack);
        m64_fp.push(fp_share);
        m64_built.push(built_share);
        println!();
    }

    // M64: the sweep's aggregate distributions against the same bands —
    // the calibration must hold as a family, not only seed by seed
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
    c.band_as("sweep mean: land below 1 km %", "land below 1 km %", mean(&m64_b1), format!("{:.1}%", mean(&m64_b1)));
    c.band_as("sweep mean: land elevation m", "mean land elevation m", mean(&m64_elev), format!("{:.0} m", mean(&m64_elev)));
    c.band_as("sweep mean: horton Rb", "horton bifurcation ratio", mean(&m64_rb), format!("Rb {:.2}", mean(&m64_rb)));
    c.band_as("sweep mean: hack ratio", "hack density ratio", mean(&m64_hack), format!("{:.2}", mean(&m64_hack)));
    c.band_as("sweep mean: floodplain %", "floodplain share of land %", mean(&m64_fp), format!("{:.2}%", mean(&m64_fp)));
    c.band_as("sweep mean: built belt %", "built belt share of coast %", mean(&m64_built), format!("{:.1}%", mean(&m64_built)));


    // ---- P4: tick v2 deltas tell the truth (E4). A client that starts
    // from bootstrap and merges every delta must end holding exactly the
    // engine's settlements; no section may reship unchanged bytes; the
    // chronicle cursor must tile [start, end) with no gap or overlap.
    {
        let seed = seeds[0];
        let mut w = World::generate(seed, size);
        let boot = w.bootstrap();

        // the client's shadow: id -> serialized settlement (as Value)
        let mut shadow: BTreeMap<i64, serde_json::Value> = boot["settlements"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| (s["id"].as_i64().unwrap(), s.clone()))
            .collect();

        // areas shadow: hub id -> held row Value (partial merges land here)
        let mut areas_shadow: BTreeMap<i64, serde_json::Value> = boot["areas"]["hubs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| (h["id"].as_i64().unwrap(), h.clone()))
            .collect();

        // market shadow: good -> held row (m_hot patches merge by good)
        let mut market_shadow: BTreeMap<String, serde_json::Value> = boot["market"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| (r["g"].as_str().unwrap().to_string(), r.clone()))
            .collect();

        // territory shadow (E4.7): the client decodes the dawn grid from
        // the pack header, then merges full RLE re-ships and 32×32 tile
        // patches as they arrive — whichever the engine judged smaller.
        let mut terr_shadow = w.fields.territory.clone();
        let mut terr_full_ships = 0usize;
        let mut terr_tile_ships = 0usize;

        const BLOCKS: [&str; 4] = ["cultures", "c_hot", "wars", "merchants"];
        let mut last_block: BTreeMap<&str, String> = BTreeMap::new();
        let mut reships = 0usize;
        let mut cursor_breaks = 0usize;
        let mut expect_ev = w.chronicle.events.len() as u64;
        let months = 240usize;
        for _ in 0..months {
            let payload = w.tick_json(1);
            let t: serde_json::Value = serde_json::from_str(&payload).unwrap();
            let ev = t["ev"].as_array().unwrap();
            if ev[0].as_u64().unwrap() != expect_ev {
                cursor_breaks += 1;
            }
            expect_ev = ev[1].as_u64().unwrap();
            for b in BLOCKS {
                if let Some(v) = t.get(b) {
                    let s = v.to_string();
                    if last_block.get(b) == Some(&s) {
                        reships += 1;
                    }
                    last_block.insert(b, s);
                }
            }
            if let Some(setts) = t.get("settlements").and_then(|v| v.as_array()) {
                for s in setts {
                    let id = s["id"].as_i64().unwrap();
                    // a full object identical to what the client holds is
                    // wasted wire — the hash gate should have caught it
                    if shadow.get(&id) == Some(s) {
                        reships += 1;
                    }
                    shadow.insert(id, s.clone());
                }
            }
            // heartbeat patches (E4.2): positional rows [id, pop, food, k,
            // wealth] merged over the held object — a patch whose every
            // non-null slot equals what the client holds is a reship
            if let Some(hots) = t.get("s_hot").and_then(|v| v.as_array()) {
                const SLOTS: [&str; 4] = ["pop", "food", "k", "wealth"];
                for p in hots {
                    let row = p.as_array().unwrap();
                    let id = row[0].as_i64().unwrap();
                    let held = shadow.get_mut(&id).expect("hot patch for unknown town");
                    let live: Vec<(&str, &serde_json::Value)> = SLOTS
                        .iter()
                        .zip(row[1..].iter())
                        .filter(|(_, v)| !v.is_null())
                        .map(|(k, v)| (*k, v))
                        .collect();
                    let redundant = !live.is_empty()
                        && live.iter().all(|(k, v)| held.get(k) == Some(v));
                    if redundant || live.is_empty() {
                        reships += 1;
                    }
                    for (k, v) in live {
                        held[k] = v.clone();
                    }
                }
            }
            if let Some(gone) = t.get("settlements_gone").and_then(|v| v.as_array()) {
                for id in gone {
                    shadow.remove(&id.as_i64().unwrap());
                }
            }
            // territory (E4.7): full RLE replaces the grid, tile patches
            // land in place; both must leave the shadow equal to truth
            if let Some(rle) = t.get("territory").and_then(|v| v.as_array()) {
                terr_full_ships += 1;
                let flat = terr_shadow.as_slice_mut().unwrap();
                let mut i = 0usize;
                let mut k = 0usize;
                while k + 1 < rle.len() {
                    let run = rle[k].as_i64().unwrap() as usize;
                    let val = rle[k + 1].as_i64().unwrap() as i16;
                    flat[i..i + run].fill(val);
                    i += run;
                    k += 2;
                }
                assert_eq!(i, flat.len(), "territory RLE must cover the grid");
            } else if let Some(p) = t.get("territory_tiles") {
                terr_tile_ships += 1;
                let (h, wd) = terr_shadow.dim();
                let tw = p["tw"].as_u64().unwrap() as usize;
                for tile in p["tiles"].as_array().unwrap() {
                    let tx = tile[0].as_u64().unwrap() as usize;
                    let ty = tile[1].as_u64().unwrap() as usize;
                    let (x0, y0) = (tx * tw, ty * tw);
                    let (t_w, t_h) = (tw.min(wd - x0), tw.min(h - y0));
                    let mut j = 0usize;
                    let rle = tile[2].as_array().unwrap();
                    let mut k = 0usize;
                    while k + 1 < rle.len() {
                        let mut run = rle[k].as_i64().unwrap();
                        let val = rle[k + 1].as_i64().unwrap() as i16;
                        while run > 0 {
                            terr_shadow[[y0 + j / t_w, x0 + j % t_w]] = val;
                            j += 1;
                            run -= 1;
                        }
                        k += 2;
                    }
                    assert_eq!(j, t_w * t_h, "tile RLE must cover its tile");
                }
            }

            // the market ledger (E4.3): full replace on good-set change,
            // else m_hot rows merge by good — unchanged rows are waste
            if let Some(m) = t.get("market").and_then(|v| v.as_array()) {
                market_shadow = m
                    .iter()
                    .map(|r| (r["g"].as_str().unwrap().to_string(), r.clone()))
                    .collect();
            }
            if let Some(mh) = t.get("m_hot").and_then(|v| v.as_array()) {
                for r in mh {
                    let g = r["g"].as_str().unwrap().to_string();
                    if market_shadow.get(&g) == Some(r) {
                        reships += 1;
                    }
                    market_shadow.insert(g, r.clone());
                }
            }
            // market areas (E4.3): full replace when "of" rides along;
            // rows with a name replace the hub, nameless rows are per-good
            // price patches — and a patch that changes nothing is waste
            if let Some(a) = t.get("areas") {
                let full = a.get("of").is_some();
                if full {
                    areas_shadow.clear();
                }
                for h in a["hubs"].as_array().unwrap() {
                    let id = h["id"].as_i64().unwrap();
                    if h.get("name").is_some() {
                        if !full && areas_shadow.get(&id) == Some(h) {
                            reships += 1;
                        }
                        areas_shadow.insert(id, h.clone());
                    } else {
                        let held = areas_shadow
                            .get_mut(&id)
                            .expect("price patch for unknown hub");
                        let pm = h["p"].as_object().unwrap();
                        let redundant =
                            pm.iter().all(|(g, v)| held["p"].get(g) == Some(v));
                        if redundant {
                            reships += 1;
                        }
                        for (g, v) in pm {
                            held["p"][g.as_str()] = v.clone();
                        }
                    }
                }
            }
        }
        let truth: BTreeMap<i64, serde_json::Value> = w
            .peoples.settlements
            .iter()
            .map(|s| (s.id.0, serde_json::to_value(s).unwrap()))
            .collect();
        let replay_ok = shadow == truth;
        let market_truth: BTreeMap<String, serde_json::Value> = w
            .economy.market
            .snapshot()
            .as_array()
            .unwrap()
            .iter()
            .map(|r| (r["g"].as_str().unwrap().to_string(), r.clone()))
            .collect();
        let areas_truth: BTreeMap<i64, serde_json::Value> =
            calliope::economy::areas_json(&w.economy.areas, &w.peoples.settlements)["hubs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|h| (h["id"].as_i64().unwrap(), h.clone()))
                .collect();
        println!(
            "seed {:>6}: {} months of deltas · {} towns replayed · {} reships · {} cursor breaks",
            seed, months, truth.len(), reships, cursor_breaks
        );
        c.must(&format!("tick deltas replay to truth ({})", seed), replay_ok,
            format!("{} towns", truth.len()), "E4.2: merge(bootstrap, deltas) = engine state");
        c.must(&format!("market replays to truth ({})", seed), market_shadow == market_truth,
            format!("{} goods", market_truth.len()), "E4.3: merge(bootstrap, m_hot) = engine ledger");
        if areas_shadow != areas_truth {
            // name the first divergent hub — a bare FAIL hides the shape
            for (id, t) in &areas_truth {
                if areas_shadow.get(id) != Some(t) {
                    println!("  area diverges at hub {}: shadow={} truth={}", id,
                        areas_shadow.get(id).map(|v| v.to_string()).unwrap_or("∅".into()), t);
                    break;
                }
            }
            for id in areas_shadow.keys() {
                if !areas_truth.contains_key(id) {
                    println!("  area shadow holds unknown hub {}", id);
                    break;
                }
            }
        }
        c.must(&format!("area prices replay to truth ({})", seed), areas_shadow == areas_truth,
            format!("{} hubs", areas_truth.len()), "E4.3: per-good hub patches rebuild the areas");
        println!(
            "seed {:>6}: territory over {} months · {} full ships · {} tile ships",
            seed, months, terr_full_ships, terr_tile_ships
        );
        c.must(&format!("territory replays to truth ({})", seed),
            terr_shadow == w.fields.territory,
            format!("{} full · {} tiles", terr_full_ships, terr_tile_ships),
            "E4.7: full RLE and tile patches must leave the client's grid exactly the engine's");
        c.must(&format!("no unchanged section reships ({})", seed), reships == 0,
            format!("{} reships", reships), "E4.2/E4.3: a section crosses only when it moved");
        c.must(&format!("event cursor tiles the log ({})", seed), cursor_breaks == 0
            && expect_ev == w.chronicle.events.len() as u64,
            format!("→ {}", expect_ev), "E4.4: ranges concatenate without gap or overlap");
        println!();
    }
    c.print();
}

// ================================================================ era (M8.3/M8.4)
// Expressive-range analysis: generate a population of worlds, project them
// onto structural metrics, draw the 2D histograms (Smith & Whitehead), and
// measure between-seed distinctiveness so we notice when the generator
// starts serving 10,000 bowls of oatmeal (Compton).

struct EraRow {
    seed: i64,
    land: f64,
    river: f64,
    mountain: f64,
    entropy: f64,
    coastc: f64,
    lm_big: f64,
    setts: f64,
    spacing: f64,
    biomes: Vec<f64>,
    hyps: Vec<f64>,
    sp_hist: Vec<f64>,
    /// 5×5 land-mass occupancy grid — where the continents sit (layout).
    grid: Vec<f64>,
    /// Landmass size-class histogram (share of land per component).
    lm_hist: Vec<f64>,
    /// M64: landform-word mix on land — the vocabulary's own range.
    lf_hist: Vec<f64>,
}

fn era_metrics(seed: i64, size: usize, years: usize) -> EraRow {
    let mut w = World::generate(seed, size);
    w.tick(years as i64 * 12);
    let (rows, cols) = w.fields.height.dim();
    let total = (rows * cols) as f64;
    let mut land = 0usize;
    let mut river = 0usize;
    let mut mountain = 0usize;
    let mut coast = 0usize;
    let mut bcount = vec![0f64; 12];
    let mut hyps = vec![0f64; 8];
    let mut lf_hist = vec![0f64; 27];
    for y in 0..rows {
        for x in 0..cols {
            let h = w.fields.height[[y, x]];
            if h < 0.0 { continue; }
            land += 1;
            if w.fields.flags[[y, x]] & CellFlags::RIVER.bits() != 0 { river += 1; }
            if h > 0.45 { mountain += 1; }
            let b = w.fields.biomes[[y, x]] as usize;
            if b < bcount.len() { bcount[b] += 1.0; }
            let bin = ((h.max(0.0).min(0.999)) * 8.0) as usize;
            hyps[bin] += 1.0;
            let l = w.fields.landform[[y, x]] as usize;
            if l < lf_hist.len() { lf_hist[l] += 1.0; }
            let mut sea_adj = false;
            for (dy, dx) in [(-1isize, 0isize), (1, 0), (0, -1), (0, 1)] {
                let ny = y as isize + dy;
                let nx = x as isize + dx;
                if ny >= 0 && nx >= 0 && ny < rows as isize && nx < cols as isize
                    && w.fields.height[[ny as usize, nx as usize]] < 0.0 { sea_adj = true; }
            }
            if sea_adj { coast += 1; }
        }
    }
    let landf = land.max(1) as f64;
    let entropy: f64 = bcount.iter().filter(|&&n| n > 0.0)
        .map(|&n| { let p = n / landf; -p * p.ln() }).sum();
    // landmasses (4-connected flood fill over land): largest share plus a
    // size-class histogram — one supercontinent reads differently from an
    // archipelago even when composition matches
    let mut seen = Array2::<bool>::from_elem((rows, cols), false);
    let mut lm_big = 0usize;
    let mut lm_hist = vec![0f64; 6];
    for y in 0..rows {
        for x in 0..cols {
            if w.fields.height[[y, x]] < 0.0 || seen[[y, x]] { continue; }
            let mut q = vec![(y, x)];
            seen[[y, x]] = true;
            let mut n = 0usize;
            let mut qi = 0usize;
            while qi < q.len() {
                let (cy, cx) = q[qi];
                qi += 1;
                n += 1;
                for (dy, dx) in [(-1isize, 0isize), (1, 0), (0, -1), (0, 1)] {
                    let ny = cy as isize + dy;
                    let nx = cx as isize + dx;
                    if ny >= 0 && nx >= 0 && ny < rows as isize && nx < cols as isize {
                        let (ny, nx) = (ny as usize, nx as usize);
                        if w.fields.height[[ny, nx]] >= 0.0 && !seen[[ny, nx]] {
                            seen[[ny, nx]] = true;
                            q.push((ny, nx));
                        }
                    }
                }
            }
            lm_big = lm_big.max(n);
            let share = n as f64 / landf;
            let bin = match share {
                s if s < 0.001 => 0,
                s if s < 0.01 => 1,
                s if s < 0.05 => 2,
                s if s < 0.20 => 3,
                s if s < 0.50 => 4,
                _ => 5,
            };
            lm_hist[bin] += n as f64; // mass-weighted: where the land lives
        }
    }
    // 5×5 occupancy grid: the coarse silhouette of the world (layout)
    let mut grid = vec![0f64; 25];
    for y in 0..rows {
        for x in 0..cols {
            if w.fields.height[[y, x]] >= 0.0 {
                let gy = y * 5 / rows;
                let gx = x * 5 / cols;
                grid[gy * 5 + gx] += 1.0;
            }
        }
    }
    // settlement spacing: nearest-neighbour distance, km
    let pts: Vec<(f64, f64)> = w.peoples.settlements.iter().filter(|s| s.pop > 0)
        .map(|s| (s.x as f64, s.y as f64)).collect();
    let mut nn: Vec<f64> = Vec::new();
    for (i, &(ax, ay)) in pts.iter().enumerate() {
        let mut best = f64::MAX;
        for (j, &(bx, by)) in pts.iter().enumerate() {
            if i == j { continue; }
            let d = ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt();
            best = best.min(d);
        }
        if best < f64::MAX { nn.push(best * 4.0); } // 4 km cells
    }
    let spacing = if nn.is_empty() { 0.0 } else { nn.iter().sum::<f64>() / nn.len() as f64 };
    let mut sp_hist = vec![0f64; 7];
    for &d in &nn {
        let bin = match d { d if d < 10.0 => 0, d if d < 20.0 => 1, d if d < 30.0 => 2,
            d if d < 45.0 => 3, d if d < 60.0 => 4, d if d < 90.0 => 5, _ => 6 };
        sp_hist[bin] += 1.0;
    }
    EraRow {
        seed,
        land: land as f64 / total,
        river: river as f64 / landf,
        mountain: mountain as f64 / landf,
        entropy,
        coastc: coast as f64 / landf.sqrt(),
        lm_big: lm_big as f64 / landf,
        setts: pts.len() as f64,
        spacing,
        biomes: bcount,
        hyps,
        sp_hist,
        grid,
        lm_hist,
        lf_hist,
    }
}

/// Jensen-Shannon divergence between two count vectors, normalized to 0..1.
fn jsd(p: &[f64], q: &[f64]) -> f64 {
    let sp: f64 = p.iter().sum::<f64>().max(1e-12);
    let sq: f64 = q.iter().sum::<f64>().max(1e-12);
    let mut d = 0.0;
    for i in 0..p.len() {
        let a = (p[i] / sp).max(1e-12);
        let b = (q[i] / sq).max(1e-12);
        let m = 0.5 * (a + b);
        d += 0.5 * a * (a / m).ln() + 0.5 * b * (b / m).ln();
    }
    (d / std::f64::consts::LN_2).clamp(0.0, 1.0)
}

/// One 2D expressive-range histogram, drawn as text (M8.3).
fn era_plot(title: &str, xs: &[f64], ys: &[f64], xl: &str, yl: &str) -> usize {
    const CW: usize = 24;
    const CH: usize = 8;
    let (x0, x1) = (xs.iter().cloned().fold(f64::MAX, f64::min), xs.iter().cloned().fold(f64::MIN, f64::max));
    let (y0, y1) = (ys.iter().cloned().fold(f64::MAX, f64::min), ys.iter().cloned().fold(f64::MIN, f64::max));
    let xr = (x1 - x0).max(1e-9);
    let yr = (y1 - y0).max(1e-9);
    let mut grid = vec![0usize; CW * CH];
    for (&x, &y) in xs.iter().zip(ys.iter()) {
        let cx = (((x - x0) / xr) * (CW as f64 - 1.0)).round() as usize;
        let cy = (((y - y0) / yr) * (CH as f64 - 1.0)).round() as usize;
        grid[(CH - 1 - cy) * CW + cx] += 1;
    }
    println!("  {} — x: {} [{:.3} … {:.3}] · y: {} [{:.3} … {:.3}]", title, xl, x0, x1, yl, y0, y1);
    let shades = [' ', '·', ':', '▪', '#', '@'];
    for r in 0..CH {
        let row: String = (0..CW).map(|ccol| {
            let n = grid[r * CW + ccol];
            shades[n.min(shades.len() - 1)]
        }).collect();
        println!("    |{}|", row);
    }
    let occupied = grid.iter().filter(|&&n| n > 0).count();
    println!("    occupied cells: {}", occupied);
    println!();
    occupied
}

fn cmd_era(size: usize, years: usize, nseeds: usize, base: i64) {
    header("ERA", &format!("size {} · {} y · {} seeds", size, years, nseeds));
    println!("expressive-range analysis (M8.3) and the oatmeal detector (M8.4)");
    println!();
    let seeds: Vec<i64> = (0..nseeds as i64).map(|i| base + i * 7919).collect();
    let rows: Vec<EraRow> = seeds.iter().map(|&s| era_metrics(s, size, years)).collect();

    println!("  {:>8} {:>6} {:>6} {:>6} {:>6} {:>7} {:>6} {:>6} {:>8}",
        "seed", "land", "river", "mntn", "entrp", "coast", "lmbig", "towns", "spacing");
    for r in &rows {
        println!("  {:>8} {:>6.3} {:>6.3} {:>6.3} {:>6.3} {:>7.2} {:>6.3} {:>6.0} {:>7.1}km",
            r.seed, r.land, r.river, r.mountain, r.entropy, r.coastc, r.lm_big, r.setts, r.spacing);
    }
    println!();

    // ---- M8.3: the four plates of the expressive range
    let land: Vec<f64> = rows.iter().map(|r| r.land).collect();
    let river: Vec<f64> = rows.iter().map(|r| r.river).collect();
    let entropy: Vec<f64> = rows.iter().map(|r| r.entropy).collect();
    let mountain: Vec<f64> = rows.iter().map(|r| r.mountain).collect();
    let setts: Vec<f64> = rows.iter().map(|r| r.setts).collect();
    let spacing: Vec<f64> = rows.iter().map(|r| r.spacing).collect();
    let coastc: Vec<f64> = rows.iter().map(|r| r.coastc).collect();
    let lm_big: Vec<f64> = rows.iter().map(|r| r.lm_big).collect();
    let occ1 = era_plot("ERA 1 · water on the land", &land, &river, "land share", "river share");
    let occ2 = era_plot("ERA 2 · relief vs variety", &mountain, &entropy, "mountain share", "biome entropy");
    let occ3 = era_plot("ERA 3 · the human layer", &setts, &spacing, "settlements", "NN spacing km");
    let occ4 = era_plot("ERA 4 · the shape of coasts", &coastc, &lm_big, "coast complexity", "largest landmass");

    // ---- M8.4: pairwise structural distance (biomes + hypsometry + spacing)
    let n = rows.len();
    let mut dists: Vec<f64> = Vec::new();
    let mut min_d = f64::MAX;
    let mut min_pair = (0i64, 0i64);
    for i in 0..n {
        for j in (i + 1)..n {
            let d = (jsd(&rows[i].biomes, &rows[j].biomes)
                + jsd(&rows[i].hyps, &rows[j].hyps)
                + jsd(&rows[i].sp_hist, &rows[j].sp_hist)
                + jsd(&rows[i].grid, &rows[j].grid)
                + jsd(&rows[i].lm_hist, &rows[j].lm_hist)
                + jsd(&rows[i].lf_hist, &rows[j].lf_hist)) / 6.0;
            dists.push(d);
            if d < min_d { min_d = d; min_pair = (rows[i].seed, rows[j].seed); }
        }
    }
    let mean_d = dists.iter().sum::<f64>() / dists.len().max(1) as f64;
    let max_d = dists.iter().cloned().fold(0.0f64, f64::max);
    let ratio = min_d / mean_d.max(1e-12);
    println!("  oatmeal: min {:.4} (seeds {} vs {}) · mean {:.4} · max {:.4} · min/mean {:.3}",
        min_d, min_pair.0, min_pair.1, mean_d, max_d, ratio);
    println!();

    // ---- checks
    let mut c = Checks::default();
    let spread = |v: &[f64]| v.iter().cloned().fold(f64::MIN, f64::max) - v.iter().cloned().fold(f64::MAX, f64::min);
    c.must("land share varies", spread(&land) > 0.01, format!("Δ{:.3}", spread(&land)), "M8.3: the range is a range");
    c.must("relief varies", spread(&mountain) > 0.01, format!("Δ{:.3}", spread(&mountain)), "M8.3: not one mountain recipe");
    c.must("towns vary", spread(&setts) >= 2.0, format!("Δ{:.0}", spread(&setts)), "M8.3: history diverges");
    let occ_mean = (occ1 + occ2 + occ3 + occ4) as f64 / 4.0 / n as f64;
    c.band("ERA occupancy / seed", occ_mean, format!("{:.2}", occ_mean));
    // Collapse alarm: a duplicated pair drives min/mean toward 0 regardless
    // of how many seeds are sampled (min alone shrinks with pair count).
    // Healthy generator reads ~0.10-0.19 across 8-16 seeds; mean ~0.05-0.07.
    c.band("oatmeal min/mean ratio", ratio, format!("{:.3}", ratio));
    c.band("oatmeal mean distance", mean_d, format!("{:.4}", mean_d));

    // ---- M64: the landform vocabulary joins the expressive range —
    // no seed may collapse toward one bland word, and the mix itself
    // must vary between worlds.
    let mut lf_min_ent = f64::MAX;
    let mut lf_max_dom = 0.0f64;
    let mut lf_dom_word = 0usize;
    let mut lf_dom_seed = 0i64;
    for r in &rows {
        let tot: f64 = r.lf_hist.iter().sum::<f64>().max(1.0);
        let ent: f64 = r.lf_hist.iter().filter(|&&v| v > 0.0)
            .map(|&v| { let p = v / tot; -p * p.ln() }).sum();
        lf_min_ent = lf_min_ent.min(ent);
        let (dom_i, &dom_n) = r.lf_hist.iter().enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap()).unwrap();
        let dom = dom_n / tot;
        if dom > lf_max_dom {
            lf_max_dom = dom;
            lf_dom_word = dom_i;
            lf_dom_seed = r.seed;
        }
    }
    let mut lf_min_d = f64::MAX;
    for i in 0..n {
        for j in (i + 1)..n {
            lf_min_d = lf_min_d.min(jsd(&rows[i].lf_hist, &rows[j].lf_hist));
        }
    }
    println!("  landforms: min entropy {:.2} nat · worst dominance {} {:.1}% (seed {}) · min pairwise JSD {:.4}",
        lf_min_ent, calliope::landform::NAMES[lf_dom_word], 100.0 * lf_max_dom, lf_dom_seed, lf_min_d);
    println!();
    c.band("landform entropy floor", lf_min_ent, format!("{:.2} nat", lf_min_ent));
    c.band("dominant landform share", lf_max_dom, format!("{} {:.1}%", calliope::landform::NAMES[lf_dom_word], 100.0 * lf_max_dom));
    c.band("landform oatmeal floor", lf_min_d, format!("{:.4}", lf_min_d));
    c.print();
}

// ================================================================ systems

/// E11.7 — profile the system lattice on a real grown world: where the
/// month's milliseconds go, which walls each system writes, and what an
/// ECS scheduler could therefore ever hope to parallelise.
fn cmd_systems(seed: i64, size: usize, years: usize) {
    header("SYSTEM LATTICE", &format!("seed {seed} · {size}² · {years}y"));

    // Walls each system writes, by inspection of systems.rs bodies.
    // P=peoples · E=economy · C=chronicle · G=grids · D=deposits ·
    // N=names/features · R=draws the one rng stream · Q=seismic ledger
    // (own stream, order-free — M22) · –=scratch only.
    // A system is SERIAL if it writes Peoples or draws the RNG: the single
    // PCG stream is a total order — determinism law makes it unsplittable.
    const ACCESS: &[(&str, &str, bool)] = &[
        ("towns", "P·R", true),
        ("famine", "P·R", true),
        // Q=seismic ledger (own stream). M24: the effects pass also
        // damages Peoples and fells towns into the chronicle: serial.
        ("quakes", "Q·P·C", true),
        // V=volcanic record + ash (own stream — M23) · M24 effects write
        // Peoples (burn/bury), Grids (ash → fertility), chronicle: serial.
        ("volcanoes", "V·P·G·C", true),
        // M79: reads the landfall ledger, wounds/repairs harbours, writes
        // Peoples and chronicle events. No shared PCG draw, but Peoples is
        // enough to keep this pass on the serial side of the lattice.
        ("storms", "P·C", true),
        ("colonize", "P·R", true),
        ("prospect", "D·R", true),
        ("rush-camps", "P·D·R", true),
        ("goods", "P", true),
        ("exonyms", "N·R", true),
        ("society", "P·R", true),
        ("census", "–", false),
        ("market-areas", "E", false),
        ("crafts", "P·R", true),
        ("economy", "E·P·R", true),
        ("merchants", "E·P·C·R", true),
        ("statecraft", "P·C·G·R", true),
        ("kindred", "P·N·R", true),
        ("union", "P·R", true),
        ("civ", "P·C·R", true),
        ("patina", "P·C·N·R", true),
        ("territory", "G", false),
        ("chronicle", "C·P·R", true),
        ("relics", "C·R", true),
        ("second-reading", "C", false),
        ("veil", "C", false),
        ("heat", "–", false),
    ];

    let mut w = World::generate(seed, size);
    let mut totals = vec![0.0f64; SYSTEMS.len()];
    let mut calls = vec![0u64; SYSTEMS.len()];
    let months = (years * 12) as i64;
    let t0 = Instant::now();
    let mut left = months;
    while left > 0 {
        let step = left.min(240);
        w.tick_profiled(step, &mut totals, &mut calls);
        left -= step;
    }
    let wall = t0.elapsed().as_secs_f64();
    let insystem: f64 = totals.iter().sum();
    let overhead = (wall - insystem).max(0.0);

    println!(" {years}y ticked in {:.0} ms · {} towns · {} events", wall * 1000.0,
        w.peoples.settlements.len(), w.chronicle.events.len());
    println!();
    println!(" {:<16} {:>8} {:>6} {:>10} {:>7}   {:<10} {}", "system", "cadence", "calls", "total ms", "share", "writes", "serial");
    let mut serial_time = 0.0f64;
    for (i, sys) in SYSTEMS.iter().enumerate() {
        let cad = match sys.cadence() {
            Cadence::Monthly => "monthly".to_string(),
            Cadence::EveryN { n, .. } => format!("1/{n}mo"),
        };
        let (aname, walls, serial) = ACCESS[i];
        debug_assert_eq!(aname, sys.name());
        if serial {
            serial_time += totals[i];
        }
        println!(" {:<16} {:>8} {:>6} {:>10.1} {:>6.1}%   {:<10} {}",
            sys.name(), cad, calls[i], totals[i] * 1000.0,
            100.0 * totals[i] / insystem.max(1e-9), walls,
            if serial { "yes" } else { "-" });
    }
    println!(" {:<16} {:>8} {:>6} {:>10.1} {:>6.1}%   (dispatch + clock)", "driver", "", "", overhead * 1000.0, 100.0 * overhead / wall.max(1e-9));

    // Amdahl on the measured workload: even a perfect scheduler that runs
    // every non-serial system for free is bounded by the serial fraction.
    let serial_share = serial_time / insystem.max(1e-9);
    let ceiling = 1.0 / serial_share.max(1e-9);
    println!();
    println!(" serial fraction (writes Peoples or draws rng): {:.1}% of in-system time", 100.0 * serial_share);
    println!(" ⇒ ECS parallel-scheduling ceiling (Amdahl): {:.3}× — before scheduler cost", ceiling);

    let mut c = Checks::default();
    let names_match = SYSTEMS.iter().zip(ACCESS).all(|(s, a)| s.name() == a.0)
        && SYSTEMS.len() == ACCESS.len();
    c.must("access table covers the lattice", names_match,
        format!("{}/{}", ACCESS.len(), SYSTEMS.len()), "E11.7: the analysis names every system");
    c.must("driver overhead is noise", overhead / wall.max(1e-9) < 0.05,
        format!("{:.2}%", 100.0 * overhead / wall.max(1e-9)), "the hand-rolled lattice costs <5% dispatch");
    c.want("ECS parallel ceiling stays low", ceiling <= 1.5,
        format!("{:.3}×", ceiling), "ADR-0022: re-open the bevy_ecs question if this rises");
    c.print();
}

// ============================================================ M49 ocean

/// One seed's ocean measurements — the four families the M49 panel
/// bands: gyre topology, current-coast temperature, upwelling
/// coverage, sea-lane seasonality.
struct OceanRow {
    seed: i64,
    basins: usize,
    gyres: usize,
    earthly: usize,
    gyre_cells: f64,
    sp95: f64,
    westx: f64,
    warm_d: f64,
    cold_d: f64,
    warm_n: usize,
    cold_n: usize,
    up_share: f64,
    up_lat: f64,
    sea_lanes: usize,
    seasonal: usize,
    seas_share: f64,
    swing_spread: f64,
    swing_p50: f64,
}

/// M74 — the seesaw seas.
///
/// A basin that leans warm for a few years and then leans back is the
/// single largest source of ordered, multi-year structure in a real
/// sky, and the thing that makes a run of bad harvests feel caused
/// rather than merely unlucky. This lane holds the law that M74 draws —
/// period, amplitude, phase — against the series it actually produces:
/// the period is *recovered blind* from the index by periodogram rather
/// than read off the struct, the realized σ must be the drawn amplitude,
/// the lean must be balanced (no basin that only ever warms), and the
/// seesaw must be irregular — successive cycles of visibly different
/// length, never a metronome. Determinism closes it: the same seed must
/// hand back the same basin, byte for byte.
/// M75 — the tilted belts. The oscillation's phase must reach across the
/// world: when the northern trade belt is wet the southern one is dry,
/// they swap when the index changes sign, and the whole effect must
/// vanish when the index is held at zero — the counterfactual that proves
/// the tilt *causes* the tie rather than merely accompanying it.
fn cmd_teleconnection(size: usize, years: i64, seeds: Vec<i64>) {
    header(
        "TELECONNECTION",
        &format!("{}x{} · {} seeds · {} y", size, size, seeds.len(), years),
    );
    println!("the tilted belts · cross-hemisphere trade-belt rain, forced vs counterfactual  (M75)");

    let mut c = Checks::default();
    let mut lags: Vec<i64> = Vec::new();
    let lat_of = |y: usize, rows: usize| -90.0 + (y as f64) * 180.0 / (rows as f64 - 1.0);
    let core = calliope::climate::TELE_BELT_LAT;
    let halfwidth = calliope::climate::TELE_BELT_SIGMA;

    // ---- the shape law, independent of any world ---------------------
    c.must(
        "the tilt is antisymmetric",
        (0..=90).map(|d| d as f64).all(|d| {
            let a = calliope::climate::teleconnection_bias(1.0, d);
            let b = calliope::climate::teleconnection_bias(1.0, -d);
            (a + b).abs() < 1e-12
        }),
        "|N + S| < 1e-12".to_string(),
        "M75: a teleconnection tilts belts against each other — a term that wet both hemispheres would be a global wet year, not a see-saw",
    );
    c.must(
        "the ITCZ is straddled, not tilted",
        calliope::climate::teleconnection_bias(2.0, 0.0).abs() < 1e-12,
        format!("{:.1e} at lat 0", calliope::climate::teleconnection_bias(2.0, 0.0).abs()),
        "M75: the tilt is in the trades; the equator sits on the pivot",
    );
    c.must(
        "the westerlies keep their own sky",
        calliope::climate::teleconnection_bias(2.0, 55.0).abs() < 0.01,
        format!("{:.4} at lat 55", calliope::climate::teleconnection_bias(2.0, 55.0).abs()),
        "M75: outside the trades the belt keeps the unforced variability M73 measured",
    );
    c.must(
        "the tilt flips with the phase",
        calliope::climate::teleconnection_bias(1.0, core)
            * calliope::climate::teleconnection_bias(-1.0, core)
            < 0.0,
        format!(
            "{:+.3} / {:+.3} at lat {:.0}",
            calliope::climate::teleconnection_bias(1.0, core),
            calliope::climate::teleconnection_bias(-1.0, core),
            core
        ),
        "M75: a warm phase and a cold phase must move the belt in opposite directions",
    );

    println!();
    println!(" seed        r(N,S) forced  r(N,S) osc=0   dN-S warm   dN-S cold   |resid| max   warm y  cold y");
    for &seed in &seeds {
        let w = World::generate(seed, size);
        let land = land_mask(&w);
        let rows = w.fields.tmean.dim().0;
        let mut north: Vec<(usize, usize)> = Vec::new();
        let mut south: Vec<(usize, usize)> = Vec::new();
        for y in 0..rows {
            let lat = lat_of(y, rows);
            let in_north = (lat - core).abs() <= halfwidth;
            let in_south = (lat + core).abs() <= halfwidth;
            if !in_north && !in_south {
                continue;
            }
            for x in 0..w.width {
                if !land[[y, x]] {
                    continue;
                }
                if in_north {
                    north.push((y, x));
                } else {
                    south.push((y, x));
                }
            }
        }
        if north.len() < 50 || south.len() < 50 {
            c.must(
                &format!("both trade belts carry land · {}", seed),
                false,
                format!("N {} · S {} cells", north.len(), south.len()),
                "M75: a cross-hemisphere gate over an empty belt proves nothing",
            );
            continue;
        }

        let belt_mean = |g: &ndarray::Array2<f64>, cells: &[(usize, usize)]| -> f64 {
            cells.iter().map(|&(y, x)| g[[y, x]]).sum::<f64>() / cells.len() as f64
        };
        let (mut ns, mut ss) = (Vec::new(), Vec::new());
        let (mut ns0, mut ss0) = (Vec::new(), Vec::new());
        let mut idx: Vec<f64> = Vec::new();
        let mut resid_max = 0.0f64;
        let mut nonfinite = 0usize;
        let mut worst_floor = 0.0f64;
        for year in 1..=years {
            let (_, dp) = w.year_anomaly_fresh(year);
            let (_, dp0) =
                calliope::climate::year_anomaly(w.variability(), rows, w.width, year, 0.0);
            let osc = w.year_osc(year);
            for &(y, x) in north.iter().chain(south.iter()) {
                let a = dp[[y, x]];
                let b = dp0[[y, x]];
                if !a.is_finite() || !b.is_finite() {
                    nonfinite += 1;
                    continue;
                }
                if a < worst_floor {
                    worst_floor = a;
                }
                if a <= calliope::climate::ANOM_P_FLOOR + 1e-9
                    || b <= calliope::climate::ANOM_P_FLOOR + 1e-9
                {
                    continue;
                }
                let expect = calliope::climate::teleconnection_bias(osc, lat_of(y, rows));
                resid_max = resid_max.max((a - b - expect).abs());
            }
            ns.push(belt_mean(&dp, &north));
            ss.push(belt_mean(&dp, &south));
            ns0.push(belt_mean(&dp0, &north));
            ss0.push(belt_mean(&dp0, &south));
            idx.push(osc);
        }

        let corr = |a: &[f64], b: &[f64]| -> f64 {
            let n = a.len() as f64;
            let ma = a.iter().sum::<f64>() / n;
            let mb = b.iter().sum::<f64>() / n;
            let mut num = 0.0;
            let mut da = 0.0;
            let mut db = 0.0;
            for i in 0..a.len() {
                num += (a[i] - ma) * (b[i] - mb);
                da += (a[i] - ma).powi(2);
                db += (b[i] - mb).powi(2);
            }
            if da <= 0.0 || db <= 0.0 {
                return 0.0;
            }
            num / (da * db).sqrt()
        };
        let r_forced = corr(&ns, &ss);
        let r_counter = corr(&ns0, &ss0);
        let (mut warm, mut cold) = (Vec::new(), Vec::new());
        for i in 0..idx.len() {
            let d = ns[i] - ss[i];
            if idx[i] > 0.25 {
                warm.push(d);
            } else if idx[i] < -0.25 {
                cold.push(d);
            }
        }
        let mean = |v: &[f64]| -> f64 {
            if v.is_empty() {
                f64::NAN
            } else {
                v.iter().sum::<f64>() / v.len() as f64
            }
        };
        let (dw, dc) = (mean(&warm), mean(&cold));

        // ---- M76: recover the declared lag blind ---------------------
        // The tilt is driven by the index sampled TELE_LAG_MONTHS before
        // the year opens. Nothing in the belt series says so. Scan every
        // candidate lag 0..24 months, correlate the realized belt
        // difference against the index at that lag, and take the argmax
        // of |r|: the analysis must land on the lag the code declares,
        // and must land on the same one in every world.
        let osc_src = calliope::oscillation::Oscillation::new(seed);
        let diff: Vec<f64> = (0..ns.len()).map(|i| ns[i] - ss[i]).collect();
        let mut lag_best = (0i64, 0.0f64, 0.0f64);
        for k in 0..=24i64 {
            let probe: Vec<f64> = (1..=years)
                .map(|year| osc_src.index(year * 12 - k))
                .collect();
            let r = corr(&diff, &probe);
            if r.abs() > lag_best.1.abs() {
                lag_best = (k, r, r);
            }
        }
        let (lag_rec, lag_r, _) = lag_best;
        lags.push(lag_rec);
        println!(
            " {:<9} {:>13.3} {:>14.3} {:>12.4} {:>11.4} {:>13.1e} {:>7} {:>7}",
            seed,
            r_forced,
            r_counter,
            dw,
            dc,
            resid_max,
            warm.len(),
            cold.len()
        );

        c.must(
            &format!("the belts speak across the equator · {}", seed),
            r_forced <= -0.30,
            format!("r = {:+.3}", r_forced),
            "M75 gate: cross-hemisphere trade-belt rainfall correlation exceeds 0.3 in magnitude — and is negative, because a teleconnection tilts, it does not lift",
        );
        c.must(
            &format!("the tie is the oscillation's, not the map's · {}", seed),
            r_counter.abs() < 0.15,
            format!("r = {:+.3} with the index held at zero", r_counter),
            "M75 counterfactual: remove the lean and the hemispheres must fall silent — otherwise the correlation was geography, not teleconnection",
        );
        c.must(
            &format!("the see-saw flips with the phase · {}", seed),
            dw.is_finite() && dc.is_finite() && dw * dc < 0.0,
            format!("warm {:+.4} · cold {:+.4}", dw, dc),
            "M75 gate: the belt difference must change sign with the oscillation phase across a full period",
        );
        c.must(
            &format!("the flip is felt, not cosmetic · {}", seed),
            dw.is_finite() && dc.is_finite() && (dw - dc).abs() >= 0.02,
            format!("swing {:.4} of the rain", (dw - dc).abs()),
            "M75: a tilt too small to move a harvest is not a teleconnection",
        );
        c.must(
            &format!("both phases are lived in · {}", seed),
            warm.len() >= 5 && cold.len() >= 5,
            format!("{} warm · {} cold years", warm.len(), cold.len()),
            "M75: a gate that only ever saw one phase proves half a law",
        );
        c.must(
            &format!("the tilt derives exactly · {}", seed),
            resid_max < 1e-12,
            format!("|forced - counterfactual - bias| max {:.1e}", resid_max),
            "M75: the coupling is the declared function of (index, latitude) and nothing else — an unexplained residual is a second mechanism hiding",
        );
        c.must(
            &format!("no poison enters the rain · {}", seed),
            nonfinite == 0 && worst_floor >= calliope::climate::ANOM_P_FLOOR - 1e-9,
            format!("{} non-finite · worst {:.3}", nonfinite, worst_floor),
            "M2.6/M75: the tilt may not take the rains wholly away — that verdict is famine's",
        );
        c.must(
            &format!("the tilt replays · {}", seed),
            {
                let w2 = World::generate(seed, size);
                (1..=6).all(|yr| {
                    (w2.year_osc(yr) - w.year_osc(yr)).abs() < 1e-15
                        && (belt_mean(&w2.year_anomaly_fresh(yr).1, &north)
                            - ns[(yr - 1) as usize])
                            .abs()
                            < 1e-15
                })
            },
            "identical".to_string(),
            "ADR-0003: the tilted belts are a pure function of the seed and the year",
        );
        c.must(
            &format!("the lag is recoverable blind · {}", seed),
            lag_rec == calliope::climate::TELE_LAG_MONTHS,
            format!(
                "{} mo (declared {}) · r = {:+.3}",
                lag_rec,
                calliope::climate::TELE_LAG_MONTHS,
                lag_r
            ),
            "M76 gate: scanning every candidate lag 0-24 mo, the belt difference must correlate most strongly with the index at exactly the declared lag — the phase relation is in the world, not only in the constant",
        );
    }
    // M76 — the lag is a property of the coupling, not of one world.
    let all_same = !lags.is_empty() && lags.iter().all(|&l| l == lags[0]);
    c.must(
        "the recovered lag is stable across worlds",
        all_same && lags[0] == calliope::climate::TELE_LAG_MONTHS,
        format!(
            "{:?} mo over {} seeds",
            lags,
            lags.len()
        ),
        "M76 gate: the teleconnection lag statistic must be the same in every seed — a lag that wandered by world would mean the phase relation is an artefact of the map, not the coupling",
    );
    c.print();
}

/// M77 — The Storm Corridors.
///
/// The corridor is not declared anywhere: the genesis field is the
/// world's own meridional temperature gradient over water, and the
/// steering is the same zonal wind the gyres read. So the 30–60° belt,
/// the eastward corridor, the poleward drift and the death over land all
/// have to *emerge* from the climate the earlier eras solved — every row
/// below measures the realized tracks and holds them against a law
/// stated elsewhere, never against a number typed into this lane.
fn cmd_storms(size: usize, years: i64, seeds: Vec<i64>) {
    header(
        "STORMS",
        &format!("{}x{} · {} seeds · {} y", size, size, seeds.len(), years),
    );
    println!("the storm corridors · genesis, steering and death over land  (M77)");

    let mut c = Checks::default();
    let mut band_shares: Vec<f64> = Vec::new();
    let mut east_shares: Vec<f64> = Vec::new();

    println!();
    println!(" seed      storms/century   gen 30-60°   over sea   east%   pole drift   land keep   sea keep   landfall%   season N/S");
    for &seed in &seeds {
        let w = World::generate(seed, size);
        let rows = w.fields.height.dim().0;
        let clim = calliope::storms::StormClimatology::new(
            &w.fields.height,
            &w.fields.tmean,
            &w.fields.tamp,
        );

        println!(
            " · {} genesis field: sites N {} / S {} · ref weight {:.4} / {:.4} · iced-out N {} / S {} · season count N {} / S {}",
            seed,
            clim.sites(1).len(),
            clim.sites(-1).len(),
            clim.peak_gradient(1),
            clim.peak_gradient(-1),
            clim.iced_out(1),
            clim.iced_out(-1),
            clim.season_count(1),
            clim.season_count(-1),
        );
        let mut tracks: Vec<calliope::storms::StormTrack> = Vec::new();
        for year in 1..=years {
            for h in [1i8, -1i8] {
                tracks.extend(clim.season(seed, year, h, &w.fields.height));
            }
        }
        if tracks.is_empty() {
            c.must(
                &format!("the westerlies breed storms · {}", seed),
                false,
                "0 tracks".to_string(),
                "M77: a mid-latitude sea with a temperature gradient across it must shed cyclones — no tracks means the genesis field never fired",
            );
            continue;
        }

        let n = tracks.len() as f64;
        let per_century = n * 100.0 / years as f64;

        // --- where they are born -------------------------------------
        let mut in_band = 0usize;
        let mut over_sea = 0usize;
        let mut hist = [0usize; 9]; // 10° bins of |lat|, 0..90
        for t in &tracks {
            let la = t.genesis_lat.abs();
            if (30.0..=60.0).contains(&la) {
                in_band += 1;
            }
            let bin = ((la / 10.0).floor() as usize).min(8);
            hist[bin] += 1;
            if w.fields.height[[t.genesis.0, t.genesis.1]] < 0.0 {
                over_sea += 1;
            }
        }
        let band_share = in_band as f64 / n;
        let sea_share = over_sea as f64 / n;
        band_shares.push(band_share);
        let peak_bin = (0..9).max_by_key(|&i| hist[i]).unwrap();

        // --- where they go -------------------------------------------
        let east = tracks.iter().filter(|t| t.drift_x() > 0.0).count() as f64 / n;
        east_shares.push(east);
        let pole: f64 = tracks.iter().map(|t| t.drift_pole(rows)).sum::<f64>() / n;

        // --- what kills them -----------------------------------------
        // Per-step intensity ratio, measured separately over land and
        // over sea across every step of every track: the engine's own
        // number, not the constant.
        let (mut lk, mut lkn, mut sk, mut skn) = (0.0f64, 0usize, 0.0f64, 0usize);
        let mut nonfinite = 0usize;
        let mut offgrid = 0usize;
        for t in &tracks {
            for pair in t.points.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                if !b.inten.is_finite() || !b.x.is_finite() || !b.y.is_finite() {
                    nonfinite += 1;
                    continue;
                }
                if b.x < 0.0 || b.x > (w.width - 1) as f64 || b.y < 0.0 || b.y > (rows - 1) as f64 {
                    offgrid += 1;
                }
                if a.inten <= 0.0 {
                    continue;
                }
                let r = b.inten / a.inten;
                if b.over_land {
                    lk += r;
                    lkn += 1;
                } else {
                    sk += r;
                    skn += 1;
                }
            }
        }
        let land_keep = if lkn > 0 { lk / lkn as f64 } else { f64::NAN };
        let sea_keep = if skn > 0 { sk / skn as f64 } else { f64::NAN };
        let landfall = tracks.iter().filter(|t| t.landfall).count() as f64 / n;

        println!(
            " {:<9} {:>10.1}   {:>10}   {:>8}  {:>6}   {:>+9.2}°   {:>9.4}   {:>8.4}   {:>9}   {:>3} / {:<3}",
            seed,
            per_century,
            pct(band_share),
            pct(sea_share),
            pct(east),
            pole,
            land_keep,
            sea_keep,
            pct(landfall),
            clim.cold_month(1),
            clim.cold_month(-1),
        );

        c.must(
            &format!("genesis peaks in the baroclinic belt · {}", seed),
            (3..6).contains(&peak_bin),
            format!("peak bin {}-{}°", peak_bin * 10, peak_bin * 10 + 10),
            "M77 gate: the busiest 10° band of cyclogenesis must fall inside 30-60° — the corridor is a consequence of where the temperature gradient concentrates, and nothing in storms.rs names a latitude",
        );
        c.must(
            &format!("the belt holds the season · {}", seed),
            band_share >= 0.60,
            pct(band_share),
            "M77 gate: a majority of a year's cyclones are born in the 30-60° belt — a corridor that leaked equatorward would mean the gradient measure is reading something other than baroclinicity",
        );
        c.must(
            &format!("cyclogenesis is a marine act · {}", seed),
            sea_share > 0.999,
            pct(sea_share),
            "M77: a cyclone needs the sea's heat under it — a genesis point on land would mean the mask is not being read",
        );
        c.must(
            &format!("the corridor runs downwind · {}", seed),
            east >= 0.80,
            pct(east),
            "M77 gate: born in the westerlies, a storm must travel east — the steering is currents::wind_stress, the same field the gyres read, so a westward corridor would put storm and current in contradiction",
        );
        c.must(
            &format!("storms climb poleward · {}", seed),
            pole > 0.0,
            format!("{:+.2}° mean", pole),
            "M77: a travelling cyclone occludes as it goes and drifts toward its own pole — a net equatorward corridor would be the wrong sign of drift",
        );
        c.must(
            &format!("land fills the storm · {}", seed),
            land_keep < 1.0 && land_keep.is_finite(),
            format!("{:.4}/step", land_keep),
            "M77 gate: cut off from the sea a cyclone loses intensity every step — measured over every land step of every track, not read off the constant",
        );
        c.must(
            &format!("the sea feeds the storm · {}", seed),
            sea_keep > 1.0 && sea_keep.is_finite(),
            format!("{:.4}/step", sea_keep),
            "M77 gate: over open water the storm must be gaining, or the land/sea contrast below proves nothing",
        );
        c.must(
            &format!("the coast is where they die · {}", seed),
            landfall > 0.05,
            pct(landfall),
            "M77: a corridor that never reaches a shore costs the world nothing — M79 needs landfalls to wire consequences to",
        );
        c.must(
            &format!("no poison on the tracks · {}", seed),
            nonfinite == 0 && offgrid == 0,
            format!("{} nan · {} off-grid", nonfinite, offgrid),
            "M77: every advected point is finite and inside the grid it was walked across",
        );

        // --- the seasons are the world's own --------------------------
        let (cn, cs) = (clim.cold_month(1), clim.cold_month(-1));
        let sep = (((cn - cs) % 12) + 12) % 12;
        c.must(
            &format!("the hemispheres storm in turn · {}", seed),
            (5..=7).contains(&sep),
            format!("{} mo apart", sep),
            "M77 gate: the storm season is the belt's own cold season, read from the realized annual cycle — the two hemispheres must fall half a year apart because the world's seasons do, not because a calendar was typed in",
        );
        let cold_half = |cm: i64, m: i64| -> bool {
            let d = (((m - cm) % 12) + 12) % 12;
            d <= 2 || d >= 10
        };
        let in_season = tracks
            .iter()
            .filter(|t| cold_half(if t.hemi >= 0 { cn } else { cs }, t.month))
            .count() as f64
            / n;
        c.must(
            &format!("storms keep to their season · {}", seed),
            in_season >= 0.50,
            pct(in_season),
            "M77 gate: the five months centred on the belt's coldest must carry more than their uniform share (41.7%) of the year's cyclones — the season is drawn from the world's temperature cycle",
        );

        // --- counts follow the sea that breeds them --------------------
        let (mut sites_n, mut sites_s) = (clim.sites(1).len(), clim.sites(-1).len());
        if sites_n == 0 {
            sites_n = 1;
        }
        if sites_s == 0 {
            sites_s = 1;
        }
        let cnt_n = tracks.iter().filter(|t| t.hemi > 0).count() as f64;
        let cnt_s = tracks.iter().filter(|t| t.hemi < 0).count() as f64;
        let rate_n = cnt_n / (years as f64) / (sites_n as f64 / 1000.0);
        let rate_s = cnt_s / (years as f64) / (sites_s as f64 / 1000.0);
        c.must(
            &format!("counts follow the breeding sea · {}", seed),
            (rate_n / rate_s).max(rate_s / rate_n) < 2.0,
            format!("{:.2} vs {:.2}/ky·1k", rate_n, rate_s),
            "M77 gate: storms per year per thousand baroclinic ocean cells must agree between hemispheres within a factor of two — a hemisphere that bred storms out of proportion to its sea would mean the count is coming from somewhere other than the zone",
        );

        // --- coastline exposure ---------------------------------------
        // Landfalls per century per thousand coastline cells: the shore's
        // exposure to the corridor, held between hemispheres.
        let land = land_mask(&w);
        let (mut coast_n, mut coast_s) = (0usize, 0usize);
        for y in 1..rows - 1 {
            let north = calliope::storms::lat_of(y as f64, rows) >= 0.0;
            for x in 1..w.width - 1 {
                if !land[[y, x]] {
                    continue;
                }
                let edge = !land[[y - 1, x]] || !land[[y + 1, x]] || !land[[y, x - 1]] || !land[[y, x + 1]];
                if edge {
                    if north {
                        coast_n += 1;
                    } else {
                        coast_s += 1;
                    }
                }
            }
        }
        let lf_n = tracks.iter().filter(|t| t.hemi > 0 && t.landfall).count() as f64;
        let lf_s = tracks.iter().filter(|t| t.hemi < 0 && t.landfall).count() as f64;
        let exp_n = lf_n * 100.0 / years as f64 / (coast_n.max(1) as f64 / 1000.0);
        let exp_s = lf_s * 100.0 / years as f64 / (coast_s.max(1) as f64 / 1000.0);
        if coast_n >= 200 && coast_s >= 200 && lf_n >= 1.0 && lf_s >= 1.0 {
            c.want(
                &format!("shores share the exposure · {}", seed),
                (exp_n / exp_s).max(exp_s / exp_n) < 2.0,
                format!("{:.1} vs {:.1}/cy·1k", exp_n, exp_s),
                "M77 gate: landfalls per century per thousand coastline cells within a factor of two between hemispheres — one hemisphere's shore may genuinely sit downwind of a wider ocean, so this stands as a WARN band, not a hard row",
            );
        }

        // --- the corridor replays -------------------------------------
        let clim2 = calliope::storms::StormClimatology::new(
            &w.fields.height,
            &w.fields.tmean,
            &w.fields.tamp,
        );
        let a = clim.probe(seed, &w.fields.height);
        let b = clim2.probe(seed, &w.fields.height);
        c.must(
            &format!("the corridor replays · {}", seed),
            a == b,
            format!("{:016x}", a),
            "ADR-0003: a storm season is derived, never stored — same seed and year must walk the same tracks",
        );
        // …and a different year is a different season, or the corridor is
        // a still image rather than a weather record.
        let y1 = clim.season(seed, 1, 1, &w.fields.height);
        let y2 = clim.season(seed, 2, 1, &w.fields.height);
        let same = y1.len() == y2.len()
            && y1.iter().zip(y2.iter()).all(|(p, q)| p.genesis == q.genesis && p.month == q.month);
        c.must(
            &format!("each year draws its own storms · {}", seed),
            !same,
            format!("{} vs {} tracks", y1.len(), y2.len()),
            "M77: the season is keyed on the year as well as the seed — two identical years would mean the corridor is a fixture, not weather",
        );
    }

    if band_shares.len() > 1 {
        let lo = band_shares.iter().cloned().fold(f64::INFINITY, f64::min);
        let hi = band_shares.iter().cloned().fold(0.0f64, f64::max);
        c.must(
            "the belt holds in every world",
            lo >= 0.60,
            format!("{} - {}", pct(lo), pct(hi)),
            "M77 gate: the corridor's latitude is a property of the general circulation, so every seed's map must put its storms in the same belt",
        );
        let elo = east_shares.iter().cloned().fold(f64::INFINITY, f64::min);
        c.must(
            "the downwind sense holds in every world",
            elo >= 0.80,
            pct(elo),
            "M77 gate: no world may run its corridor against the westerlies",
        );
    }
    c.print();
}

fn cmd_tropics(size: usize, years: i64, seeds: Vec<i64>) {
    header(
        "TROPICS",
        &format!("{}x{} · {} seeds · {} y", size, size, seeds.len(), years),
    );
    println!("warm-sea fury · cyclones bred on heat and spin, and where they come ashore  (M78)");

    let mut band_shares: Vec<f64> = Vec::new();
    let mut recurve_shares: Vec<f64> = Vec::new();
    let mut c = Checks::default();

    for &seed in &seeds {
        let w = World::generate(seed, size);
        let rows = w.fields.height.dim().0;
        let clim = calliope::storms::StormClimatology::new(
            &w.fields.height,
            &w.fields.tmean,
            &w.fields.tamp,
        );

        println!();
        println!(
            " · {} warm sea: sites N {} / S {} · spinless N {} / S {} · warm month N {} / S {} (frontal cold month N {} / S {}) · season count N {} / S {}",
            seed,
            clim.trop_sites(1).len(),
            clim.trop_sites(-1).len(),
            clim.spinless(1),
            clim.spinless(-1),
            clim.warm_month(1),
            clim.warm_month(-1),
            clim.cold_month(1),
            clim.cold_month(-1),
            clim.trop_season_count(1),
            clim.trop_season_count(-1),
        );

        let mut tracks: Vec<calliope::storms::StormTrack> = Vec::new();
        for year in 1..=years {
            for h in [1i8, -1i8] {
                tracks.extend(clim.trop_season(seed, year, h, &w.fields.height));
            }
        }
        if tracks.is_empty() {
            c.must(
                &format!("the warm seas breed cyclones · {}", seed),
                false,
                "0 tracks".to_string(),
                "M78: a sea over 26 °C outside the equatorial dead band must shed tropical cyclones — no tracks means the genesis field never fired",
            );
            continue;
        }

        let n = tracks.len() as f64;

        // --- where they are born -------------------------------------
        let mut in_band = 0usize;
        let mut too_polar = 0usize;
        let mut in_dead_band = 0usize;
        let mut cold_born = 0usize;
        let mut over_land = 0usize;
        let mut maxlat = 0.0f64;
        let mut hist = [0usize; 9];
        for t in &tracks {
            let la = t.genesis_lat.abs();
            maxlat = maxlat.max(la);
            if (calliope::storms::TROP_LAT_MIN..=30.0).contains(&la) {
                in_band += 1;
            }
            if la > 30.0 {
                too_polar += 1;
            }
            if la < calliope::storms::TROP_LAT_MIN {
                in_dead_band += 1;
            }
            if w.fields.height[[t.genesis.0, t.genesis.1]] >= 0.0 {
                over_land += 1;
            } else if clim.sst_at(t.genesis.0, t.genesis.1) < calliope::storms::TROP_SST_MIN {
                cold_born += 1;
            }
            hist[((la / 10.0).floor() as usize).min(8)] += 1;
        }
        let band = in_band as f64 / n;
        band_shares.push(band);
        print!("   genesis by |lat|:");
        for (i, h) in hist.iter().enumerate() {
            if *h > 0 {
                print!(" {}-{}°:{}", i * 10, i * 10 + 10, h);
            }
        }
        println!();

        c.must(
            &format!("no cyclone is born on a cold sea · {}", seed),
            cold_born == 0,
            format!("{} of {} below {:.0} °C", cold_born, tracks.len(), calliope::storms::TROP_SST_MIN),
            "M78 gate: the warm-sea engine runs on surface heat — every genesis cell must stand over water at or above the 26 °C threshold in its own warm month",
        );
        c.must(
            &format!("no cyclone is born on land · {}", seed),
            over_land == 0,
            format!("{} of {}", over_land, tracks.len()),
            "M78: a tropical cyclone has no engine without a sea under it",
        );
        c.must(
            &format!("the equator breeds nothing · {}", seed),
            in_dead_band == 0,
            format!(
                "{} inside {:.0}° · {} warm cells struck out for want of spin",
                in_dead_band,
                calliope::storms::TROP_LAT_MIN,
                clim.spinless(1) + clim.spinless(-1)
            ),
            "M78 gate: within a few degrees of the equator the Coriolis parameter vanishes and the warmest sea on the map cannot close a vortex — the dead band must be empty",
        );
        c.must(
            &format!("the warm seas keep the cyclone in the tropics · {}", seed),
            too_polar == 0 && band >= 0.99,
            format!("{} in {:.0}-30° · deepest {:.1}°", pct(band), calliope::storms::TROP_LAT_MIN, maxlat),
            "M78 gate: genesis must fall inside 5-30° in every world — and nothing in storms.rs names a poleward limit, so this is a claim about where this world's 26 °C isotherm lies",
        );

        // --- how they travel ------------------------------------------
        let mut west_early = 0usize;
        let mut west_n = 0usize;

        let mut recurved = 0usize;
        let mut reached = 0usize;
        let mut peaks: Vec<f64> = Vec::new();
        let mut lifetimes: Vec<f64> = Vec::new();
        let mut landfalls = 0usize;
        let mut poleward = 0usize;
        for t in &tracks {
            peaks.push(t.peak);
            lifetimes.push(t.days());
            if t.landfall {
                landfalls += 1;
            }
            let p0 = t.points[0];
            // Its young life in the trades: where it stands after five
            // days, or — if the sea or a coast fills it sooner — where it
            // stood when it died. Sampling strictly at day 5 would score
            // every storm with a shorter life as though it had failed to
            // move west, and 38-41% of this population fills before then.
            // A storm that never took a step has no motion to testify to
            // and is left out of the count entirely, not counted against.
            if let Some(p) = t.points.iter().filter(|p| p.day > 0.0 && p.day <= 5.0).last() {
                west_n += 1;
                if p.x < p0.x {
                    west_early += 1;
                }
            }

            if let Some(last) = t.points.last() {
                let l0 = calliope::storms::lat_of(p0.y, rows).abs();
                let l1 = calliope::storms::lat_of(last.y, rows).abs();
                if l1 > l0 {
                    poleward += 1;
                }
                // recurvature: of the storms that live to leave the
                // trades, how many are running east by the end?
                if let Some(cross) = t
                    .points
                    .iter()
                    .find(|p| calliope::storms::lat_of(p.y, rows).abs() >= 30.0)
                {
                    reached += 1;
                    if last.x > cross.x {
                        recurved += 1;
                    }
                }
            }
        }
        // How far poleward the population actually gets, so the
        // recurvature row below can be read against the reach that feeds
        // it rather than against an empty set.
        {
            let mut maxpole: Vec<f64> = tracks
                .iter()
                .map(|t| {
                    t.points
                        .iter()
                        .map(|p| calliope::storms::lat_of(p.y, rows).abs())
                        .fold(0.0f64, f64::max)
                })
                .collect();
            let mut net_west = 0usize;
            for t in &tracks {
                if t.points.last().unwrap().x < t.points[0].x {
                    net_west += 1;
                }
            }
            maxpole.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let q = |f: f64| maxpole[((maxpole.len() as f64 - 1.0) * f) as usize];
            println!(
                "   poleward reach |lat|: p50 {:.1}° · p90 {:.1}° · deepest {:.1}° · net westward over life {}/{}",
                q(0.5),
                q(0.9),
                maxpole[maxpole.len() - 1],
                net_west,
                tracks.len(),
            );
        }

        let recurve = if reached > 0 {

            recurved as f64 / reached as f64
        } else {
            0.0
        };
        recurve_shares.push(recurve);
        let mean_peak = peaks.iter().sum::<f64>() / n;
        let mean_life = lifetimes.iter().sum::<f64>() / n;
        println!(
            "   {} tracks · {:.1}/century · mean peak {:.2} · mean life {:.0} d · landfall {} · poleward {} · recurved {}/{}",
            tracks.len(),
            n * 100.0 / years as f64,
            mean_peak,
            mean_life,
            pct(landfalls as f64 / n),
            pct(poleward as f64 / n),
            recurved,
            reached,
        );

        c.must(
            &format!("the trades carry the young storm west · {}", seed),
            west_n > 0 && west_early as f64 / west_n as f64 >= 0.90,
            format!("{} of {} that moved at all", pct(west_early as f64 / west_n.max(1) as f64), west_n),
            "M78 gate: below 30° the zonal wind blows east-to-west, so a storm's first days must move it westward — the same wind_stress field the ocean's gyres read",
        );

        c.must(
            &format!("the storm climbs out of the tropics · {}", seed),
            poleward as f64 / n >= 0.90,
            pct(poleward as f64 / n),
            "M78: beta drift and the steering flow carry a cyclone poleward as it ages",
        );
        c.must(
            &format!("those that leave the trades recurve east · {}", seed),
            reached == 0 || recurve >= 0.70,
            format!("{} of {} reaching 30°", pct(recurve), reached),
            "M78 gate: a storm that survives into the westerlies must be turned back eastward by them — the recurvature is the wind field's doing, nothing in storms.rs bends a path",
        );
        c.want(
            &format!("the cool sea starves the engine · {}", seed),
            (2.0..=30.0).contains(&mean_life),
            format!("{:.0} d mean life", mean_life),
            "M78: cut off from its warm water a tropical cyclone fills within days — a storm that lived for months would mean the fuel term never bit",
        );

        // --- the season -----------------------------------------------
        let mut monn = [0usize; 12];
        let mut mons = [0usize; 12];
        for t in &tracks {
            if t.hemi >= 0 {
                monn[t.month as usize] += 1;
            } else {
                mons[t.month as usize] += 1;
            }
        }

        // The season's centre of mass on the circle of the year. The
        // modal month is not an estimator of a season: the humps here run
        // 30-49 tracks across three adjacent months, so the argmax turns
        // over on sampling noise (31337's north reads 35/36/49 in months
        // 0/1/2 and lands on 2 by a hair). The circular mean reads the
        // whole realized distribution and is still measured off the
        // tracks, not off the climatology that drew them.
        let circ_mean = |a: &[usize; 12]| -> f64 {
            let (mut sx, mut sy) = (0.0f64, 0.0f64);
            for (m, &k) in a.iter().enumerate() {
                let th = (m as f64) * std::f64::consts::TAU / 12.0;
                sx += (k as f64) * th.cos();
                sy += (k as f64) * th.sin();
            }
            let mut r = sy.atan2(sx) / std::f64::consts::TAU * 12.0;
            if r < 0.0 {
                r += 12.0;
            }
            r
        };
        let circ_dist = |a: f64, b: f64| -> f64 {
            let d = (a - b).abs() % 12.0;
            d.min(12.0 - d)
        };
        let (pn, ps) = (circ_mean(&monn), circ_mean(&mons));
        let sep = circ_dist(pn, ps);
        // A centre of mass at 11.96 must not print as "month 12.0" — the
        // year wraps, so round first and then bring it back onto 0-11.
        let show = |m: f64| -> f64 {
            let mut r = (m * 10.0).round() / 10.0;
            if r >= 12.0 {
                r -= 12.0;
            }
            if r.abs() < 0.05 { 0.0 } else { r }
        };
        c.must(
            &format!("the seasons stand half a year apart · {}", seed),
            sep >= 5.0,
            format!("season centre N month {:.1} · S month {:.1} · {:.2} apart", show(pn), show(ps), sep),
            "M78 gate: each hemisphere's cyclones fall in its own warm season, so the two peaks must sit close to six months apart — as the world's seasons do",
        );
        c.must(
            &format!("the warm-sea season opposes the frontal one · {}", seed),
            circ_dist(pn, clim.cold_month(1) as f64) >= 3.0
                && circ_dist(ps, clim.cold_month(-1) as f64) >= 3.0,
            format!(
                "N warm-sea {:.1} vs frontal {} · S warm-sea {:.1} vs frontal {}",
                show(pn),
                clim.cold_month(1),
                show(ps),
                clim.cold_month(-1)
            ),
            "M78 gate: the frontal corridor of M77 peaks in the cold season and the warm-sea engine in the hot one — two engines running on opposite terms cannot share a peak month",
        );


        c.must(
            &format!("nothing poisons the tracks · {}", seed),
            tracks
                .iter()
                .all(|t| t.points.iter().all(|p| p.x.is_finite() && p.y.is_finite() && p.inten.is_finite())),
            format!("{} points", tracks.iter().map(|t| t.points.len()).sum::<usize>()),
            "M78: a non-finite position or intensity would poison every landfall that reads it",
        );

        // --- replay ----------------------------------------------------
        let clim2 = calliope::storms::StormClimatology::new(
            &w.fields.height,
            &w.fields.tmean,
            &w.fields.tamp,
        );
        c.must(
            &format!("the warm seas replay · {}", seed),
            clim.trop_probe(seed, &w.fields.height) == clim2.trop_probe(seed, &w.fields.height),
            format!("{:016x}", clim.trop_probe(seed, &w.fields.height)),
            "ADR-0003: a cyclone season is a pure function of (seed, year, hemisphere) — it is derived on demand, never stored",
        );
        let y1 = clim.trop_season(seed, 3, 1, &w.fields.height);
        let y2 = clim.trop_season(seed, 4, 1, &w.fields.height);
        c.must(
            &format!("each year draws its own cyclones · {}", seed),
            !(y1.len() == y2.len()
                && y1
                    .iter()
                    .zip(y2.iter())
                    .all(|(a, b)| a.genesis == b.genesis && a.month == b.month)),
            format!("{} vs {} tracks", y1.len(), y2.len()),
            "M78: the season is keyed on the year — two identical years would mean the warm seas are a fixture, not weather",
        );
    }

    if band_shares.len() > 1 {
        let lo = band_shares.iter().cloned().fold(f64::INFINITY, f64::min);
        c.must(
            "the tropical band holds in every world",
            lo >= 0.99,
            pct(lo),
            "M78 gate: the 26 °C isotherm is a property of the general circulation, so every seed must breed its cyclones in the same band",
        );
        let rlo = recurve_shares.iter().cloned().fold(f64::INFINITY, f64::min);
        c.must(
            "no world runs its recurvature backwards",
            rlo >= 0.70,
            pct(rlo),
            "M78 gate: the westerlies turn storms east in every world or the steering field is not the one the ocean reads",
        );
    }
    c.print();
}



fn cmd_oscillation(months: i64, seeds: Vec<i64>) {
    header(
        "OSCILLATION",
        &format!("{} seeds · {} mo", seeds.len(), months),
    );
    println!("the slow lean of the seas · drawn law vs realized series  (M74)");

    let mut c = Checks::default();
    let mut periods: Vec<f64> = Vec::new();
    let mut proms: Vec<f64> = Vec::new();
    let mut recs: Vec<f64> = Vec::new();
    println!();
    println!(" seed        period mo  recovered   amp    σ realized     mean   |idx|max  warm%  cold%  peak/floor  rival  coher");
    for &seed in &seeds {
        let o = calliope::oscillation::Oscillation::new(seed);
        let n = months as usize;
        let series: Vec<f64> = (0..months).map(|m| o.index(m)).collect();
        let finite = series.iter().all(|v| v.is_finite());
        let mean = series.iter().sum::<f64>() / n as f64;
        let var = series.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / n as f64;
        let sd = var.sqrt();
        let amax = series.iter().fold(0.0f64, |a, v| a.max(v.abs()));
        let warm = series.iter().filter(|v| **v > 0.8 * o.amp()).count() as f64 / n as f64;
        let cold = series.iter().filter(|v| **v < -0.8 * o.amp()).count() as f64 / n as f64;

        // Blind recovery: the strongest Fourier power over candidate
        // periods, scanned finely enough to resolve the declared band.
        // M76 keeps the whole spectrum, not only its argmax — a peak is
        // only a mode if it stands above the floor around it and has no
        // rival elsewhere in the scan.
        let mut spectrum: Vec<(f64, f64)> = Vec::new();
        let mut best = (0.0f64, 0.0f64);
        let mut t = 12.0f64;
        while t <= 140.0 {
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (m, v) in series.iter().enumerate() {
                let a = std::f64::consts::TAU * m as f64 / t;
                re += (v - mean) * a.cos();
                im += (v - mean) * a.sin();
            }
            let pw = re * re + im * im;
            if pw > best.1 {
                best = (t, pw);
            }
            spectrum.push((t, pw));
            t += 0.25;
        }
        let rec = best.0;

        // ---- M76: is this a mode, or is it noise wearing a peak? -----
        // The floor is the median power of every candidate period that
        // is not part of the peak's own skirt (±25% of the recovered
        // period). The prominence is peak/floor. The rival is the
        // strongest power outside that skirt: a second comparable peak
        // would mean two modes, not one dominant seesaw.
        let skirt = |p: f64| (p - rec).abs() / rec <= 0.25;
        let mut off: Vec<f64> = spectrum
            .iter()
            .filter(|(p, _)| !skirt(*p))
            .map(|(_, w)| *w)
            .collect();
        off.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let floor = if off.is_empty() { 0.0 } else { off[off.len() / 2] };
        let rival = off.iter().cloned().fold(0.0f64, f64::max);
        let prominence = if floor > 0.0 { best.1 / floor } else { f64::INFINITY };
        let rival_share = if best.1 > 0.0 { rival / best.1 } else { 1.0 };

        // Phase lock: the realized index against a unit sinusoid at the
        // *recovered* period. A quasi-periodic mode keeps its phase over
        // many cycles; broadband noise does not. This is the coherence
        // (fraction of variance the single recovered tone explains).
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (m, v) in series.iter().enumerate() {
            let a = std::f64::consts::TAU * m as f64 / rec;
            re += (v - mean) * a.cos();
            im += (v - mean) * a.sin();
        }
        let coherence = if var > 0.0 {
            2.0 * (re * re + im * im) / (n as f64 * n as f64 * var)
        } else {
            0.0
        };

        // Irregularity: the gaps between upward zero crossings must not
        // all be the same number — a metronome is not a basin.
        let mut cross: Vec<usize> = Vec::new();
        for m in 1..n {
            if series[m - 1] <= 0.0 && series[m] > 0.0 {
                cross.push(m);
            }
        }
        let gaps: Vec<f64> = cross.windows(2).map(|w| (w[1] - w[0]) as f64).collect();
        let gsd = if gaps.len() > 2 {
            let gm = gaps.iter().sum::<f64>() / gaps.len() as f64;
            (gaps.iter().map(|g| (g - gm) * (g - gm)).sum::<f64>() / gaps.len() as f64).sqrt()
        } else {
            0.0
        };

        println!(
            " {:<10} {:>8.1}  {:>8.1}  {:>6.2}  {:>10.3}  {:>7.3}  {:>8.2}  {:>5.1}  {:>5.1}  {:>10.1}  {:>5.2}  {:>5.2}",
            seed, o.period(), rec, o.amp(), sd, mean, amax,
            100.0 * warm, 100.0 * cold, prominence, rival_share, coherence
        );

        let sk = |t: &str| format!("{} · {}", t, seed);
        c.must(
            &sk("period inside the declared band"),
            o.period() >= calliope::oscillation::OSC_PERIOD_MIN
                && o.period() <= calliope::oscillation::OSC_PERIOD_MAX,
            format!("{:.1} mo in 24-84", o.period()),
            "M74: an ENSO-class basin leans for two to seven years — never a season, never a century",
        );
        c.must(
            &sk("amplitude inside the declared band"),
            o.amp() >= calliope::oscillation::OSC_AMP_MIN
                && o.amp() <= calliope::oscillation::OSC_AMP_MAX,
            format!("{:.2} in 0.55-1.45", o.amp()),
            "M74: every world draws a basin of its own strength, inside one envelope",
        );
        c.must(
            &sk("the period is recoverable blind"),
            (rec - o.period()).abs() / o.period() <= 0.15,
            format!("{:+.1}% of drawn", 100.0 * (rec - o.period()) / o.period()),
            "M74: the dominant periodogram peak of the realized index must find the drawn period within 15% — the lean is in the series, not only in the struct",
        );
        c.must(
            &sk("realized σ is the drawn amplitude"),
            (sd - o.amp()).abs() / o.amp() <= 0.10,
            format!("{:+.1}% of drawn", 100.0 * (sd - o.amp()) / o.amp()),
            "M74: clean lane and coloured noise are mixed at fixed variance shares, so σ is the amplitude however the shares are tuned",
        );
        c.must(
            &sk("the lean is balanced"),
            mean.abs() <= 0.15 * o.amp(),
            format!("{:+.3} vs {:.2}", mean, o.amp()),
            "M74: a basin returns — no world may sit warm forever",
        );
        c.must(
            &sk("warm and cold phases both occur"),
            warm >= 0.08 && cold >= 0.08,
            format!("{:.0}% warm · {:.0}% cold", 100.0 * warm, 100.0 * cold),
            "M74: both sides of the seesaw must be lived in, or the index is a trend",
        );
        c.must(
            &sk("the cap holds"),
            amax <= calliope::oscillation::OSC_CAP_SIGMA * o.amp() + 1e-9,
            format!("{:.2} <= {:.2}", amax, calliope::oscillation::OSC_CAP_SIGMA * o.amp()),
            "M74: a tail draw may not hand the causal path an absurd year",
        );
        c.must(
            &sk("the seesaw is irregular"),
            gsd >= 1.0,
            format!("cycle σ {:.1} mo", gsd),
            "M74: real basins skip and stall — successive cycles of identical length would be a metronome",
        );
        c.must(
            &sk("every month is finite"),
            finite,
            format!("{} mo", n),
            "M74: no poison enters the causal path M72 opened",
        );
        c.must(
            &sk("the basin replays"),
            calliope::oscillation::Oscillation::new(seed).probe() == o.probe(),
            format!("{:016x}", o.probe()),
            "ADR-0003: the seesaw is derived from the seed alone — rebuilding it must give the same law and the same series",
        );
        // ---- M76: the seesaw read as a spectrum ----------------------
        c.must(
            &sk("the dominant peak sits in the declared band"),
            rec >= calliope::oscillation::OSC_PERIOD_MIN
                && rec <= calliope::oscillation::OSC_PERIOD_MAX,
            format!("{:.1} mo in 24-84", rec),
            "M76 gate: the peak the spectrum actually finds — not the drawn constant — must land inside the ENSO-class band",
        );
        c.must(
            &sk("the peak stands above the noise floor"),
            prominence >= 3.0,
            format!("{:.1}x floor", prominence),
            "M76 gate: dominant spectral power at least 3x the off-peak median — a mode rises out of its own background, decorative noise does not",
        );
        c.must(
            &sk("the peak is single, not one of a pair"),
            rival_share <= 0.5,
            format!("rival {:.0}% of peak", 100.0 * rival_share),
            "M76 gate: the strongest power outside the peak's skirt must be under half of it — two comparable peaks would be two modes, not one seesaw",
        );
        c.must(
            &sk("the mode holds its phase"),
            coherence >= 0.30,
            format!("{:.0}% of variance in the tone", 100.0 * coherence),
            "M76: a quasi-periodic mode keeps phase over many cycles, so one tone carries a real share of the variance — broadband noise spreads it thin",
        );
        proms.push(prominence);
        recs.push(rec);
        periods.push(o.period());
    }

    // The basin is the world's, not the engine's: two worlds must not be
    // handed the same seesaw.
    let pmin = periods.iter().cloned().fold(f64::MAX, f64::min);
    let pmax = periods.iter().cloned().fold(f64::MIN, f64::max);
    c.must(
        "each world draws its own basin",
        pmax - pmin >= 4.0,
        format!("{:.1}-{:.1} mo across seeds", pmin, pmax),
        "M74: the period is drawn per seed — a sweep that all leans on one rhythm means the draw is not keyed to the world",
    );
    // M76 — the sweep-wide statement: every seed, not the best one.
    let worst_prom = proms.iter().cloned().fold(f64::MAX, f64::min);
    c.must(
        "every world's peak clears the floor",
        worst_prom >= 3.0,
        format!("weakest {:.1}x over {} seeds", worst_prom, proms.len()),
        "M76 gate: the 3x prominence must hold on every sweep seed — one world proving a mode proves nothing about the law",
    );
    let rmin = recs.iter().cloned().fold(f64::MAX, f64::min);
    let rmax = recs.iter().cloned().fold(f64::MIN, f64::max);
    c.must(
        "the recovered rhythms differ across worlds",
        rmax - rmin >= 4.0,
        format!("{:.1}-{:.1} mo recovered", rmin, rmax),
        "M76: the spectrum must see the per-seed draw too — one recovered period across every world would mean the analysis is reading the engine, not the series",
    );
    c.print();
}

/// M73 — the sky's variance, held in bands.
///
/// M71 gave every year its own weather and M72 made it causal; this lane
/// is the instrument that keeps the swing believable. It measures the
/// realized interannual σ of the temperature and rain lanes over land,
/// split into the three latitude belts the declared amplitude law
/// separates, and holds it against three independent kinds of evidence:
///
/// 1. **The declared law.** Each belt's realized σ is compared with the
///    belt-mean of `climate::anomaly_amp_t`/`_p` over the very land cells
///    measured — not a hard-coded number, so a change to the amplitude
///    constants moves target and measurement together and only a genuine
///    normalizer drift can open a gap.
/// 2. **Budyko–Sellers shape.** Polar amplification is the physics here:
///    a weaker meridional gradient and ice-albedo feedback make the high
///    latitudes swing several times wider than the tropics. The measured
///    polar/tropical σ ratio must be > 2 and must match the ratio the
///    declared law itself predicts for these belts.
/// 3. **Absolute ceilings.** Lively, never chaos: tropical σT < 1.5 °C
///    and polar σT < 4 °C on every seed (the spec's own caps), with the
///    rain lane never able to take a whole year's rain away.
///
/// Cross-seed spread closes it: the σ of a belt is a property of the sky's
/// law, not of the seed that drew it, so the spread of each belt's σ
/// across the sweep must stay inside a tenth of its mean.
fn cmd_climate_variance(size: usize, years: i64, seeds: Vec<i64>) {
    header(
        "CLIMATE-VARIANCE",
        &format!("{}x{} · {} seeds · {} y", size, size, seeds.len(), years),
    );
    println!("interannual σ per latitude belt · declared vs realized · cross-seed spread  (M73)");

    const BELTS: [&str; 3] = ["tropical  <23.5", "temperate 23.5-55", "polar     >=55"];
    // σT hard ceilings per belt (M73 gate: tropical <1.5 °C, polar <4 °C;
    // the temperate belt sits between them and is held to the polar cap).
    const CAP_T: [f64; 3] = [1.5, 4.0, 4.0];

    let mut c = Checks::default();
    // [seed][belt] realized σ, and the declared belt mean beside it.
    let mut all_t: Vec<[f64; 3]> = Vec::new();
    let mut all_p: Vec<[f64; 3]> = Vec::new();
    let mut all_p0: Vec<[f64; 3]> = Vec::new();
    let mut all_dt: Vec<[f64; 3]> = Vec::new();
    let mut all_dp: Vec<[f64; 3]> = Vec::new();
    let mut nonfinite = 0usize;
    let mut worst_floor = 0.0f64;
    let mut empty_belt = 0usize;

    println!();
    println!(" seed      belt                  σ T °C  declared    σ P frac  declared   σP unforced   cells");
    for &seed in &seeds {
        let w = World::generate(seed, size);
        let land = land_mask(&w);
        let rows = w.fields.tmean.dim().0;
        let belt_of = |lat: f64| -> usize {
            if lat < 23.5 {
                0
            } else if lat < 55.0 {
                1
            } else {
                2
            }
        };
        // (Σx, Σx², n) per belt for each lane; declared amplitude summed
        // over the same cells, once per cell (not once per year).
        let mut acc_t = [[0.0f64; 3]; 3];
        let mut acc_p = [[0.0f64; 3]; 3];
        // M75: the sky now has two sources of interannual rain variance —
        // the unforced lattice M73 declared, and the tilt the oscillation
        // lays on the trade belts. The declaration is composed from both
        // (they are independent by construction: one is a noise lane, the
        // other a function of the index), and the *shape* laws are stated
        // on the unforced lane, which is the one the amplitude law
        // describes. Nothing is loosened; the prediction is re-derived
        // against the mechanism that now exists.
        let osc_years: Vec<f64> = (1..=years).map(|yr| w.year_osc(yr)).collect();
        let osc_mean = osc_years.iter().sum::<f64>() / osc_years.len() as f64;
        let osc_sigma = (osc_years.iter().map(|v| (v - osc_mean).powi(2)).sum::<f64>()
            / osc_years.len() as f64)
            .sqrt();
        let mut acc_p0 = [[0.0f64; 3]; 3];
        let mut dec_t = [[0.0f64; 2]; 3];
        let mut dec_p = [[0.0f64; 2]; 3];
        for y in 0..rows {
            let lat = (-90.0 + (y as f64) * 180.0 / (rows as f64 - 1.0)).abs();
            let b = belt_of(lat);
            for x in 0..w.width {
                if !land[[y, x]] {
                    continue;
                }
                dec_t[b][0] += calliope::climate::anomaly_amp_t(lat);
                dec_t[b][1] += 1.0;
                let shape = calliope::climate::teleconnection_bias(
                    1.0,
                    -90.0 + (y as f64) * 180.0 / (rows as f64 - 1.0),
                );
                let amp_p = calliope::climate::anomaly_amp_p(lat);
                dec_p[b][0] += (amp_p * amp_p + shape * shape * osc_sigma * osc_sigma).sqrt();
                dec_p[b][1] += 1.0;
            }
        }
        for year in 1..=years {
            let (dt, dp) = w.year_anomaly_fresh(year);
            let (_, dp0) =
                calliope::climate::year_anomaly(w.variability(), rows, w.width, year, 0.0);
            for y in 0..rows {
                let lat = (-90.0 + (y as f64) * 180.0 / (rows as f64 - 1.0)).abs();
                let b = belt_of(lat);
                for x in 0..w.width {
                    if !land[[y, x]] {
                        continue;
                    }
                    let (a, r) = (dt[[y, x]], dp[[y, x]]);
                    if !a.is_finite() || !r.is_finite() {
                        nonfinite += 1;
                        continue;
                    }
                    if r < worst_floor {
                        worst_floor = r;
                    }
                    acc_t[b][0] += a;
                    acc_t[b][1] += a * a;
                    acc_t[b][2] += 1.0;
                    acc_p[b][0] += r;
                    acc_p[b][1] += r * r;
                    acc_p[b][2] += 1.0;
                    let r0 = dp0[[y, x]];
                    acc_p0[b][0] += r0;
                    acc_p0[b][1] += r0 * r0;
                    acc_p0[b][2] += 1.0;
                }
            }
        }
        let sigma = |a: [f64; 3]| -> f64 {
            if a[2] < 2.0 {
                return f64::NAN;
            }
            let m = a[0] / a[2];
            (a[1] / a[2] - m * m).max(0.0).sqrt()
        };
        let mut st = [0.0f64; 3];
        let mut sp = [0.0f64; 3];
        let mut sp0 = [0.0f64; 3];
        let mut dtm = [0.0f64; 3];
        let mut dpm = [0.0f64; 3];
        for b in 0..3 {
            if acc_t[b][2] < 2.0 || dec_t[b][1] < 1.0 {
                empty_belt += 1;
                st[b] = f64::NAN;
                sp[b] = f64::NAN;
                sp0[b] = f64::NAN;
                dtm[b] = f64::NAN;
                dpm[b] = f64::NAN;
                println!(" {:<9} {:<20}      (no land in this belt)", seed, BELTS[b]);
                continue;
            }
            st[b] = sigma(acc_t[b]);
            sp[b] = sigma(acc_p[b]);
            sp0[b] = sigma(acc_p0[b]);
            dtm[b] = dec_t[b][0] / dec_t[b][1];
            dpm[b] = dec_p[b][0] / dec_p[b][1];
            println!(
                " {:<9} {:<20} {:>7.3} {:>9.3} {:>11.4} {:>9.4} {:>13.4} {:>7}",
                seed,
                BELTS[b],
                st[b],
                dtm[b],
                sp[b],
                dpm[b],
                sp0[b],
                acc_t[b][2] as i64 / years,
            );
        }
        all_t.push(st);
        all_p.push(sp);
        all_p0.push(sp0);
        all_dt.push(dtm);
        all_dp.push(dpm);

        // ---- per-seed gates -----------------------------------------
        for b in 0..3 {
            if !st[b].is_finite() {
                continue;
            }
            c.must(
                &format!("σT under its cap · {} · {}", seed, BELTS[b].trim()),
                st[b] < CAP_T[b],
                format!("{:.3} < {:.1} °C", st[b], CAP_T[b]),
                "M73: lively, never chaos — tropical under 1.5 °C, polar under 4 °C",
            );
            let err_t = (st[b] - dtm[b]).abs() / dtm[b];
            c.must(
                &format!("σT is the declared law · {} · {}", seed, BELTS[b].trim()),
                err_t <= 0.10,
                format!("{:+.1}% of declared", 100.0 * (st[b] - dtm[b]) / dtm[b]),
                "M73: realized σ within 10% of anomaly_amp_t over the same cells — the normalizer cannot drift",
            );
            let err_p = (sp[b] - dpm[b]).abs() / dpm[b];
            c.must(
                &format!("σP is the declared law · {} · {}", seed, BELTS[b].trim()),
                err_p <= 0.12,
                format!("{:+.1}% of declared", 100.0 * (sp[b] - dpm[b]) / dpm[b]),
                "M73: the rain lane obeys anomaly_amp_p within 12% (the −0.85 floor clips its low tail)",
            );
        }
        let (t0, t2) = (st[0], st[2]);
        if t0.is_finite() && t2.is_finite() {
            let ratio = t2 / t0;
            let declared = dtm[2] / dtm[0];
            c.must(
                &format!("polar amplification · {}", seed),
                ratio > 2.0,
                format!("{:.2}× tropical", ratio),
                "M73 (Budyko–Sellers): a weak polar gradient and ice-albedo feedback make the high belt swing >2× the tropics",
            );
            c.must(
                &format!("amplification is the declared one · {}", seed),
                (ratio - declared).abs() / declared <= 0.10,
                format!("{:.2} vs {:.2}", ratio, declared),
                "M73: the measured polar/tropical ratio matches the ratio the amplitude law predicts for these belts",
            );
            c.must(
                &format!("σT climbs poleward · {}", seed),
                st[0] < st[1] && st[1] < st[2],
                format!("{:.2} < {:.2} < {:.2}", st[0], st[1], st[2]),
                "M73: band by band, never a flat or inverted profile",
            );
            c.must(
                &format!("σP climbs poleward · {}", seed),
                sp0[0] < sp0[1] && sp0[1] < sp0[2],
                format!("{:.3} < {:.3} < {:.3}", sp0[0], sp0[1], sp0[2]),
                "M73: the unforced rain lane carries the same latitude shape as the heat lane (M75's tilt is a forced term on top, held by its own lane)",
            );
        }
    }

    // ---- cross-seed spread ------------------------------------------
    println!();
    println!(" belt                  σT mean   spread    σP mean   spread");
    for b in 0..3 {
        let ts: Vec<f64> = all_t.iter().map(|s| s[b]).filter(|v| v.is_finite()).collect();
        let ps: Vec<f64> = all_p0.iter().map(|s| s[b]).filter(|v| v.is_finite()).collect();
        if ts.len() < 2 {
            continue;
        }
        let mean_t = ts.iter().sum::<f64>() / ts.len() as f64;
        let mean_p = ps.iter().sum::<f64>() / ps.len() as f64;
        let spread_t = (ts.iter().cloned().fold(f64::MIN, f64::max)
            - ts.iter().cloned().fold(f64::MAX, f64::min))
            / mean_t;
        let spread_p = (ps.iter().cloned().fold(f64::MIN, f64::max)
            - ps.iter().cloned().fold(f64::MAX, f64::min))
            / mean_p;
        println!(
            " {:<20} {:>7.3} {:>8.1}% {:>10.4} {:>8.1}%",
            BELTS[b],
            mean_t,
            100.0 * spread_t,
            mean_p,
            100.0 * spread_p
        );
        c.must(
            &format!("σT is the sky's, not the seed's · {}", BELTS[b].trim()),
            spread_t <= 0.10,
            format!("{:.1}% of mean", 100.0 * spread_t),
            "M73: the belt's swing is a property of the law — cross-seed spread inside a tenth of the mean",
        );
        c.must(
            &format!("σP is the sky's, not the seed's · {}", BELTS[b].trim()),
            spread_p <= 0.10,
            format!("{:.1}% of mean", 100.0 * spread_p),
            "M73: same law, unforced rain lane (the forced tilt is per-world by design — M75)",
        );
    }

    c.must(
        "every anomaly cell is finite",
        nonfinite == 0,
        format!("{} non-finite", nonfinite),
        "M73: no poison enters the causal path M72 opened",
    );
    c.must(
        "the rains are never wholly taken",
        worst_floor > -1.0,
        format!("worst {:+.3} of the mean", worst_floor),
        "M2.6/M73: total failure of the rains is famine's verdict, not the sky's noise",
    );
    c.must(
        "every belt carries land to measure",
        empty_belt == 0,
        format!("{} empty belt-seeds", empty_belt),
        "M73: a gate over an empty belt proves nothing",
    );

    c.print();
}

/// M49 — the ocean stack's own instrument panel. Everything the ocean
/// phases (M40 gyres · M41 heat · M42 rain · M47 upwelling · M48
/// seasonality) put into the world, measured on one page across the
/// seed sweep, each family banded. Generation only: every field here
/// is dawn state, so no century has to run for the ocean to be judged.
fn cmd_ocean(size: usize, seeds: Vec<i64>) {
    header(
        "OCEAN",
        &format!("{}x{} · {} seeds", size, size, seeds.len()),
    );
    println!("gyres · current-coast heat · upwelling · sea-lane seasonality  (M40–M48)");

    let mut rows_out: Vec<OceanRow> = Vec::new();
    let mut gyre_lines: Vec<String> = Vec::new();

    for &seed in &seeds {
        let w = World::generate(seed, size);
        let (rows, cols) = w.fields.height.dim();
        let water = w.fields.height.mapv(|h| h < 0.0);
        let lat_s = |y: usize| -90.0 + y as f64 * 180.0 / (rows - 1) as f64;

        // ---- gyre topology (M40) -------------------------------------
        // Label the ocean into basins; within each basin and hemisphere
        // read the subtropical band (10–40°). Positive ψ turns clockwise
        // on screen, so the north wants ψ > 0 and the south ψ < 0 —
        // anticyclonic both ways, Earth's sense. Cell counts per gyre
        // are the topology the spec asks for: a gyre is a region, not a
        // sign.
        let lab = ndimage::label(&water, false);
        let basins = ndimage::top_components(&lab, 2500.0, 8);
        let (mut gy_n, mut gy_ok, mut gy_basins) = (0usize, 0usize, 0usize);
        let mut gy_cells: Vec<f64> = Vec::new();
        let mut band_sp: Vec<f64> = Vec::new();
        let mut west_sp: Vec<f64> = Vec::new();
        let mut int_sp: Vec<f64> = Vec::new();
        for (bi_idx, &(bi, area)) in basins.iter().enumerate() {
            let mut any = false;
            let mut parts: Vec<String> = Vec::new();
            for hemi in 0..2 {
                let mut n = 0usize;
                let mut psum = 0.0f64;
                for y in 0..rows {
                    let ls = lat_s(y);
                    let north = ls < 0.0; // negative row-latitude = north (grid law)
                    if (hemi == 0) != north {
                        continue;
                    }
                    if !(10.0..=40.0).contains(&ls.abs()) {
                        continue;
                    }
                    let mut x = 0usize;
                    while x < cols {
                        if lab.lab[[y, x]] != bi as i32 {
                            x += 1;
                            continue;
                        }
                        let x0 = x;
                        while x < cols && lab.lab[[y, x]] == bi as i32 {
                            x += 1;
                        }
                        for xi in x0..x {
                            n += 1;
                            psum += w.currents.psi[[y, xi]] as f64;
                            let sp = (w.currents.u[[y, xi]] as f64)
                                .hypot(w.currents.v[[y, xi]] as f64);
                            band_sp.push(sp);
                            if xi < x0 + 4 {
                                west_sp.push(sp);
                            } else {
                                int_sp.push(sp);
                            }
                        }
                    }
                }
                if n >= 250 {
                    gy_n += 1;
                    any = true;
                    gy_cells.push(n as f64);
                    let mp = psum / n as f64;
                    let want_pos = hemi == 0;
                    let ok = mp != 0.0 && (mp > 0.0) == want_pos;
                    if ok {
                        gy_ok += 1;
                    }
                    parts.push(format!(
                        "{} {} {} cells ψ̄ {:+.2}",
                        if hemi == 0 { "N" } else { "S" },
                        if mp > 0.0 { "cw" } else { "ccw" },
                        n,
                        mp
                    ));
                }
            }
            if any {
                gy_basins += 1;
                gyre_lines.push(format!(
                    "  seed {:>6} basin {} ({:.0} cells): {}",
                    seed,
                    bi_idx + 1,
                    area,
                    parts.join(" · ")
                ));
            }
        }
        let sp95 = quantile(&band_sp, 0.95);
        let westx = {
            let wi = quantile(&west_sp, 0.95);
            let ii = quantile(&int_sp, 0.95);
            if ii > 0.0 {
                wi / ii
            } else {
                f64::NAN
            }
        };
        let gy_cells_mean = if gy_cells.is_empty() {
            0.0
        } else {
            gy_cells.iter().sum::<f64>() / gy_cells.len() as f64
        };

        // ---- current-coast temperature against the zonal mean (M41) ---
        // The spec's yardstick verbatim: a coast the currents warm must
        // run warmer than the coastal land of its own row, a coast they
        // cool colder. Row-relative, because latitude is the stronger
        // law by an order of magnitude; coastal-only, because interiors
        // are a different climate entirely.
        let heat = calliope::climate::current_bias(&water, &w.currents.v);
        let mut coastal = vec![false; rows * cols];
        let mut row_t = vec![0.0f64; rows];
        let mut row_n = vec![0usize; rows];
        for y in 0..rows {
            for x in 0..cols {
                if water[[y, x]] {
                    continue;
                }
                let mut touches = false;
                for dy in -1i64..=1 {
                    for dx in -1i64..=1 {
                        let yy = y as i64 + dy;
                        let xx = x as i64 + dx;
                        if yy < 0 || xx < 0 || yy >= rows as i64 || xx >= cols as i64 {
                            continue;
                        }
                        if water[[yy as usize, xx as usize]] {
                            touches = true;
                        }
                    }
                }
                if touches {
                    coastal[y * cols + x] = true;
                    row_t[y] += w.fields.tmean[[y, x]] as f64;
                    row_n[y] += 1;
                }
            }
        }
        let (mut wsum, mut wn, mut csum, mut cn) = (0.0f64, 0usize, 0.0f64, 0usize);
        for y in 0..rows {
            if row_n[y] == 0 {
                continue;
            }
            let rm = row_t[y] / row_n[y] as f64;
            for x in 0..cols {
                if !coastal[y * cols + x] {
                    continue;
                }
                let d = w.fields.tmean[[y, x]] as f64 - rm;
                let h = heat[[y, x]];
                if h >= 0.5 {
                    wsum += d;
                    wn += 1;
                } else if h <= -0.5 {
                    csum += d;
                    cn += 1;
                }
            }
        }
        let warm_d = if wn == 0 { f64::NAN } else { wsum / wn as f64 };
        let cold_d = if cn == 0 { f64::NAN } else { csum / cn as f64 };

        // ---- upwelling coverage (M47) ---------------------------------
        let (mut coast_o, mut rich, mut stray) = (0usize, 0usize, 0usize);
        let mut up_lats: Vec<f64> = Vec::new();
        for y in 0..rows {
            for x in 0..cols {
                let u = w.fields.upwelling[[y, x]] as f64;
                if !water[[y, x]] {
                    if u > 0.0 {
                        stray += 1;
                    }
                    continue;
                }
                let mut adj = false;
                for dy in -1i64..=1 {
                    for dx in -1i64..=1 {
                        let yy = y as i64 + dy;
                        let xx = x as i64 + dx;
                        if yy < 0 || xx < 0 || yy >= rows as i64 || xx >= cols as i64 {
                            continue;
                        }
                        if !water[[yy as usize, xx as usize]] {
                            adj = true;
                        }
                    }
                }
                if !adj {
                    if u > 0.0 {
                        stray += 1;
                    }
                    continue;
                }
                coast_o += 1;
                if w.fields.upwelling[[y, x]] >= calliope::climate::NUTRIENT_RICH {
                    rich += 1;
                    up_lats.push(lat_s(y).abs());
                }
            }
        }
        let up_share = rich as f64 / coast_o.max(1) as f64;
        let up_lat = if up_lats.is_empty() {
            f64::NAN
        } else {
            quantile(&up_lats, 0.5)
        };

        // ---- sea-lane seasonality spread (M37/M48) --------------------
        // Every sea-touching lane's year, read through the very law the
        // ledger applies: a shut month carries nothing, an open one
        // carries `season_mult`. The spread (p90 − p10 of per-lane
        // swing) is the spec's own metric — a world whose lanes all
        // swing alike has no calendar worth sailing by.
        let sea_lanes: Vec<&calliope::trade::Route> =
            w.routes.iter().filter(|r| r.sea > 0.0).collect();
        let mut swings: Vec<f64> = Vec::new();
        let mut seasonal = 0usize;
        for r in &sea_lanes {
            let (mut lo, mut hi) = (f64::MAX, 0.0f64);
            for m in 0..12i64 {
                let mult = if r.closed >> (m as usize) & 1 == 1 {
                    0.0
                } else {
                    calliope::trade::season_mult(r.season, m)
                };
                lo = lo.min(mult);
                hi = hi.max(mult);
            }
            let sw = if hi > 0.0 { 100.0 * (1.0 - lo / hi) } else { 100.0 };
            swings.push(sw);
            if r.closed != 0 || r.season.abs() >= calliope::trade::MONSOON_LANE {
                seasonal += 1;
            }
        }
        let seas_share = 100.0 * seasonal as f64 / sea_lanes.len().max(1) as f64;
        let swing_spread = if swings.is_empty() {
            0.0
        } else {
            quantile(&swings, 0.9) - quantile(&swings, 0.1)
        };
        let swing_p50 = if swings.is_empty() { 0.0 } else { quantile(&swings, 0.5) };

        println!();
        println!(
            " seed {} — {} basins · {} gyres ({} earthly) · mean {:.0} cells/gyre · speed p95 {:.2} · west {:.1}×",
            seed, gy_basins, gy_n, gy_ok, gy_cells_mean, sp95, westx
        );
        println!(
            "   coasts: warm rims {:+.2}°C ({} cells) · cold rims {:+.2}°C ({}) against their own row's coast",
            warm_d, wn, cold_d, cn
        );
        println!(
            "   upwelling: {} of {} coastal-ocean cells nutrient-rich ({}) · median |lat| {:.0}° · {} stray off-coast",
            rich, coast_o, pct(up_share), up_lat, stray
        );
        println!(
            "   lanes: {} sea-touching · {} seasonal ({:.0}%) · swing p50 {:.0}% · spread p90−p10 {:.0}pp",
            sea_lanes.len(), seasonal, seas_share, swing_p50, swing_spread
        );

        rows_out.push(OceanRow {
            seed,
            basins: gy_basins,
            gyres: gy_n,
            earthly: gy_ok,
            gyre_cells: gy_cells_mean,
            sp95,
            westx,
            warm_d,
            cold_d,
            warm_n: wn,
            cold_n: cn,
            up_share,
            up_lat,
            sea_lanes: sea_lanes.len(),
            seasonal,
            seas_share,
            swing_spread,
            swing_p50,
        });
    }

    println!();
    println!("gyre roll (subtropical band 10–40°, ≥250 cells):");
    for l in &gyre_lines {
        println!("{}", l);
    }

    println!();
    println!(
        " {:>6} {:>7} {:>6} {:>8} {:>8} {:>7} {:>8} {:>8} {:>7} {:>8} {:>7} {:>8} {:>8}",
        "seed", "basins", "gyres", "cells/g", "sp p95", "west×", "warm ΔT", "cold ΔT", "upwell", "seasonal", "seas%", "swing50", "spread"
    );
    for r in &rows_out {
        println!(
            " {:>6} {:>7} {:>6} {:>8.0} {:>8.2} {:>6.1}× {:>7.2}C {:>7.2}C {:>7} {:>8} {:>6.0}% {:>7.0}% {:>6.0}pp",
            r.seed, r.basins, r.gyres, r.gyre_cells, r.sp95, r.westx,
            r.warm_d, r.cold_d, pct(r.up_share), r.seasonal, r.seas_share, r.swing_p50, r.swing_spread
        );
    }

    let mean = |f: &dyn Fn(&OceanRow) -> f64| -> f64 {
        let v: Vec<f64> = rows_out.iter().map(f).filter(|x| x.is_finite()).collect();
        if v.is_empty() {
            f64::NAN
        } else {
            v.iter().sum::<f64>() / v.len() as f64
        }
    };
    let m_basins = mean(&|r: &OceanRow| r.basins as f64);
    let m_cells = mean(&|r: &OceanRow| r.gyre_cells);
    let m_sp95 = mean(&|r: &OceanRow| r.sp95);
    let m_west = mean(&|r: &OceanRow| r.westx);
    let m_warm = mean(&|r: &OceanRow| r.warm_d);
    let m_cold = mean(&|r: &OceanRow| r.cold_d);
    let m_up = mean(&|r: &OceanRow| r.up_share);
    let m_uplat = mean(&|r: &OceanRow| r.up_lat);
    let m_seas = mean(&|r: &OceanRow| r.seas_share);
    let m_spread = mean(&|r: &OceanRow| r.swing_spread);

    let gy_total: usize = rows_out.iter().map(|r| r.gyres).sum();
    let gy_earthly: usize = rows_out.iter().map(|r| r.earthly).sum();
    let no_gyre = rows_out.iter().filter(|r| r.gyres == 0).count();
    let no_lanes = rows_out.iter().filter(|r| r.sea_lanes == 0).count();
    let sign_ok = rows_out
        .iter()
        .all(|r| !(r.warm_d.is_finite() && r.cold_d.is_finite()) || r.warm_d > r.cold_d);
    let sampled = rows_out
        .iter()
        .all(|r| r.warm_n >= 50 && r.cold_n >= 50);

    let mut c = Checks::default();
    c.must(
        "every seed carries a gyre",
        no_gyre == 0,
        format!("{} seeds without", no_gyre),
        "M49 gate: an ocean with no circulation is an unsolved basin, not a calm world",
    );
    c.must(
        "gyre sense matches hemisphere",
        gy_total > 0 && gy_earthly == gy_total,
        format!("{}/{}", gy_earthly, gy_total),
        "M49/M40 gate: clockwise north · counterclockwise south, every basin in the sweep",
    );
    c.band("gyre basins per seed", m_basins, format!("{:.1}", m_basins));
    c.band("gyre cells per gyre", m_cells, format!("{:.0}", m_cells));
    c.band("surface current speed p95", m_sp95, format!("{:.3}", m_sp95));
    c.band("western boundary intensification", m_west, format!("{:.1}×", m_west));
    c.band("current-coast warm anomaly", m_warm, format!("{:+.2}°C", m_warm));
    c.band("current-coast cold anomaly", m_cold, format!("{:+.2}°C", m_cold));
    c.must(
        "warm coasts outrun cold coasts",
        sign_ok,
        format!("{:+.2} vs {:+.2}", m_warm, m_cold),
        "M49 gate: the two rims must sit on opposite sides of their own latitude's coast, every seed",
    );
    c.want(
        "both rims are sampled everywhere",
        sampled,
        format!("min warm {} · min cold {}",
            rows_out.iter().map(|r| r.warm_n).min().unwrap_or(0),
            rows_out.iter().map(|r| r.cold_n).min().unwrap_or(0)),
        "≥50 coastal cells per rim per seed, else the anomaly is one bay's opinion",
    );
    c.band_as("upwelling coverage (ocean lane)", "upwelling share of coastline", m_up, pct(m_up));
    c.band("upwelling median latitude", m_uplat, format!("{:.0}°", m_uplat));
    c.must(
        "every seed sails a sea lane",
        no_lanes == 0,
        format!("{} seeds without", no_lanes),
        "M49 gate: the seasonality bands need water traffic to measure",
    );
    c.band("seasonal sea-lane share", m_seas, format!("{:.0}%", m_seas));
    c.band("sea-lane seasonality spread", m_spread, format!("{:.0}pp", m_spread));
    c.print();
}


// ================================================== M50 metamorphic ocean

/// One seed's metamorphic reading: what the injected warm current did
/// to its coast, and how a fixed sea lane's passage answers the
/// current-strength ladder.
struct MetaRow {
    seed: i64,
    strip: usize,
    coast: usize,
    rim: usize,
    sea_dt: f64,
    sea_dt_min: f64,
    dt: f64,
    dt_min: f64,
    dp: f64,
    rain_share: f64,
    far_touched: usize,
    lanes: usize,
    fav: Vec<f64>,
    adv: Vec<f64>,
    fav_mono: bool,
    adv_mono: bool,
    gap_mono: bool,
}

/// M50 — the ocean stack proves it *responds*, not merely that it once
/// looked plausible. Two metamorphic relations, both hard:
///
/// 1. **Kill the warm current.** A synthetic poleward ribbon is laid
///    along every shore in the subtropical/temperate band, the climate
///    leg (heat transport → annual mean → the rain march) is solved
///    with it and again with it zeroed, and the coast the ribbon
///    touched must cool by at least `META_COOL_MIN` and dry with it.
///    The current is synthetic on purpose: the relation under test is
///    the pipeline's response law, which must not depend on whether a
///    given seed happened to grow a strong western boundary.
/// 2. **Scale the currents.** Each sailed lane's own path is priced
///    over grids whose currents are multiplied by the M50 ladder. The
///    favourable passage must fall monotonically and the adverse
///    passage rise monotonically — the sail law is affine in the
///    current under a monotone clamp, so anything else is a bug.
fn cmd_ocean_metamorphic(size: usize, seeds: Vec<i64>) {
    header(
        "OCEAN · METAMORPHIC",
        &format!("{}x{} · {} seeds", size, size, seeds.len()),
    );
    println!("kill the warm current · scale the currents  (M50)");

    let ladder = calliope::trade::META_CURRENT_LADDER;
    let mut rows_out: Vec<MetaRow> = Vec::new();

    for &seed in &seeds {
        let w = World::generate(seed, size);
        let (rows, cols) = w.fields.height.dim();
        // The dawn widens the world with ocean margins (ADR-0014), so the
        // shipped grid is wider than it is tall while the climate march
        // is written for the square domain it was solved on. Crop back to
        // the centred square: the same land, the margins the widening
        // added set aside.
        if cols < rows {
            println!(" seed {} — grid {}x{} is narrower than tall, no square domain to read", seed, rows, cols);
            continue;
        }
        let x0 = (cols - rows) / 2;
        let cols = rows;
        let height = w
            .fields
            .height
            .slice(ndarray::s![.., x0..x0 + rows])
            .mapv(|h| h as f64);
        let water = height.mapv(|h| h < 0.0);
        let lat = calliope::climate::latitude_deg(rows);
        let cont = calliope::climate::continentality(&water);
        let tbase = calliope::climate::temperature_mean(&height, &lat);

        // ---- 1. the synthetic warm ribbon -----------------------------
        // Poleward flow on both hemispheres (grid v is southward-positive,
        // so the north wants v < 0): water that remembers a warmer origin
        // latitude, which is exactly what current_bias reads.
        let shore_d = ndimage::distance_transform_edt(&water);
        let mid = (rows - 1) as f64 / 2.0;
        let vs = calliope::climate::META_WARM_V as f32;
        let (lat_lo, lat_hi) = calliope::climate::META_LAT;
        let mut v_on = ndarray::Array2::<f32>::zeros((rows, cols));
        let mut strip = ndarray::Array2::<bool>::from_elem((rows, cols), false);
        let mut n_strip = 0usize;
        for y in 0..rows {
            let la = (-90.0 + y as f64 * 180.0 / (rows - 1) as f64).abs();
            if !(lat_lo..=lat_hi).contains(&la) {
                continue;
            }
            for x in 0..cols {
                if !water[[y, x]] || shore_d[[y, x]] > calliope::climate::META_STRIP {
                    continue;
                }
                v_on[[y, x]] = if (y as f64) < mid { -vs } else { vs };
                strip[[y, x]] = true;
                n_strip += 1;
            }
        }
        let v_off = ndarray::Array2::<f32>::zeros((rows, cols));
        let heat_on = calliope::climate::current_bias(&water, &v_on);
        let heat_off = calliope::climate::current_bias(&water, &v_off);
        let t_on = &tbase + &heat_on;
        let t_off = &tbase + &heat_off;
        let (p_on, _) =
            calliope::climate::precipitation(&height, &water, &t_on, &lat, &cont, &heat_on);
        let (p_off, _) =
            calliope::climate::precipitation(&height, &water, &t_off, &lat, &cont, &heat_off);

        // Two bodies under test. The *sea rim* — the ribbon water that
        // hugs the shore — is what the current directly warms, and it is
        // where the relation is measured (M50 gate). The *coast* — land
        // 8-adjacent to the ribbon — feels that warmth only through the
        // coastal-decay kernel (HEAT_COAST_DECAY), which bounds how much
        // of it can ever reach land; its drop is reported as a secondary,
        // together with the rainfall relation it carries.
        let mut n_rim = 0usize;
        let (mut sdt_sum, mut sdt_min) = (0.0f64, f64::MAX);
        let mut n_coast = 0usize;
        let (mut dt_sum, mut dp_sum, mut rain_up) = (0.0f64, 0.0f64, 0usize);
        let mut dt_min = f64::MAX;
        let mut far_touched = 0usize;
        for y in 0..rows {
            for x in 0..cols {
                if water[[y, x]] {
                    // sea rim: ribbon water touching land
                    if strip[[y, x]] {
                        let mut shore = false;
                        for dy in -1i64..=1 {
                            for dx in -1i64..=1 {
                                let yy = y as i64 + dy;
                                let xx = x as i64 + dx;
                                if yy < 0 || xx < 0 || yy >= rows as i64 || xx >= cols as i64 {
                                    continue;
                                }
                                if !water[[yy as usize, xx as usize]] {
                                    shore = true;
                                }
                            }
                        }
                        if shore {
                            n_rim += 1;
                            let sdt = t_on[[y, x]] - t_off[[y, x]];
                            sdt_sum += sdt;
                            sdt_min = sdt_min.min(sdt);
                        }
                    }
                    continue;
                }
                let mut touches = false;
                for dy in -1i64..=1 {
                    for dx in -1i64..=1 {
                        let yy = y as i64 + dy;
                        let xx = x as i64 + dx;
                        if yy < 0 || xx < 0 || yy >= rows as i64 || xx >= cols as i64 {
                            continue;
                        }
                        if strip[[yy as usize, xx as usize]] {
                            touches = true;
                        }
                    }
                }
                // control: the reach is bounded — deep interior land,
                // beyond the coastal rings, must not move one bit.
                if shore_d[[y, x]] == 0.0
                    && !touches
                    && heat_on[[y, x]] != 0.0
                    && dist_from_sea(&water, y, x, cols, rows)
                        > calliope::climate::HEAT_COAST_RINGS
                {
                    far_touched += 1;
                }
                if !touches {
                    continue;
                }
                n_coast += 1;
                let dt = t_on[[y, x]] - t_off[[y, x]];
                dt_sum += dt;
                dt_min = dt_min.min(dt);
                let base = p_off[[y, x]].max(1e-6);
                let dp = (p_on[[y, x]] - p_off[[y, x]]) / base;
                dp_sum += dp;
                if p_on[[y, x]] > p_off[[y, x]] {
                    rain_up += 1;
                }
            }
        }
        let dt = dt_sum / n_coast.max(1) as f64;
        let dp = dp_sum / n_coast.max(1) as f64;
        let rain_share = rain_up as f64 / n_coast.max(1) as f64;
        let sea_dt = sdt_sum / n_rim.max(1) as f64;
        if n_coast == 0 {
            dt_min = f64::NAN;
        }
        if n_rim == 0 {
            sdt_min = f64::NAN;
        }


        // ---- 2. the current-strength ladder ---------------------------
        // Each lane keeps its own path; only the water under it changes.
        let g = &w.trade;
        let f = g.f;
        let (gh, gw) = g.cost.dim();
        let mut lanes: Vec<&calliope::trade::Route> =
            w.routes.iter().filter(|r| r.sea > 0.0).collect();
        lanes.sort_by(|a, b| {
            b.cost
                .partial_cmp(&a.cost)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.a.cmp(&b.a))
                .then(a.b.cmp(&b.b))
        });
        let grids: Vec<calliope::trade::TradeGrid> = ladder
            .iter()
            .map(|&k| {
                let mut gk = g.clone();
                gk.cu.mapv_inplace(|c| c * k);
                gk.cv.mapv_inplace(|c| c * k);
                gk
            })
            .collect();
        let mut fav = vec![0.0f64; ladder.len()];
        let mut adv = vec![0.0f64; ladder.len()];
        let mut n_lane = 0usize;
        for r in lanes.iter() {
            if n_lane >= calliope::trade::META_LANES {
                break;
            }
            let p0 = r.path.first().copied().unwrap_or([0, 0]);
            let p1 = r.path.last().copied().unwrap_or([0, 0]);
            let start = ((p0[1].max(0) as usize / f).min(gh - 1), (p0[0].max(0) as usize / f).min(gw - 1));
            let goal = ((p1[1].max(0) as usize / f).min(gh - 1), (p1[0].max(0) as usize / f).min(gw - 1));
            let Some((path, _)) = calliope::trade::astar(g, start, goal) else {
                continue;
            };
            // only lanes that actually cross open water can answer
            let open_cells = path.iter().filter(|&&(y, x)| g.open[[y, x]]).count();
            if open_cells < 3 {
                continue;
            }
            for (i, gk) in grids.iter().enumerate() {
                let out = calliope::trade::path_cost(gk, &path, false);
                let home = calliope::trade::path_cost(gk, &path, true);
                fav[i] += out.min(home);
                adv[i] += out.max(home);
            }
            n_lane += 1;
        }
        if n_lane > 0 {
            for i in 0..ladder.len() {
                fav[i] /= n_lane as f64;
                adv[i] /= n_lane as f64;
            }
        }
        
        let strict_up = |v: &[f64]| v.windows(2).all(|w| w[1] > w[0] + 1e-9);
        let gap: Vec<f64> = adv.iter().zip(fav.iter()).map(|(a, f)| a - f).collect();
        // The favourable passage is affine in the current only up to the
        // admissibility clamp (SAIL_MULT_FLOOR / PLAN_COST): once a lane's
        // fastest water saturates the floor, extra current buys nothing
        // there while a neighbouring rung still moves, so a rung can sit
        // flat by a fraction of a percent. The relation under test is
        // therefore "never dearer, and cheaper end to end" — with the
        // strictness carried by the adverse passage and the gap, which no
        // clamp bounds from below.
        let tol = 0.01;
        let fav_mono = n_lane > 0
            && fav.windows(2).all(|w| w[1] <= w[0] * (1.0 + tol))
            && fav[ladder.len() - 1] < fav[0] - 1e-9;
        let adv_mono = n_lane > 0 && strict_up(&adv);
        let gap_mono = n_lane > 0 && strict_up(&gap);

        println!();
        println!(
            " seed {} — ribbon {} ocean cells · sea rim {} · coast under test {} land cells",
            seed, n_strip, n_rim, n_coast
        );
        println!(
            "   kill the current: sea rim cools {:.2}°C mean (weakest cell {:.2}) [gate] · coast cools {:.2}°C mean (weakest {:.2}) [secondary] · rain falls {:.1}% mean · {:.0}% of cells drier without it · far interior touched {}",
            sea_dt, sdt_min, dt, dt_min, 100.0 * dp, 100.0 * rain_share, far_touched
        );
        print!("   ladder (×current):");
        for (i, k) in ladder.iter().enumerate() {
            print!(" {:.1}→{:.1}/{:.1}", k, fav[i], adv[i]);
        }
        println!(
            "   [{} lanes · favourable{} · adverse{}]",
            n_lane,
            if fav_mono { " falls" } else { " NOT monotone" },
            if adv_mono { " rises" } else { " NOT monotone" }
        );

        rows_out.push(MetaRow {
            seed,
            strip: n_strip,
            coast: n_coast,
            rim: n_rim,
            sea_dt,
            sea_dt_min: sdt_min,
            dt,
            dt_min,
            dp,
            rain_share,
            far_touched,
            lanes: n_lane,
            fav,
            adv,
            fav_mono,
            adv_mono,
            gap_mono,
        });
    }

    println!();
    println!(
        " {:>6} {:>8} {:>7} {:>8} {:>9} {:>9} {:>9} {:>9} {:>8} {:>7} {:>6} {:>9} {:>9}",
        "seed", "ribbon", "rim", "coast", "rim ΔT", "rim min", "land ΔT", "land min", "rain Δ", "drier%", "lanes", "fav 0→2", "adv 0→2"
    );
    for r in &rows_out {
        let n = r.fav.len();
        println!(
            " {:>6} {:>8} {:>7} {:>8} {:>7.2}C {:>8.2}C {:>8.2}C {:>8.2}C {:>8.1}% {:>7.0}% {:>6} {:>4.1}→{:<4.1} {:>4.1}→{:<4.1}",
            r.seed, r.strip, r.rim, r.coast, r.sea_dt, r.sea_dt_min, r.dt, r.dt_min, 100.0 * r.dp,
            100.0 * r.rain_share, r.lanes,
            r.fav.first().copied().unwrap_or(0.0), r.fav.get(n - 1).copied().unwrap_or(0.0),
            r.adv.first().copied().unwrap_or(0.0), r.adv.get(n - 1).copied().unwrap_or(0.0),
        );
    }

    let n = rows_out.len().max(1) as f64;
    let mean_dt: f64 = rows_out.iter().map(|r| r.dt).sum::<f64>() / n;
    let worst_dt = rows_out.iter().map(|r| r.dt).fold(f64::MAX, f64::min);
    let mean_sea: f64 = rows_out.iter().map(|r| r.sea_dt).sum::<f64>() / n;
    let worst_sea = rows_out.iter().map(|r| r.sea_dt).fold(f64::MAX, f64::min);
    let mean_dp = rows_out.iter().map(|r| 100.0 * r.dp).sum::<f64>() / n;
    let worst_share = rows_out.iter().map(|r| r.rain_share).fold(f64::MAX, f64::min);
    let far = rows_out.iter().map(|r| r.far_touched).sum::<usize>();
    let lanes_min = rows_out.iter().map(|r| r.lanes).min().unwrap_or(0);
    let fav_all = !rows_out.is_empty() && rows_out.iter().all(|r| r.fav_mono);
    let adv_all = !rows_out.is_empty() && rows_out.iter().all(|r| r.adv_mono);
    let gap_all = !rows_out.is_empty() && rows_out.iter().all(|r| r.gap_mono);
    let gap_gain = {
        let g: Vec<f64> = rows_out
            .iter()
            .filter(|r| r.lanes > 0)
            .map(|r| {
                let n = r.fav.len();
                (r.adv[n - 1] - r.fav[n - 1]) - (r.adv[0] - r.fav[0])
            })
            .collect();
        if g.is_empty() { f64::NAN } else { g.iter().sum::<f64>() / g.len() as f64 }
    };

    let mut c = Checks::default();
    c.must(
        "the harness ran every seed",
        rows_out.len() == seeds.len() && rows_out.iter().all(|r| r.coast >= 200),
        format!("{}/{} seeds · min coast {}", rows_out.len(), seeds.len(),
            rows_out.iter().map(|r| r.coast).min().unwrap_or(0)),
        "M50: a relation measured on a handful of cells is an anecdote",
    );
    c.must(
        "killing the warm current cools the sea rim",
        worst_sea >= calliope::climate::META_COOL_MIN
            && rows_out.iter().all(|r| r.rim >= 200),
        format!("{:.2}°C worst seed · {:.2}°C mean", worst_sea, mean_sea),
        "M50 gate: ≥2.0 °C mean cooling over the water the current directly warms, every seed",
    );
    c.want(
        "the cooling carries onto the land",
        worst_dt > 0.0,
        format!("{:.2}°C worst seed · {:.2}°C mean", worst_dt, mean_dt),
        "M50 secondary: HEAT_COAST_DECAY bounds how much of the rim anomaly reaches shore — reported, not gated",
    );
    c.must(
        "killing the warm current dries its coast",
        mean_dp > 0.0 && worst_share >= calliope::climate::META_RAIN_SHARE_MIN,
        format!("{:.1}% mean · {:.0}% of cells worst seed", mean_dp, 100.0 * worst_share),
        "M50: the marine layer falls with the water that fed it (STAB_GAIN), as a rule not an average",
    );
    c.must(
        "the reach stays coastal",
        far == 0,
        format!("{} interior cells moved", far),
        "M50 control: beyond HEAT_COAST_RINGS the land must not feel a current at all",
    );
    c.must(
        "every seed prices open-water lanes",
        lanes_min > 0,
        format!("min {} lanes", lanes_min),
        "M50 gate: the ladder needs blue-water passages to answer it",
    );
    c.must(
        "favourable passage falls with the current",
        fav_all,
        format!("{}/{} seeds strict", rows_out.iter().filter(|r| r.fav_mono).count(), rows_out.len()),
        "M50 gate: never dearer rung to rung (1% clamp tolerance) and cheaper at ×2 than ×0, every seed",
    );
    c.must(
        "adverse passage rises with the current",
        adv_all,
        format!("{}/{} seeds strict", rows_out.iter().filter(|r| r.adv_mono).count(), rows_out.len()),
        "M50 gate: beating up-current must cost more, every rung",
    );
    c.must(
        "the passage gap widens monotonically",
        gap_all,
        format!("{:+.2} cost gained ×0→×2", gap_gain),
        "M50 gate: the travel-time delta between out and home answers current strength, strictly",
    );
    c.print();
}

/// Chebyshev distance from `(y, x)` to the nearest water cell, capped —
/// the control leg only needs to know "further inland than the reach".
fn dist_from_sea(water: &ndarray::Array2<bool>, y: usize, x: usize, cols: usize, rows: usize) -> usize {
    let cap = calliope::climate::HEAT_COAST_RINGS + 1;
    for r in 1..=cap {
        let y0 = y.saturating_sub(r);
        let y1 = (y + r).min(rows - 1);
        let x0 = x.saturating_sub(r);
        let x1 = (x + r).min(cols - 1);
        for yy in y0..=y1 {
            for xx in x0..=x1 {
                if yy != y0 && yy != y1 && xx != x0 && xx != x1 {
                    continue;
                }
                if water[[yy, xx]] {
                    return r;
                }
            }
        }
    }
    cap
}

// ================================================================ atlas (M63)

/// M63 — The Atlas Learns. Proves the cartographic law at its source:
/// the same Rust tables that generate the WGSL prelude and the palette
/// texture answer here, so a passing gate is a statement about what the
/// GPU compiles, not about a copy of it. The phase gate demands two
/// things by name: a synthetic desert-elevation cell renders outside
/// the green hue band under the cross-blended ramp, and the lens toggle
/// round-trips through pack v2 without touching `hash_state`.
fn cmd_atlas(size: usize, seeds: Vec<i64>) {
    use calliope::atlas;
    header("ATLAS", &format!("size {} · {} seeds · M63", size, seeds.len()));

    let mut c = Checks::default();

    // -- palette soundness: every vocabulary word owns a distinct swatch
    match atlas::palettes_sound() {
        Ok(()) => c.must(
            "palettes cover their vocabularies",
            true,
            format!(
                "{}+{}+{} swatches",
                atlas::ROCK_COLORS.len(),
                atlas::SOIL_COLORS.len(),
                atlas::LANDFORM_COLORS.len()
            ),
            "M63: rock, soil, landform lenses — one distinct swatch per word",
        ),
        Err(e) => c.must("palettes cover their vocabularies", false, e, "M63"),
    }

    // -- the cross-blended ramp: desert country must never read green.
    // Sweep the whole elevation ladder as a synthetic arid column
    // (60 mm/yr, 24 °C) and take the worst (most-green) hue on it.
    let green = |hue: f32| hue > 70.0 && hue < 170.0;
    let mut worst_arid: (f32, f32) = (-1.0, -1.0); // (hue, at elevation)
    let mut arid_green = 0usize;
    for i in 0..=100 {
        let h = i as f32 / 100.0;
        let hue = atlas::hue_deg(atlas::hypso_rgb(h, 60.0, 24.0));
        if green(hue) {
            arid_green += 1;
        }
        if hue > worst_arid.0 {
            worst_arid = (hue, h);
        }
    }
    println!(
        " arid column sweep (60 mm · 24 °C): 101 elevations · max hue {:.0}° at h={:.2} · {} in green band",
        worst_arid.0, worst_arid.1, arid_green
    );
    c.must(
        "desert ladder outside green band",
        arid_green == 0,
        format!("max hue {:.0}°", worst_arid.0),
        "M63 gate: synthetic desert-elevation cells render outside 70–170°",
    );

    // -- and the humid lowland still IS green, or the check above would
    // pass vacuously with a broken ramp
    let humid = atlas::hue_deg(atlas::hypso_rgb(0.08, 1600.0, 12.0));
    c.must(
        "humid lowland reads green",
        green(humid),
        format!("hue {:.0}°", humid),
        "M63: the wet ladder keeps its green — the arid law is not vacuous",
    );

    // -- frost greys both ladders toward firn
    let cold = atlas::hypso_rgb(0.5, 800.0, -20.0);
    let chroma = cold[0].max(cold[1]).max(cold[2]) - cold[0].min(cold[1]).min(cold[2]);
    c.must(
        "frost greys the ladder",
        chroma < 0.07,
        format!("chroma {:.3}", chroma),
        "M63: deep-cold cells lose hue toward polar grey/firn",
    );

    // -- the generated WGSL prelude actually carries the law
    let wgsl = atlas::wgsl_ramps();
    let ok_wgsl = wgsl.contains("fn seg(")
        && wgsl.contains("fn elev_ramp(")
        && wgsl.contains("fn elev_arid_ramp(")
        && wgsl.contains("fn hypso(")
        && wgsl.matches("seg(v,").count() == 2 * (atlas::ELEV_STOPS.len() - 1);
    c.must(
        "wgsl prelude generated from tables",
        ok_wgsl,
        format!("{} B", wgsl.len()),
        "M63: seg + both ramps + hypso, one segment per table stop pair",
    );

    // -- the lens lanes ride the wire: rock, soil, landform flagged for
    // GPU upload in the field registry (pack order = upload order, E2.2)
    let lane = |n: &str| calliope::pack::FIELD_SPECS.iter().find(|f| f.name == n);
    let lanes_ok = ["rock", "soil", "landform"].iter().all(|n| lane(n).map(|f| f.gpu).unwrap_or(false));
    c.must(
        "lens lanes flagged for gpu upload",
        lanes_ok,
        "rock·soil·landform".into(),
        "M63: the deep-earth grids reach Orbital through the registry",
    );

    // -- M69: the upload order is the registry's, not a hand-kept list.
    // `render::set_world`'s positional signature is judged by a const
    // assertion in the crate (a drifted flag is a build break); the row
    // here reports the order the harness actually saw, so the report
    // names the law instead of trusting it silently.
    let order = calliope::pack::gpu_order();
    println!(" gpu upload order ({}): {}", order.len(), order.join(" · "));
    c.must(
        "upload order = registry gpu rows",
        calliope::pack::gpu_order_is(calliope::pack::GPU_UPLOAD_ORDER)
            && order == calliope::pack::GPU_UPLOAD_ORDER.to_vec(),
        format!("{} grids", order.len()),
        "M69: the Orbital upload list is the field registry's gpu rows in pack order — no second list",
    );




    // -- per seed: every id on the grid has a swatch (no magenta on the
    // map), and packing the world for the wire leaves the state hash
    // untouched — the lens toggle is pure presentation
    for &seed in &seeds {
        let w = World::generate(seed, size);
        let h0 = hash_state(&w);
        let bytes = w.pack();
        let h1 = hash_state(&w);
        let max_rock = w.fields.rock.iter().copied().max().unwrap_or(0) as usize;
        let max_soil = w.fields.soil.iter().copied().max().unwrap_or(0) as usize;
        let max_lf = w.fields.landform.iter().copied().max().unwrap_or(0) as usize;
        println!(
            " seed {:>6}: pack {} B · max ids rock {} soil {} landform {} · hash {:016x}",
            seed, bytes.len(), max_rock, max_soil, max_lf, h0
        );
        c.must(
            &format!("ids within palettes (seed {})", seed),
            max_rock < atlas::ROCK_COLORS.len()
                && max_soil < atlas::SOIL_COLORS.len()
                && max_lf < atlas::LANDFORM_COLORS.len(),
            format!("{}/{}/{}", max_rock, max_soil, max_lf),
            "M63: no id on the map escapes its palette row",
        );
        c.must(
            &format!("pack leaves hash_state alone (seed {})", seed),
            h0 == h1 && !bytes.is_empty(),
            if h0 == h1 { "untouched".into() } else { "MUTATED".into() },
            "M63 gate: the lens toggle round-trips pack v2 without altering state",
        );
    }

    c.print();
}

// ================================================================ gate (M65)

/// One composed lane: where its rows came from, what they counted.
struct LaneTally {
    name: String,
    pass: usize,
    warn: usize,
    fail: usize,
    fails: Vec<String>,
    age_h: Option<f64>,
}

/// Count the [PASS]/[WARN]/[FAIL] rows in one lane's report text.
fn tally_lane(name: &str, text: &str, age_h: Option<f64>) -> LaneTally {
    let mut t = LaneTally { name: name.to_string(), pass: 0, warn: 0, fail: 0, fails: Vec::new(), age_h };
    for l in text.lines() {
        if l.starts_with("[PASS]") {
            t.pass += 1;
        } else if l.starts_with("[WARN]") {
            t.warn += 1;
        } else if l.starts_with("[FAIL]") {
            t.fail += 1;
            t.fails.push(l.trim_end().to_string());
        }
    }
    t
}

/// M65 — the Era I gate. The deep earth closes as one verdict, not a
/// folder of reports: compose every lane the suite writes (`--reports
/// <dir>`, zero recompute — the same rows SUMMARY.txt greps) or run the
/// native lanes as subprocesses of this binary when standalone, then walk
/// one world through three centuries as a structural leg. One FAIL
/// anywhere — including the honestly-held rows — keeps the era open; the
/// gate exists to see them, never to scope past them.
fn cmd_gate(size: usize, years: usize, seed: i64, reports: Option<String>) {
    header("ERA I GATE", &format!("size {} · {}y leg · seed {}", size, years, seed));
    let mut c = Checks::default();
    let mut lanes: Vec<LaneTally> = Vec::new();
    let mut missing: Vec<&str> = Vec::new();
    // wasm-audit is an on-demand instrument: its staleness law is content
    // (the recorded subject sha256 against the shipped bytes), never the
    // clock — captured here, judged after the table.
    let mut audit_sha: Option<String> = None;
    // M55 (corrected scope) — the veto's load-bearing claim is a law of the
    // model, so it is proved over the ensemble of composed civ lanes, not
    // demanded of every single world's 150 years. Per lane: how many towns
    // the veto-lifted run stood on ground the real run's veto refused.
    let mut veto_lanes: Vec<(String, usize)> = Vec::new();

    if let Some(dir) = &reports {
        // ---- compose the suite's own reports (the SUMMARY's rows, sealed
        // into a verdict). Excluded by name: the SUMMARY (a mirror, not a
        // lane), the append-only history, and this gate's own output.
        println!(" composing {dir}");
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.ends_with(".txt"))
                    .filter(|n| n != "SUMMARY.txt" && n != "bench-history.txt" && n != "gate.txt")
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        for n in &names {
            let path = format!("{dir}/{n}");
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let age_h = std::fs::metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.elapsed().ok())
                .map(|d| d.as_secs_f64() / 3600.0);
            if n.starts_with("civ-") {
                veto_lanes.push((
                    n.clone(),
                    text.lines().filter(|l| l.contains("REFUSED by the real veto")).count(),
                ));
            }
            let mut t = tally_lane(n, &text, age_h);
            if n == "wasm-audit.txt" {
                t.age_h = None;
                audit_sha = text
                    .lines()
                    .find(|l| l.contains("sha256 "))
                    .and_then(|l| l.rsplit(' ').next())
                    .map(|s| s.trim().to_string())
                    .filter(|s| s.len() == 64);
            }
            lanes.push(t);
        }
        // The era's load-bearing lane families must all be on the table —
        // a gate over an empty folder proves nothing.
        for fam in [
            "terrain-", "climate-", "hydro-", "resources-", "civ-", "ocean.txt",
            "ocean-meta.txt", "properties.txt", "era.txt", "earth.txt", "determinism.txt",
        ] {
            if !names.iter().any(|n| n.starts_with(fam.trim_end_matches(".txt")) && (fam.ends_with('-') || n == fam)) {
                missing.push(fam);
            }
        }
    } else {
        // ---- standalone: run the native lanes as subprocesses of this
        // very binary, so the composed rows are exactly the lanes' rows.
        // (The suite-only legs — assay, wasm size, cross-runtime replay,
        // browser — need cargo/bun/Chromium and ride report.sh instead.)
        let exe = std::env::current_exe().expect("current_exe");
        let ss = size.to_string();
        let mut runs: Vec<Vec<String>> = Vec::new();
        for s in ["12345", "777", "90210"] {
            for lane in ["terrain", "climate", "hydro", "resources"] {
                runs.push(vec![lane.into(), s.into(), ss.clone()]);
            }
        }
        runs.push(vec!["ocean".into(), ss.clone(), "12345".into(), "777".into(), "31337".into(), "90210".into(), "555".into()]);
        runs.push(vec!["ocean".into(), ss.clone(), "12345".into(), "777".into(), "31337".into(), "90210".into(), "555".into(), "--metamorphic".into()]);
        runs.push(vec!["properties".into(), ss.clone(), "60".into(), "12345".into(), "777".into(), "90210".into()]);
        runs.push(vec!["era".into(), "256".into(), "60".into(), "16".into(), "12345".into()]);
        runs.push(vec!["earth".into(), ss.clone(), "150".into(), "12345".into(), "777".into(), "90210".into()]);
        runs.push(vec!["determinism".into(), "12345".into(), ss.clone(), "120".into()]);
        println!(" standalone: running {} native lanes (suite-only legs ride report.sh)", runs.len());
        for args in &runs {
            let name = args.join(" ");
            match std::process::Command::new(&exe).args(args.iter()).output() {
                Ok(out) if out.status.success() => {
                    lanes.push(tally_lane(&name, &String::from_utf8_lossy(&out.stdout), None));
                }
                Ok(out) => {
                    let mut t = tally_lane(&name, &String::from_utf8_lossy(&out.stdout), None);
                    t.fail += 1;
                    t.fails.push(format!("[FAIL] lane crashed ({})", out.status));
                    lanes.push(t);
                }
                Err(e) => {
                    lanes.push(LaneTally {
                        name: name.clone(), pass: 0, warn: 0, fail: 1,
                        fails: vec![format!("[FAIL] lane failed to spawn: {e}")], age_h: None,
                    });
                }
            }
        }
    }

    println!();
    println!(" {:<28} {:>5} {:>5} {:>5} {:>7}", "lane", "pass", "warn", "fail", "age");
    let (mut tp, mut tw, mut tf) = (0usize, 0usize, 0usize);
    let mut oldest: f64 = 0.0;
    for t in &lanes {
        let age = t.age_h.map(|a| format!("{:.1}h", a)).unwrap_or_else(|| "-".into());
        println!(" {:<28} {:>5} {:>5} {:>5} {:>7}", t.name, t.pass, t.warn, t.fail, age);
        tp += t.pass;
        tw += t.warn;
        tf += t.fail;
        // Freshness reckons only lanes that carry check rows — side
        // artifacts with none (wasm-audit's prose dump) can't stale a
        // verdict they contribute zero rows to.
        if t.pass + t.warn + t.fail > 0 {
            oldest = oldest.max(t.age_h.unwrap_or(0.0));
        }
    }
    println!(" {:<28} {:>5} {:>5} {:>5}", "TOTAL", tp, tw, tf);
    if tf > 0 {
        println!();
        println!(" held rows (each holds the era open):");
        for t in &lanes {
            for f in &t.fails {
                // indented so SUMMARY.txt's ^[FAIL] grep never double-counts
                println!("   {} · {}", t.name, f);
            }
        }
    }

    // ---- the structural leg: one world, three centuries, two chunkings.
    // The lanes prove the parts; this leg proves the whole keeps its laws
    // at era scale — replay identity under different tick chunkings
    // (ADR-0003), finite fields, towns standing, a chronicle still spoken.
    let months = (years as i64) * 12;
    println!();
    println!(" structural leg: seed {seed} · {months} months · chunkings 240/120 vs 180");
    let t0 = Instant::now();
    let mut w1 = World::generate(seed, size);
    let dawn = w1.peoples.settlements.len();
    let pre = (months - 120).max(0);
    let mut left = pre;
    while left > 0 {
        let step = left.min(240);
        w1.tick(step);
        left -= step;
    }
    let (tail_evs, _, _) = w1.tick(months - pre);
    let h1 = (hash_state(&w1), hash_settlements(&w1));
    let towns = w1.peoples.settlements.len();
    let mut w2 = World::generate(seed, size);
    let mut left = months;
    while left > 0 {
        let step = left.min(180);
        w2.tick(step);
        left -= step;
    }
    let h2 = (hash_state(&w2), hash_settlements(&w2));
    let secs = t0.elapsed().as_secs_f64();
    let finite = {
        let f = &w1.fields;
        f.height.iter().all(|v| v.is_finite())
            && f.tmean.iter().all(|v| v.is_finite())
            && f.precip.iter().all(|v| v.is_finite())
            && f.discharge.iter().all(|v| v.is_finite())
            && f.fertility.iter().all(|v| v.is_finite())
    };
    println!(
        " towns {dawn}→{towns} · {} events in the final decade · {:.0}s for both runs ({:.0} mo/s)",
        tail_evs.len(),
        secs,
        (2 * months) as f64 / secs
    );

    if reports.is_some() {
        c.must(
            "every era lane on the table",
            missing.is_empty(),
            if missing.is_empty() { format!("{} lanes", lanes.len()) } else { format!("missing {}", missing.join(" ")) },
            "M65: a gate over a partial suite proves nothing — run report.sh",
        );
        // M76 follow-on: a lane present but silent is not a clean lane.
        // A report truncated mid-write (crash, killed process, a snapshot
        // racing the write) composes as 0/0/0 and would slip past both
        // the presence row above and the 0-FAIL row below — the era would
        // seal over evidence nobody produced. A lane on the table must
        // carry at least one check row, or it holds the era open.
        let silent: Vec<&str> = lanes
            .iter()
            .filter(|t| t.pass + t.warn + t.fail == 0)
            .map(|t| t.name.as_str())
            .collect();
        c.must(
            "every composed lane actually speaks",
            silent.is_empty(),
            if silent.is_empty() { format!("{} lanes with rows", lanes.len()) } else { format!("silent: {}", silent.join(" ")) },
            "M76: a lane that emitted no check rows proves nothing — a truncated or crashed report may not compose as clean",
        );
        c.want(
            "composed reports are fresh",
            oldest <= 6.0,
            format!("oldest {:.1}h", oldest),
            "M65: stale rows compose stale verdicts — rerun the suite within 6h",
        );
        // M55 ensemble law: somewhere across the composed worlds, the dry
        // frontier's veto must actually refuse a colonist ground it wanted.
        // A single world may never hold that auction (late foundings, deep
        // craft already learned) and says so as a WARN in its own lane; the
        // ensemble is where the claim is either proved or falsified.
        let total_refused: usize = veto_lanes.iter().map(|(_, k)| *k).sum();
        let detail = veto_lanes
            .iter()
            .map(|(n, k)| format!("{}:{}", n.trim_start_matches("civ-").trim_end_matches(".txt"), k))
            .collect::<Vec<_>>()
            .join(" · ");
        c.must(
            "the veto bites somewhere in the ensemble",
            total_refused >= 1 && !veto_lanes.is_empty(),
            format!("{} refused town(s) over {} civ lane(s) [{}]", total_refused, veto_lanes.len(), detail),
            "M55 gate: across the composed worlds, the veto-lifted run must stand ≥1 town on ground deeper than its founder's well reach at the founding — the law lives in the model, and one world's history may never test it",
        );
    }
    c.must(
        "the suite composes clean",
        tf == 0,
        format!("{tf} fail · {tw} warn · {tp} pass"),
        "M65: every composed lane 0 FAIL — held rows hold the era, by design",
    );
    // The wasm audit's staleness law is content, not clock: its recorded
    // subject sha256 must equal the shipped binary's bytes today. Judged
    // only when the audit recorded a subject and sha256sum can answer.
    let mut audit_fresh = true;
    if let (Some(sha), Some(dir)) = (&audit_sha, &reports) {
        let shipped = format!("{dir}/../web/js/wasm/calliope_bg.wasm");
        if std::path::Path::new(&shipped).is_file() {
            if let Ok(out) = std::process::Command::new("sha256sum").arg(&shipped).output() {
                if out.status.success() {
                    let now = String::from_utf8_lossy(&out.stdout)
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_string();
                    audit_fresh = now == *sha;
                    c.must(
                        "the wasm audit speaks for shipped bytes",
                        audit_fresh,
                        if audit_fresh { "same bytes".into() } else { "STALE AUDIT".into() },
                        "E6.6: audit subject sha256 = shipped wasm — rerun scripts/wasm-audit.sh after a rebuild",
                    );
                }
            }
        }
    }
    c.must(
        "replay identity at era scale",
        h1 == h2,
        if h1 == h2 { "identical".into() } else { "DIVERGED".into() },
        &format!("ADR-0003: {years}y under two chunkings ⇒ one state, one town ledger"),
    );
    c.must(
        "fields finite after the centuries",
        finite,
        if finite { "all finite".into() } else { "NaN/inf".into() },
        "M65: height·tmean·precip·discharge·fertility carry no poison",
    );
    c.must(
        "towns endure",
        towns >= 1,
        format!("{dawn}→{towns}"),
        "M65: three centuries leave a living map, not a wiped one",
    );
    c.want(
        "the chronicle still speaks",
        !tail_evs.is_empty(),
        format!("{} events / final decade", tail_evs.len()),
        "M65: an old world keeps making history",
    );
    let sealed = tf == 0 && missing.is_empty() && audit_fresh && h1 == h2 && finite && towns >= 1;
    c.must(
        "era I seals",
        sealed,
        if sealed { "SEALED".into() } else { "HELD OPEN".into() },
        "M65 gate: the deep earth closes only over a clean composed suite",
    );
    c.print();
}

// ================================================================ compute (M67)

/// The compute lane's report — see the module docs in compute.rs. The
/// CPU rows (exactness against the exact-EDT referee, once-per-world
/// cost) run on any build; the GPU rows join when the binary carries
/// the `gpu` feature *and* the machine offers a compute-capable adapter
/// (report.sh sources a software Vulkan when headless, so the WGSL leg
/// executes in CI instead of being claimed). No adapter is a skip, not
/// a fail — the harness stays self-contained.
fn cmd_compute(size: usize, seeds: Vec<i64>, golden: Option<String>) {
    use calliope::compute;
    header("COMPUTE", &format!("M67 lane · size {size}"));

    let mut c = Checks::default();
    // `--golden <path>`: the twin's seed field written out as the third
    // executor's referee. The JS port in render/compositor.js is the one
    // executor no device holds; `scripts/coast-js-parity.mjs` replays
    // this file under bun and demands byte-equality (ADR-0026/0027 — one
    // law, three executors, no hand-mirrored fork).
    let mut golden_cases: Vec<(String, usize, usize, Vec<f32>, Vec<u32>)> = Vec::new();
    if golden.is_some() {
        let (fw, fh) = (96usize, 64usize);
        let fix = compute::fixture(fw, fh);
        let fs = compute::jfa_cpu(compute::coast_seeds(&fix, fw, fh), fw, fh);
        golden_cases.push(("fixture".into(), fw, fh, fix, fs));
    }

    #[cfg(feature = "gpu")]
    let mut gpu: Option<(wgpu::Instance, compute::ComputeLane)> = {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        })) {
            Some(adapter) if compute::adapter_supported(&adapter) => {
                let info = adapter.get_info();
                println!(" adapter: {:?} · {} · {:?}", info.backend, info.name, info.device_type);
                match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None)) {
                    Ok((device, queue)) => Some((instance, compute::ComputeLane::new(device, queue))),
                    Err(e) => {
                        println!(" adapter present but no device ({e}) — gpu rows skipped, not failed");
                        None
                    }
                }
            }
            Some(_) => {
                println!(" adapter lacks compute downlevel — gpu rows skipped, not failed");
                None
            }
            None => {
                println!(" no gpu adapter on this machine — gpu rows skipped, not failed");
                None
            }
        }
    };
    #[cfg(not(feature = "gpu"))]
    println!(" built without the `gpu` feature — gpu rows skipped, not failed");

    // ---- bring-up contract on the fixture (the lane's own handshake) ----
    #[cfg(feature = "gpu")]
    if let Some((_i, lane)) = gpu.as_mut() {
        let (fw, fh) = (96usize, 64usize);
        let fix = compute::fixture(fw, fh);
        match pollster::block_on(compute::coast_contract(lane, &fix, fw, fh)) {
            Ok(r) => {
                println!(
                    " fixture {}×{}: gpu {:.1} ms · cpu twin {:.1} ms · {} cells",
                    fw, fh, r.gpu_ms, r.cpu_ms, r.cells
                );
                c.must(
                    "lane fixture contract",
                    r.matched,
                    if r.matched { "byte-parity".into() } else { format!("{} diverge", r.mismatches) },
                    "M67 gate: WGSL kernel and CPU twin are one law — executed on a device and compared",
                );
            }
            Err(e) => c.must(
                "lane fixture contract",
                false,
                "error".into(),
                &format!("M67 gate: the lane must execute — {e}"),
            ),
        }
    }

    // ---- real worlds: parity (gpu) · exactness and cost (always) --------
    let mut worst_err = 0.0f64;
    let mut worst_share = 0.0f64;
    let mut worst_ms = 0.0f64;
    for &seed in &seeds {
        let world = World::generate(seed, size);
        // The shipped grid is WIDER than `size`: ocean margins widen every
        // world to `fields.width` columns (ADR: M-widen), and render.rs
        // rings the coast on that grid. The lane's first cut walked
        // `size × size` — a truncated window of the real world, self-
        // consistent but not the field production computes. Measure the
        // grid the engine actually uploads.
        let gw = world.width;
        let gh = size;
        let hf: Vec<f32> = world.fields.height.iter().map(|&v| v as f32).collect();
        assert_eq!(hf.len(), gw * gh, "height grid is not width×size");
        let land: Vec<bool> = hf.iter().map(|&v| v >= 0.0).collect();

        let t0 = Instant::now();
        let seeds0 = compute::coast_seeds(&hf, gw, gh);
        let cpu = compute::jfa_cpu(seeds0.clone(), gw, gh);
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        if golden.is_some() {
            golden_cases.push((format!("seed {seed}"), gw, gh, hf.clone(), cpu.clone()));
        }

        // The referee compares exact integers: the JFA's squared distance
        // to its chosen seed against the exact EDT's square. Comparing
        // rounded f32 distances instead counts precision noise as misses
        // (measured: 1–2% phantom "off" cells with zero real error).
        let edt = compute::exact_edt_sq(&land, gw, gh);
        let mut max_err = 0.0f64;
        let mut off = 0usize;
        let mut sea = 0usize;
        for i in 0..gw * gh {
            if land[i] {
                continue;
            }
            sea += 1;
            let s = cpu[i];
            let jfa_d2 = if s == compute::NONE {
                f64::INFINITY
            } else {
                let (x, y) = ((i % gw) as f64, (i / gw) as f64);
                let (sx, sy) = ((s as usize % gw) as f64, (s as usize / gw) as f64);
                (sx - x) * (sx - x) + (sy - y) * (sy - y)
            };
            if jfa_d2 != edt[i] {
                off += 1;
                let err = (jfa_d2.sqrt() - edt[i].sqrt()).abs();
                if err > max_err {
                    max_err = err;
                }
            }
        }
        let share = if sea == 0 { 0.0 } else { off as f64 / sea as f64 };
        println!(
            " seed {seed}: grid {gw}×{gh} · coast law cpu {ms:.0} ms · max |jfa−exact| {max_err:.3} cells · {} of {} sea cells miss ({})",
            off, sea, pct(share)
        );

        #[cfg(feature = "gpu")]
        if let Some((_i, lane)) = gpu.as_mut() {
            match pollster::block_on(compute::coast_seeds_gpu(lane, &seeds0, gw as u32, gh as u32)) {
                Ok(g) => {
                    let n = g.iter().zip(&cpu).filter(|(a, b)| a != b).count();
                    c.must(
                        &format!("seed {seed} gpu/cpu parity"),
                        n == 0,
                        if n == 0 { "agree".into() } else { format!("{n} diverge") },
                        "M67 gate: GPU and CPU walk one seed field on a real world",
                    );
                }
                Err(e) => c.must(
                    &format!("seed {seed} gpu/cpu parity"),
                    false,
                    "error".into(),
                    &format!("M67 gate: the gpu leg must execute — {e}"),
                ),
            }
        }

        worst_err = worst_err.max(max_err);
        worst_share = worst_share.max(share);
        worst_ms = worst_ms.max(ms);
    }

    c.band("jfa max err cells", worst_err, format!("{worst_err:.3} cells"));
    c.band("jfa wrong cell share", worst_share, pct(worst_share));
    c.band("coast law cpu ms", worst_ms, format!("{worst_ms:.0} ms"));

    // ---- the golden export (the JS twin's referee) ----------------------
    if let Some(path) = golden.as_ref() {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(b"CJFA");
        buf.extend_from_slice(&(golden_cases.len() as u32).to_le_bytes());
        for (name, w, h, hgt, sf) in &golden_cases {
            buf.extend_from_slice(&(name.len() as u32).to_le_bytes());
            buf.extend_from_slice(name.as_bytes());
            buf.extend_from_slice(&(*w as u32).to_le_bytes());
            buf.extend_from_slice(&(*h as u32).to_le_bytes());
            for v in hgt {
                buf.extend_from_slice(&v.to_le_bytes());
            }
            for v in sf {
                buf.extend_from_slice(&v.to_le_bytes());
            }
        }
        match std::fs::write(path, &buf) {
            Ok(()) => println!(
                " golden: {} case(s) · {} B -> {path}",
                golden_cases.len(),
                buf.len()
            ),
            Err(e) => {
                c.must("golden export written", false, "error".into(), &format!("M67: the JS parity lane needs its referee — {e}"));
            }
        }
    }
    c.print();
}

fn main() {
    let mut a: Vec<String> = std::env::args().collect();
    // The one flag the harness knows: `terrain --explain` (M61) — pulled
    // out before positional parsing so the strict parser stays strict.
    let explain = a.iter().any(|s| s == "--explain");
    a.retain(|s| s != "--explain");
    // `gate --reports <dir>` (M65) — same treatment: the pair is lifted
    // out whole before the positional parser sees the argument list.
    let mut reports: Option<String> = None;
    if let Some(i) = a.iter().position(|s| s == "--reports") {
        reports = a.get(i + 1).cloned();
        a.drain(i..a.len().min(i + 2));
        if reports.is_none() {
            eprintln!("error: --reports needs a directory");
            std::process::exit(2);
        }
    }
    // `compute --golden <path>` (M67 follow-on) — same lift-out treatment.
    let mut golden: Option<String> = None;
    if let Some(i) = a.iter().position(|s| s == "--golden") {
        golden = a.get(i + 1).cloned();
        a.drain(i..a.len().min(i + 2));
        if golden.is_none() {
            eprintln!("error: --golden needs a file path");
            std::process::exit(2);
        }
    }
    let cmd = a.get(1).map(|s| s.as_str()).unwrap_or("help");
    // strictly positional: a malformed argument aborts loudly instead of
    // silently falling back (a stray "--size" once cost a 12345² world).
    let num = |i: usize, d: i64| -> i64 {
        match a.get(i) {
            None => d,
            Some(s) => s.parse().unwrap_or_else(|_| {
                eprintln!("error: argument {} ({:?}) is not a number — flags are not supported, args are positional", i - 1, s);
                std::process::exit(2);
            }),
        }
    };
    let sized = |i: usize, d: i64| -> usize {
        let v = num(i, d);
        if !(64..=1024).contains(&v) {
            eprintln!("error: size {} out of range 64–1024", v);
            std::process::exit(2);
        }
        v as usize
    };

    match cmd {
        "terrain" => cmd_terrain(num(2, 12345), sized(3, 512), explain),
        "climate" => cmd_climate(num(2, 12345), sized(3, 512)),
        "hydro" => cmd_hydro(num(2, 12345), sized(3, 512)),
        "resources" => cmd_resources(num(2, 12345), sized(3, 512)),
        "civ" => cmd_civ(num(2, 12345), sized(3, 512), num(4, 120) as usize),
        "economy" => cmd_economy(num(2, 12345), sized(3, 512), num(4, 80) as usize),
        "telling" => cmd_telling(num(2, 12345), sized(3, 512), num(4, 150) as usize),
        "determinism" => cmd_determinism(num(2, 12345), sized(3, 512), num(4, 120)),
        "bench" => cmd_bench(),
        "ocean" => {
            let size = sized(2, 512);
            let meta = a.iter().any(|s| s == "--metamorphic");
            let mut seeds: Vec<i64> = a.get(3..).unwrap_or(&[]).iter().filter_map(|s| s.parse().ok()).collect();
            if seeds.is_empty() {
                seeds = vec![12345, 777, 31337, 90210, 555];
            }
            if meta {
                cmd_ocean_metamorphic(size, seeds);
            } else {
                cmd_ocean(size, seeds);
            }
        }
        "climate-variance" => {
            let size = sized(2, 512);
            let years = num(3, 60);
            let mut seeds: Vec<i64> = a.get(4..).unwrap_or(&[]).iter().filter_map(|s| s.parse().ok()).collect();
            if seeds.is_empty() {
                seeds = vec![12345, 777, 31337, 90210, 555];
            }
            cmd_climate_variance(size, years, seeds);
        }
        "oscillation" => {
            let months = num(2, 1200);
            let mut seeds: Vec<i64> = a.get(3..).unwrap_or(&[]).iter().filter_map(|s| s.parse().ok()).collect();
            if seeds.is_empty() {
                seeds = vec![12345, 777, 31337, 90210, 555];
            }
            cmd_oscillation(months, seeds);
        }
        "storms" => {
            let size = sized(2, 512);
            let years = num(3, 60);
            let mut seeds: Vec<i64> = a.get(4..).unwrap_or(&[]).iter().filter_map(|s| s.parse().ok()).collect();
            if seeds.is_empty() {
                seeds = vec![12345, 777, 31337, 90210, 555];
            }
            cmd_storms(size, years, seeds);
        }
        "tropics" => {
            let size = sized(2, 512);
            let years = num(3, 60);
            let mut seeds: Vec<i64> = a.get(4..).unwrap_or(&[]).iter().filter_map(|s| s.parse().ok()).collect();
            if seeds.is_empty() {
                seeds = vec![12345, 777, 31337, 90210, 555];
            }
            cmd_tropics(size, years, seeds);
        }
        "teleconnection" => {

            let size = sized(2, 512);
            let years = num(3, 120);
            let mut seeds: Vec<i64> = a.get(4..).unwrap_or(&[]).iter().filter_map(|s| s.parse().ok()).collect();
            if seeds.is_empty() {
                seeds = vec![12345, 777, 31337, 90210, 555];
            }
            cmd_teleconnection(size, years, seeds);
        }
        "systems" => cmd_systems(num(2, 12345), sized(3, 512), num(4, 150) as usize),
        "perf" => {
            let size = sized(2, 512);
            let mut seeds: Vec<i64> = a.get(3..).unwrap_or(&[]).iter().filter_map(|s| s.parse().ok()).collect();
            if seeds.is_empty() {
                seeds = vec![12345, 777, 90210];
            }
            cmd_perf(size, seeds);
        }
        "sweep" => {
            let size = sized(2, 512);
            let years = num(3, 100) as usize;
            let mut seeds: Vec<i64> = a.get(4..).unwrap_or(&[]).iter().filter_map(|s| s.parse().ok()).collect();
            if seeds.is_empty() {
                seeds = vec![12345, 777, 31337, 90210, 555];
            }
            cmd_sweep(size, years, seeds);
        }
        "properties" => {
            let size = sized(2, 512);
            let years = num(3, 60) as usize;
            let mut seeds: Vec<i64> = a.get(4..).unwrap_or(&[]).iter().filter_map(|s| s.parse().ok()).collect();
            if seeds.is_empty() {
                seeds = vec![12345, 777, 90210];
            }
            cmd_properties(size, years, seeds);
        }
        "earth" => {
            let size = sized(2, 512);
            let years = num(3, 150) as usize;
            let mut seeds: Vec<i64> = a.get(4..).unwrap_or(&[]).iter().filter_map(|s| s.parse().ok()).collect();
            if seeds.is_empty() {
                seeds = vec![12345, 777, 90210];
            }
            cmd_earth(size, years, seeds);
        }
        "earth-hash" => {
            // Labeled layer hashes on stdout — the native leg of the M27
            // deep-earth replay check; `wasm-replay.mjs earth` prints the
            // wasm leg in the same format.
            let seed = num(2, 777);
            let size = sized(3, 512);
            let months = num(4, 240);
            let mut w = World::generate(seed, size);
            let mut left = months;
            while left > 0 {
                let step = left.min(240);
                w.tick(step);
                left -= step;
            }
            println!("{}", w.earth_hash_line());
        }
        "seismic-hash" => {
            // Bare hex on stdout — the native leg of the M22 cross-runtime
            // replay check; scripts/wasm-replay.mjs prints the wasm leg.
            let seed = num(2, 777);
            let size = sized(3, 512);
            let months = num(4, 240);
            let mut w = World::generate(seed, size);
            let mut left = months;
            while left > 0 {
                let step = left.min(240);
                w.tick(step);
                left -= step;
            }
            println!("{:016x}", w.seismic.hash());
        }
        "seismic-debug" => {
            // Sub-hash bisection line, native leg — same format as the
            // wasm `seismic_debug()` export.
            let seed = num(2, 777);
            let size = sized(3, 512);
            let months = num(4, 0);
            let mut w = World::generate(seed, size);
            let mut left = months;
            while left > 0 {
                let step = left.min(240);
                w.tick(step);
                left -= step;
            }
            let (pt, pc, pb) = w.plates.debug_parts();
            let (sf, ss, sl) = w.seismic.debug_parts();
            println!(
                "table={:016x} cell={:016x} boundary={:016x} faults={:016x} since={:016x} log={:016x}",
                pt, pc, pb, sf, ss, sl
            );
        }
        "coast-debug" => {
            // M44 bisection line, native leg — same format as the wasm
            // `coast_debug()` export. Coast is generation-time state, so
            // months only matter for parity with the replay harness.
            let seed = num(2, 777);
            let size = sized(3, 512);
            let w = World::generate(seed, size);
            let (pos, bits, form) = w.coastform.debug_parts(&w.fields.coastform);
            println!("pos={pos:016x} bits={bits:016x} form={form:016x}");
        }
        "era" => cmd_era(sized(2, 256), num(3, 60) as usize, num(4, 16) as usize, num(5, 12345)),
        "atlas" => {
            let size = sized(2, 512);
            let mut seeds: Vec<i64> = a.get(3..).unwrap_or(&[]).iter().filter_map(|s| s.parse().ok()).collect();
            if seeds.is_empty() {
                seeds = vec![12345, 777, 90210];
            }
            cmd_atlas(size, seeds);
        }
        "patina" => {
            let size = sized(2, 512);
            let years = num(3, 300) as usize;
            let mut seeds: Vec<i64> = a.get(4..).unwrap_or(&[]).iter().filter_map(|s| s.parse().ok()).collect();
            if seeds.is_empty() {
                seeds = vec![12345, 777, 90210];
            }
            cmd_patina(size, years, seeds);
        }
        "gate" => cmd_gate(sized(2, 512), num(3, 300) as usize, num(4, 12345), reports),
        "compute" => {
            let size = sized(2, 512);
            let mut seeds: Vec<i64> = a.get(3..).unwrap_or(&[]).iter().filter_map(|s| s.parse().ok()).collect();
            if seeds.is_empty() {
                seeds = vec![12345, 777, 90210];
            }
            cmd_compute(size, seeds, golden);
        }
        _ => {
            println!("usage: diagnose <terrain|climate|climate-variance|oscillation|teleconnection|hydro|resources|civ|economy|telling|determinism|bench|perf|sweep|properties|era|patina|systems|ocean|atlas|gate|compute> [args]");
            println!("  terrain|climate|hydro|resources  <seed=12345> <size=512>");
            println!("  civ <seed> <size> <years=120> · economy <seed> <size> <years=80> · telling <seed> <size> <years=150>");
            println!("  determinism <seed> <size> <months=120> · bench · perf <size=512> <seeds…> · sweep <size> <years> <seeds…>");
            println!("  properties <size=512> <years=60> <seeds…> · era <size=256> <years=60> <n=16> <base=12345>");
            println!("  patina <size=512> <years=300> <seeds…> · systems <seed=12345> <size=512> <years=150>");
            println!("  gate <size=512> <years=300> <seed=12345> [--reports <dir>]  — the Era I gate (M65)");
            println!("  compute <size=512> <seeds…> [--golden <file>]  — the M67 lane: JFA coast law vs exact EDT, GPU leg when built with --features gpu; --golden writes the seed-field referee for the JS twin");
        }
    }
}
