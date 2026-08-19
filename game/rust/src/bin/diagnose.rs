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
//!   diagnose seismic-hash <seed> <size> <months> bare ledger hash (wasm replay leg)

use std::collections::{BTreeMap, BTreeSet};
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

fn masked<T: Copy + Into<f64>>(a: &Array2<T>, m: &Array2<bool>) -> Vec<f64> {
    a.iter().zip(m.iter()).filter(|(_, &b)| b).map(|(&v, _)| v.into()).collect()
}

fn biome_counts(w: &World) -> [usize; 11] {
    let mut c = [0usize; 11];
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
        s.push_str(&format!("s{}|{}|{}|{}|{}|{:.2}|{:?}\n", t.id, t.name, t.pop, t.people.0, t.realm.0, t.wealth, t.goods.iter().map(|g| g.name()).collect::<Vec<_>>()));
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
    // M16/ADR-0024 — the plate sketch is state: polygons, kinds, ages.
    s.push_str(&format!("P{:016x}\n", w.plates.hash()));
    // M22 — the seismic ledger is state: seams, clocks, the quake log.
    s.push_str(&format!("Q{:016x}\n", w.seismic.hash()));
    // M23 — the volcanic record is state: cones, clocks, log, ash.
    s.push_str(&format!("V{:016x}\n", w.volcanism.hash()));
    // M25 — the waterline is state: freeze phase, stand, isostasy rows.
    s.push_str(&format!("L{:016x}\n", w.sealevel.hash()));
    // M26 — the coastal landform grid is state: the classifier held still.
    s.push_str(&format!("F{:016x}\n", calliope::landform::hash(&w.landform)));
    // M28 — the LGM ice footprint is state: thickness grid, ELA rows.
    s.push_str(&format!("I{:016x}\n", w.ice.hash()));
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

    // Replay identity: the flagship seed run twice from scratch with
    // different chunkings must agree on the ledger byte-for-byte.
    let seed0 = seeds[0];
    let hash_after = |chunk: i64| -> (u64, u64) {
        let mut w = World::generate(seed0, size);
        let mut left = months;
        while left > 0 {
            let step = left.min(chunk);
            w.tick(step);
            left -= step;
        }
        (w.seismic.hash(), w.ice.hash())
    };
    let ((ha, ia), (hb, ib)) = (hash_after(240), hash_after(12));
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
        let (evs, _founded, _dep) = w.tick(12);
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
            match e.k.name() {
                "discovery" => log.strikes += 1,
                "depletion" => log.depletions += 1,
                "war" => log.wars += 1,
                "famine" => log.famines += 1,
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

fn cmd_terrain(seed: i64, size: usize) {
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

    let mut c = Checks::default();
    c.band("land fraction", land_frac, pct(land_frac));
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
        let lf = &w.landform;
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
        let purity = calliope::landform::hash(&calliope::landform::classify(hgt, sl, &w.ice)) == calliope::landform::hash(lf);
        c.must(
            "landform grid regen byte-identical",
            purity,
            if purity { "identical".into() } else { "DIVERGED".into() },
            "M26 gate: pure function of height + sea level; joins hash_state",
        );

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
    for b in 1..11u8 {
        println!("  {:<24} {:>7}  {}", gc::Biome::from_code(b), bc[b as usize], pct(share(b)));
    }
    let desert = share(gc::DESERT);
    let frozen = share(gc::TUNDRA) + share(gc::ICE);
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
        let mut counts = [0usize; 11];
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
        let dom = (1..11).max_by_key(|&i| counts[i]).unwrap_or(1);
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
    c.want("rice hugs the water", pk[2] == 0 || pk[2] < pk[1] + pk[3], format!("rice {} vs wheat+maize {}", pk[2], pk[1] + pk[3]), "paddies are the exception, not the rule");
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
    c.print();
}

// ================================================================ resources

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
    c.print();
}

// ================================================================ civ

fn cmd_civ(seed: i64, size: usize, years: usize) {
    let mut w = World::generate(seed, size);
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
    if years >= 100 {
        // by the century mark the colonies should have broken the river
        // monoculture: dry-coast harbours, cistern towns, mining camps.
        let dry = w.peoples.settlements.iter().filter(|s| !s.river).count();
        c.want("dry-country towns exist", dry >= 1, format!("{} of {}", dry, w.peoples.settlements.len()), "≥1 town beyond fresh water by the century mark");
    }
    c.must("routes exist", !w.routes.is_empty(), format!("{}", w.routes.len()), "a world without trade is broken");
    c.want("no unconnected towns", unconnected == 0, format!("{}", unconnected), "every town should reach the web of trade");
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
            let end_ok = b.end.iter().any(|(e, _)| s.name.ends_with(e));
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

fn cmd_economy(seed: i64, size: usize, years: usize) {
    let mut w = World::generate(seed, size);
    header("ECONOMY", &format!("seed {} · {}x{} · {}y", seed, w.width, size, years));

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
    for _ in 0..months {
        let (evs, _f, _d) = w.tick(1);
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

    let finite_ok = w.economy.market.iter_some().all(|(_, p)| p.is_finite()) && wealth.iter().all(|v| v.is_finite());
    let treasuries_ok = w.peoples.realms.iter().all(|r| r.treasury >= 0.0 && r.treasury.is_finite());

    let mut c = Checks::default();
    c.band("max pinned price share", max_pinned, pct(max_pinned));
    c.band("wealth gini", g, format!("{:.2}", g));
    c.must("routes exist", !w.routes.is_empty(), format!("{}", w.routes.len()), "the web of trade must hold");
    c.want("no unconnected towns", unconnected == 0, format!("{}", unconnected), "every town trades");
    c.want("harbours exist", ports >= 1, format!("{}", ports), "coastal trade should produce ports");
    c.must("prices finite", finite_ok, if finite_ok { "yes".into() } else { "NO".into() }, "no NaN in the market");
    c.must("treasuries sane", treasuries_ok, if treasuries_ok { "yes".into() } else { "NO".into() }, "≥0 and finite");
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
        });
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
        if flags.is_empty() {
            flags.push('·');
        }

        println!("{:>7} {:>6.1} {:>6.1} {:>6.1} {:>5.1} {:>5} {:>4} {:>2}→{:<2} {:>9} {:>6.2} {:>5} {:>4} {:>4} {:>4} {:>4} {:>4} {:>5.1} {:>6}  {}", seed, 100.0 * land_frac, 100.0 * desert, 100.0 * forest, 100.0 * mtn, li.n, w.deposits.len(), setts0, w.peoples.settlements.len(), pop1, growth, era, arts, log.strikes, log.camps, log.wars, w.routes.len(), evyr, gen_ms, flags);

        rows.push(Row { seed, land: land_frac, desert, forest, mtn, camps: log.camps, strikes: log.strikes, famines: log.famines, zipf, growth, pace, era, evyr, flags, sundered: log.peoples_rose });
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
    println!("flags: B border-land · P placeholder-leak · R no-routes · G stagnant · S no-strikes · U unconnected");

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
    c.must("all seeds clean of hard flags", clean == rows.len(), if worst_flags.is_empty() { "all clean".into() } else { worst_flags.join(" ") }, "no B/P/R/G/S/U flags on any seed");
    c.want("strikes on every seed", strike_seeds == rows.len(), format!("{}/{}", strike_seeds, rows.len()), "prospecting fires everywhere");
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

        // ---- P3: pack v2 round-trips — stable bytes, honest layout,
        // valid crc, quantization inside ε, territory RLE exact (E3.3-E3.6)
        let p1 = w.pack();
        let p2 = w.pack();
        c.must(&format!("pack is stable ({})", seed), p1 == p2,
            format!("{} B", p1.len()), "M8.1: same world ⇒ same bytes");
        let hlen = u32::from_le_bytes([p1[0], p1[1], p1[2], p1[3]]) as usize;
        let hdr: serde_json::Value = serde_json::from_slice(&p1[4..4 + hlen]).unwrap();
        let base = 4 + hlen;
        let crc = calliope::util::crc32(&p1[base..]);
        c.must(&format!("pack v2 + crc32 ({})", seed),
            hdr["pack"].as_u64() == Some(2) && hdr["crc32"].as_u64() == Some(crc as u64),
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
            ok_layout &= off == expected_off && nb == shape[0] * shape[1] * cell;
            expected_off = off + nb;
            total += nb;
        }
        ok_layout &= p1.len() == 4 + hlen + total;
        c.must(&format!("unpack layout sound ({})", seed), ok_layout,
            format!("{} arrays", entries.len()), "M8.1: offsets contiguous, sizes exact");

        // decode every section exactly the way the client does and compare
        let f32_grid = |name: &str| -> &ndarray::Array2<f32> {
            match name {
                "height" => &w.fields.height, "tmean" => &w.fields.tmean, "tamp" => &w.fields.tamp,
                "precip" => &w.fields.precip, "pamp" => &w.fields.pamp, "discharge" => &w.fields.discharge,
                "flow_amp" => &w.fields.flow_amp, "fertility" => &w.fields.fertility,
                other => unreachable!("unknown f32 field {other}"),
            }
        };
        let u8_grid = |name: &str| -> &ndarray::Array2<u8> {
            match name {
                "biomes" => &w.fields.biomes, "crops" => &w.fields.crops,
                "strahler" => &w.fields.strahler, "flags" => &w.fields.flags,
                "rock" => &w.fields.rock,
                other => unreachable!("unknown u8 field {other}"),
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
                let qs: Vec<u16> = p1[off..off + nb].chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
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
                        ok_data &= p1[off..off + nb].iter().zip(grid.iter()).all(|(&a, &b)| a == b);
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
            format!("worst {:.3}·scale", worst_q), "E3.4: u16 wire loses ≤ half a step");

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
        println!();
    }

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
                + jsd(&rows[i].lm_hist, &rows[j].lm_hist)) / 5.0;
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

fn main() {
    let a: Vec<String> = std::env::args().collect();
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
        "terrain" => cmd_terrain(num(2, 12345), sized(3, 512)),
        "climate" => cmd_climate(num(2, 12345), sized(3, 512)),
        "hydro" => cmd_hydro(num(2, 12345), sized(3, 512)),
        "resources" => cmd_resources(num(2, 12345), sized(3, 512)),
        "civ" => cmd_civ(num(2, 12345), sized(3, 512), num(4, 120) as usize),
        "economy" => cmd_economy(num(2, 12345), sized(3, 512), num(4, 80) as usize),
        "telling" => cmd_telling(num(2, 12345), sized(3, 512), num(4, 150) as usize),
        "determinism" => cmd_determinism(num(2, 12345), sized(3, 512), num(4, 120)),
        "bench" => cmd_bench(),
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
        "era" => cmd_era(sized(2, 256), num(3, 60) as usize, num(4, 16) as usize, num(5, 12345)),
        "patina" => {
            let size = sized(2, 512);
            let years = num(3, 300) as usize;
            let mut seeds: Vec<i64> = a.get(4..).unwrap_or(&[]).iter().filter_map(|s| s.parse().ok()).collect();
            if seeds.is_empty() {
                seeds = vec![12345, 777, 90210];
            }
            cmd_patina(size, years, seeds);
        }
        _ => {
            println!("usage: diagnose <terrain|climate|hydro|resources|civ|economy|telling|determinism|bench|perf|sweep|properties|era|patina|systems> [args]");
            println!("  terrain|climate|hydro|resources  <seed=12345> <size=512>");
            println!("  civ <seed> <size> <years=120> · economy <seed> <size> <years=80> · telling <seed> <size> <years=150>");
            println!("  determinism <seed> <size> <months=120> · bench · perf <size=512> <seeds…> · sweep <size> <years> <seeds…>");
            println!("  properties <size=512> <years=60> <seeds…> · era <size=256> <years=60> <n=16> <base=12345>");
            println!("  patina <size=512> <years=300> <seeds…> · systems <seed=12345> <size=512> <years=150>");
        }
    }
}
