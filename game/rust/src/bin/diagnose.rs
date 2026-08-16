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
//!   diagnose determinism <seed> <size> <months> same seed => same world, always
//!   diagnose bench                              generation + tick throughput
//!   diagnose sweep       <size> <years> <seeds> cross-seed robustness table

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use ndarray::Array2;

use calliope::constants as gc;
use calliope::economy;
use calliope::ndimage;
use calliope::resources;
use calliope::society;
use calliope::util::quantile;
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
    w.height.mapv(|h| h >= 0.0)
}

fn masked(a: &Array2<f64>, m: &Array2<bool>) -> Vec<f64> {
    a.iter().zip(m.iter()).filter(|(_, &b)| b).map(|(&v, _)| v).collect()
}

fn biome_counts(w: &World) -> [usize; 11] {
    let mut c = [0usize; 11];
    for &b in w.biomes.iter() {
        c[b as usize] += 1;
    }
    c
}

fn border_land(w: &World) -> usize {
    let (h, ww) = w.height.dim();
    let mut n = 0usize;
    for x in 0..ww {
        n += (w.height[[0, x]] >= 0.0) as usize + (w.height[[h - 1, x]] >= 0.0) as usize;
    }
    for y in 0..h {
        n += (w.height[[y, 0]] >= 0.0) as usize + (w.height[[y, ww - 1]] >= 0.0) as usize;
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
    for t in &w.settlements {
        s.push_str(&format!("{}|{}|{}|{:.2}\n", t.id, t.name, t.pop, t.wealth));
    }
    fnv(s.as_bytes())
}

/// Hash the true world state — arrays and entities, NOT the packed payload
/// (whose header embeds wall-clock stage timings and thus always differs).
fn hash_state(w: &World) -> u64 {
    let mut bytes: Vec<u8> = Vec::new();
    for &v in w.height.iter() {
        bytes.extend_from_slice(&v.to_bits().to_le_bytes());
    }
    for &b in w.biomes.iter() {
        bytes.push(b);
    }
    for (&r, &l) in w.rivers.iter().zip(w.lakes.iter()) {
        bytes.push((r as u8) | ((l as u8) << 1));
    }
    let mut s = String::new();
    for d in &w.deposits {
        s.push_str(&format!("d{}|{}|{}|{:.2}|{}|{:.0}\n", d.r, d.x, d.y, d.rich, d.known, d.left));
    }
    for t in &w.settlements {
        s.push_str(&format!("s{}|{}|{}|{}|{:.2}|{:?}\n", t.id, t.name, t.pop, t.culture, t.wealth, t.goods));
    }
    for f in &w.features {
        s.push_str(&format!("f{}|{}|{}|{}\n", f.t, f.name, f.x, f.y));
    }
    for r in &w.routes {
        s.push_str(&format!("r{}|{}|{:.2}|{:.3}\n", r.a, r.b, r.cost, r.sea));
    }
    for (g, p) in &w.market.prices {
        s.push_str(&format!("m{}|{:.2}\n", g, p));
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

// ================================================================ run log

#[derive(Default)]
struct RunLog {
    rows: Vec<(usize, i64, usize, usize, f64, f64, usize, usize, usize)>,
    census: BTreeMap<String, usize>,
    camps: usize,
    strikes: usize,
    depletions: usize,
    wars: usize,
    placeholders: usize,
    empties: usize,
    arc: Vec<(i64, String)>,
    max_gap: i64,
    total_events: usize,
}

/// Advance `years` in 12-month ticks, logging everything worth judging.
fn run_years(w: &mut World, years: usize) -> RunLog {
    let mut log = RunLog::default();
    let mut last_m = w.month;
    for yr in 1..=years {
        let (evs, _founded, _dep) = w.tick(12);
        for e in &evs {
            *log.census.entry(e.k.clone()).or_default() += 1;
            if e.text.contains('{') || e.text.contains('}') {
                log.placeholders += 1;
            }
            if e.text.trim().is_empty() {
                log.empties += 1;
            }
            if e.text.contains("mining camp") {
                log.camps += 1;
            }
            match e.k.as_str() {
                "discovery" => log.strikes += 1,
                "depletion" => log.depletions += 1,
                "war" => log.wars += 1,
                "tech" | "society" => log.arc.push((e.m, e.text.clone())),
                _ => {}
            }
            log.max_gap = log.max_gap.max(e.m - last_m);
            last_m = e.m;
        }
        log.total_events += evs.len();
        let pop: i64 = w.settlements.iter().map(|s| s.pop).sum();
        let wealth: f64 = w.settlements.iter().map(|s| s.wealth).sum();
        let treasury: f64 = w.societies.iter().map(|s| s.treasury).sum();
        let techs: usize = w.societies.iter().map(|s| s.techs.len()).sum();
        let known = w.deposits.iter().filter(|d| d.known).count();
        log.rows.push((yr, pop, w.settlements.len(), w.routes.len(), wealth, treasury, techs, known, evs.len()));
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
    let hs = masked(&w.height, &land);
    let depths: Vec<f64> = w.height.iter().filter(|&&h| h < 0.0).cloned().collect();
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

    let arch = w.features.iter().filter(|f| f.t == "archipelago").count();
    let named_isles = w.features.iter().filter(|f| f.t == "island").count();
    let ranges = w.features.iter().filter(|f| f.t == "range").count();
    println!("named: {} archipelagos · {} islands · {} mountain ranges", arch, named_isles, ranges);
    let bl = border_land(&w);

    let mut c = Checks::default();
    c.range("land fraction", land_frac, pct(land_frac), (0.22, 0.40), (0.15, 0.52), "sweet 22–40% · hard 15–52%");
    c.must("border land cells", bl == 0, format!("{}", bl), "must be 0 — no clipped landmasses");
    c.range("largest landmass share of land", largest / land_n.max(1.0), pct(largest / land_n.max(1.0)), (0.25, 0.85), (0.10, 0.93), "sweet 25–85% · hard 10–93%");
    c.range("landmass count", li.n as f64, format!("{}", li.n), (15.0, 400.0), (6.0, 2000.0), "sweet 15–400 · hard 6–2000");
    c.range("small isles+islets", (islands + islets) as f64, format!("{}", islands + islets), (10.0, 380.0), (3.0, 1900.0), "sweet 10–380 · hard 3–1900");
    c.range("mountain share of land (h>0.5)", mtn, pct(mtn), (0.02, 0.14), (0.005, 0.22), "sweet 2–14% · hard 0.5–22%");
    c.range("coastline crenulation", coast_ratio, format!("{:.3}", coast_ratio), (0.02, 0.30), (0.012, 0.50), "sweet 0.02–0.30 at 4 km cells");
    c.want("archipelagos named", arch >= 1, format!("{}", arch), "≥1 — island clusters should get names");
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
        println!("  {:<24} {:>7}  {}", gc::PRETTY_BIOMES[b as usize], bc[b as usize], pct(share(b)));
    }
    let desert = share(gc::DESERT);
    let frozen = share(gc::TUNDRA) + share(gc::ICE);
    let forest = share(gc::WOODLAND) + share(gc::SEASONAL_RAIN_FOREST) + share(gc::TEMPERATE_RAIN_FOREST) + share(gc::BOREAL_FOREST) + share(gc::TROPICAL_RAIN_FOREST);
    let open = share(gc::GRASSLAND) + share(gc::SAVANNA);

    let ts = masked(&w.tmean, &land);
    let ps = masked(&w.precip, &land);
    let amps: Vec<f64> = w.tamp.iter().zip(land.iter()).filter(|(_, &b)| b).map(|(&v, _)| v.abs()).collect();
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
                tsum += w.tmean[[y, x]];
                psum += w.precip[[y, x]];
                counts[w.biomes[[y, x]] as usize] += 1;
            }
        }
        let label = format!("{:.0}°–{:.0}°", latitude(y0, rows), latitude(y1.saturating_sub(1), rows));
        if n == 0 {
            println!("  {:<12} {:>6} {:>8} {:>9}  —", label, 0, "—", "—");
            continue;
        }
        let dom = (1..11).max_by_key(|&i| counts[i]).unwrap_or(1);
        println!("  {:<12} {:>6} {:>7.1}C {:>7.0}mm  {}", label, n, tsum / n as f64, psum / n as f64, gc::PRETTY_BIOMES[dom]);
    }

    let mut c = Checks::default();
    c.range("desert share of land", desert, pct(desert), (0.12, 0.28), (0.06, 0.38), "sweet 12–28% · hard 6–38%");
    c.range("tundra+ice share of land", frozen, pct(frozen), (0.05, 0.30), (0.01, 0.45), "sweet 5–30% · hard 1–45%");
    c.range("forest share of land", forest, pct(forest), (0.25, 0.60), (0.15, 0.75), "sweet 25–60% · hard 15–75%");
    c.range("grass+savanna share of land", open, pct(open), (0.10, 0.45), (0.04, 0.60), "sweet 10–45% · hard 4–60%");
    c.range("land mean temperature", t_mean, format!("{:.1}°C", t_mean), (5.0, 20.0), (-2.0, 28.0), "sweet 5–20°C · hard -2–28°C");
    c.range("land mean precipitation", p_mean, format!("{:.0}mm", p_mean), (500.0, 1500.0), (250.0, 2400.0), "sweet 500–1500 · hard 250–2400");
    c.range("mean seasonal swing", a_mean, format!("{:.1}°C", a_mean), (4.0, 14.0), (2.0, 20.0), "sweet 4–14°C · hard 2–20°C");
    c.print();
}

// ================================================================ hydro

fn cmd_hydro(seed: i64, size: usize) {
    let w = World::generate(seed, size);
    header("HYDROLOGY", &format!("seed {} · {}x{}", seed, w.width, size));

    let land = land_mask(&w);
    let land_n = land.iter().filter(|&&b| b).count() as f64;
    let river_n = w.rivers.iter().filter(|&&r| r).count() as f64;
    let lake_n = w.lakes.iter().filter(|&&l| l).count() as f64;
    println!("river cells: {} ({} of land) · lake cells: {} ({} of land)", river_n as usize, pct(river_n / land_n.max(1.0)), lake_n as usize, pct(lake_n / land_n.max(1.0)));

    let li = ndimage::label(&w.rivers, true);
    let systems = li.areas.iter().filter(|&&a| a >= 12.0).count();
    let longest = li.areas.iter().cloned().fold(0.0f64, f64::max);
    println!("river systems (≥12 cells): {} · longest network {} cells ≈ {:.0} km", systems, longest as usize, longest * gc::KM_PER_CELL);

    let dis: Vec<f64> = w.discharge.iter().zip(w.rivers.iter()).filter(|(_, &r)| r).map(|(&d, _)| d).collect();
    if !dis.is_empty() {
        println!("discharge on rivers: p50 {:.1} · p90 {:.1} · p99 {:.1} · max {:.1}", quantile(&dis, 0.5), quantile(&dis, 0.9), quantile(&dis, 0.99), quantile(&dis, 1.0));
    }
    let deltas = w.features.iter().filter(|f| f.t == "delta").count();
    let marshes = w.features.iter().filter(|f| f.t == "marsh").count();
    println!("named: {} deltas · {} marshes", deltas, marshes);
    for f in w.features.iter().filter(|f| f.t == "delta").take(6) {
        println!("  delta: {} @({},{})", f.name, f.x, f.y);
    }
    let river_towns = w.settlements.iter().filter(|s| s.river).count();
    println!("river towns: {} of {}", river_towns, w.settlements.len());

    let finite = dis.iter().all(|d| d.is_finite());
    let mut c = Checks::default();
    c.range("river share of land", river_n / land_n.max(1.0), pct(river_n / land_n.max(1.0)), (0.008, 0.05), (0.003, 0.10), "sweet 0.8–5% · hard 0.3–10%");
    c.range("lake share of land", lake_n / land_n.max(1.0), pct(lake_n / land_n.max(1.0)), (0.0, 0.03), (0.0, 0.08), "sweet 0–3% · hard 0–8%");
    c.range("river systems", systems as f64, format!("{}", systems), (8.0, 400.0), (3.0, 2000.0), "sweet 8–400 · hard 3–2000");
    c.want("named deltas", deltas >= 1, format!("{}", deltas), "≥1 — great river mouths get names");
    c.must("discharge finite", finite, if finite { "yes".into() } else { "NO".into() }, "no NaN/inf in flow accumulation");
    c.range("river-town share", river_towns as f64 / w.settlements.len().max(1) as f64, pct(river_towns as f64 / w.settlements.len().max(1) as f64), (0.2, 0.95), (0.05, 1.0), "sweet 20–95% — fresh water pulls towns");
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
            missing.push(kind);
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
                w.settlements
                    .iter()
                    .map(|s| (((d.x - s.x).pow(2) + (d.y - s.y).pow(2)) as f64).sqrt())
                    .fold(f64::INFINITY, f64::min)
            })
            .collect();
        dists.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let med = dists[dists.len() / 2];
        let is_mineral = matches!(kind, "stone" | "coal" | "copper" | "iron" | "silver" | "gold" | "mithril");
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

    let gold = w.deposits.iter().any(|d| d.r == "gold");
    let mithril = w.deposits.iter().any(|d| d.r == "mithril");
    let essential_missing = missing.iter().filter(|k| !matches!(**k, "mithril" | "bananas")).count();

    let mut c = Checks::default();
    c.range("deposits per 1000 land cells", per_1000, format!("{:.2}", per_1000), (1.0, 6.0), (0.5, 12.0), "sweet 1–6 · hard 0.5–12");
    c.range("mineral hidden share at dawn", hidden_share, pct(hidden_share), (0.45, 0.85), (0.25, 0.95), "sweet 45–85% — leave an age of prospectors");
    c.want("essential kinds all present", essential_missing == 0, format!("{} missing", essential_missing), "everything except mithril/bananas should place");
    c.want("gold placed", gold, if gold { "yes".into() } else { "no".into() }, "a world without gold has a dull late game");
    c.want("mithril placed", mithril, if mithril { "yes".into() } else { "no".into() }, "the legendary seam should exist somewhere");
    c.print();
}

// ================================================================ civ

fn cmd_civ(seed: i64, size: usize, years: usize) {
    let mut w = World::generate(seed, size);
    header("CIVILIZATION", &format!("seed {} · {}x{} · {}y", seed, w.width, size, years));
    println!("world \"{}\" · {} cultures · {} settlements at dawn", w.world_name, w.cultures.len(), w.settlements.len());

    let pop0: i64 = w.settlements.iter().map(|s| s.pop).sum();
    let setts0 = w.settlements.len();
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
    for (soc, cu) in w.societies.iter().zip(w.cultures.iter()) {
        println!("  {:<22} {:<10} {:<14} {:>2} arts · treasury {:>8.0} · lore {:>6.0}", cu.people, society::POLITIES[soc.polity], society::ERAS[soc.era], soc.techs.len(), soc.treasury, soc.knowledge);
    }

    let pop1: i64 = w.settlements.iter().map(|s| s.pop).sum();
    let growth = pop1 as f64 / pop0.max(1) as f64;
    let names: BTreeSet<&str> = w.settlements.iter().map(|s| s.name.as_str()).collect();
    let max_era = w.societies.iter().map(|s| s.era).max().unwrap_or(0);
    let techs_total: usize = w.societies.iter().map(|s| s.techs.len()).sum();
    let ev_per_year = log.total_events as f64 / years.max(1) as f64;
    let unconnected = w.settlements.iter().filter(|s| s.connections == 0).count();
    let finite_ok = w.settlements.iter().all(|s| s.wealth.is_finite() && s.pop >= 0) && w.market.prices.values().all(|p| p.is_finite());

    let mut c = Checks::default();
    c.range("population growth ×", growth, format!("{:.2}×", growth), (2.0, 500.0), (1.05, 2000.0), "sweet 2–500× — dawn towns are tiny");
    if years >= 100 {
        // pacing: the world should still be becoming in its second half,
        // not sitting on a saturated plateau for a century.
        let half_pop = log.rows[years / 2 - 1].1 as f64;
        let pace = half_pop / pop1.max(1) as f64;
        c.want("still growing at half-run", pace <= 0.92, format!("{:.0}% of final", 100.0 * pace), "pop at half-run ≤92% of final");
    }
    c.want("settlements grew", w.settlements.len() >= setts0, format!("{}→{}", setts0, w.settlements.len()), "colonies should outnumber the dawn towns");
    c.must("routes exist", !w.routes.is_empty(), format!("{}", w.routes.len()), "a world without trade is broken");
    c.want("no unconnected towns", unconnected == 0, format!("{}", unconnected), "every town should reach the web of trade");
    c.must("no template placeholders", log.placeholders == 0, format!("{}", log.placeholders), "no {P}/{S} may leak into chronicle text");
    c.must("no empty event texts", log.empties == 0, format!("{}", log.empties), "every event tells its story");
    c.must("settlement names unique", names.len() == w.settlements.len(), format!("{} names / {} towns", names.len(), w.settlements.len()), "the taken-set must hold");
    c.range("events per year", ev_per_year, format!("{:.1}", ev_per_year), (2.0, 40.0), (0.5, 100.0), "sweet 2–40 · hard 0.5–100");
    c.want("no long silences", log.max_gap <= 36, format!("{} mo", log.max_gap), "≤36 months between chronicle entries");
    if years >= 80 {
        c.want("strikes happened", log.strikes >= 1, format!("{}", log.strikes), "the age of prospectors must actually happen");
        c.want("era advanced past Stone", max_era >= 1, society::ERAS[max_era].to_string(), "≥ Age of Bronze by now");
        c.want("arts accumulate", techs_total >= 3 * w.societies.len(), format!("{} arts / {} peoples", techs_total, w.societies.len()), "≥3 arts per people");
    }
    if years >= 140 {
        c.want("era reached Iron", max_era >= 2, society::ERAS[max_era].to_string(), "≥ Age of Iron by now");
    }
    c.must("numbers stay finite", finite_ok, if finite_ok { "yes".into() } else { "NO".into() }, "no NaN pops, wealth or prices");
    c.print();
}

// ================================================================ economy

fn cmd_economy(seed: i64, size: usize, years: usize) {
    let mut w = World::generate(seed, size);
    header("ECONOMY", &format!("seed {} · {}x{} · {}y", seed, w.width, size, years));

    const TRACKED: [&str; 10] = ["grain", "fish", "timber", "stone", "coal", "copper", "iron", "silver", "gold", "mithril"];
    let mut series: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
    let mut strikes = 0usize;
    let mut depletions = 0usize;
    let mut trade_events = 0usize;
    let months = years * 12;
    for _ in 0..months {
        let (evs, _f, _d) = w.tick(1);
        for e in &evs {
            match e.k.as_str() {
                "discovery" => strikes += 1,
                "depletion" => depletions += 1,
                "trade" => trade_events += 1,
                _ => {}
            }
        }
        for g in TRACKED {
            if let Some(&p) = w.market.prices.get(g) {
                series.entry(g).or_default().push(p);
            }
        }
    }

    println!("{:<9} {:>6} {:>7} {:>7} {:>7} {:>9} {:>8}", "good", "base", "mean", "min", "max", "mean/base", "pinned");
    let mut max_pinned = 0.0f64;
    for (g, s) in &series {
        let base = economy::base_value(g);
        let mean = s.iter().sum::<f64>() / s.len() as f64;
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
    let wealth: Vec<f64> = w.settlements.iter().map(|s| s.wealth).collect();
    let g = gini(&wealth);
    let total_w: f64 = wealth.iter().sum();
    println!("wealth: total {:.0} · gini {:.2}", total_w, g);
    let mut by_wealth: Vec<&calliope::settlements::Settlement> = w.settlements.iter().collect();
    by_wealth.sort_by(|a, b| b.wealth.partial_cmp(&a.wealth).unwrap());
    println!("richest towns:");
    for s in by_wealth.iter().take(5) {
        println!("  {:<20} pop {:>6} · wealth {:>8.0} · exports {} {}", s.name, s.pop, s.wealth, s.exports.as_deref().unwrap_or("—"), if s.port { "· harbour" } else { "" });
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
    let ports = w.settlements.iter().filter(|s| s.port).count();
    let unconnected = w.settlements.iter().filter(|s| s.connections == 0).count();
    println!();
    println!("routes: {} ({} sea / {} mixed / {} land) · mean cost {:.1} · mean length {:.0} km", w.routes.len(), sea_r, mixed_r, land_r, mean_cost, mean_len * gc::KM_PER_CELL);
    println!("harbours: {} · unconnected towns: {}", ports, unconnected);
    println!("treasuries:");
    for (soc, cu) in w.societies.iter().zip(w.cultures.iter()) {
        println!("  {:<22} {:>9.0}", cu.people, soc.treasury);
    }

    let finite_ok = w.market.prices.values().all(|p| p.is_finite()) && wealth.iter().all(|v| v.is_finite());
    let treasuries_ok = w.societies.iter().all(|s| s.treasury >= 0.0 && s.treasury.is_finite());

    let mut c = Checks::default();
    c.range("max pinned price share", max_pinned, pct(max_pinned), (0.0, 0.25), (0.0, 0.55), "sweet ≤25% · hard ≤55%");
    c.range("wealth gini", g, format!("{:.2}", g), (0.20, 0.80), (0.05, 0.95), "sweet 0.20–0.80 — some inequality, no monopoly");
    c.must("routes exist", !w.routes.is_empty(), format!("{}", w.routes.len()), "the web of trade must hold");
    c.want("no unconnected towns", unconnected == 0, format!("{}", unconnected), "every town trades");
    c.want("harbours exist", ports >= 1, format!("{}", ports), "coastal trade should produce ports");
    c.must("prices finite", finite_ok, if finite_ok { "yes".into() } else { "NO".into() }, "no NaN in the market");
    c.must("treasuries sane", treasuries_ok, if treasuries_ok { "yes".into() } else { "NO".into() }, "≥0 and finite");
    if years >= 60 {
        c.want("strikes moved markets", strikes >= 1, format!("{}", strikes), "discovery shocks should fire");
    }
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
    header("BENCH", "native release");
    let sizes = [320usize, 512, 640, 768];
    println!("{:>5} {:>9} {:>11} {:>9}  stage breakdown (ms)", "size", "gen ms", "pack bytes", "cells");
    let mut gen512 = 0.0f64;
    for &s in &sizes {
        let t = Instant::now();
        let w = World::generate(4242, s);
        let ms = t.elapsed().as_millis() as f64;
        if s == 512 {
            gen512 = ms;
        }
        let packed = w.pack();
        let mut stages = String::new();
        if let Some(arr) = w.meta()["timings"].as_array() {
            let parts: Vec<String> = arr
                .iter()
                .filter_map(|e| {
                    let name = e[0].as_str()?;
                    let v = e[1].as_f64()?;
                    if name == "total" {
                        None
                    } else {
                        Some(format!("{} {:.0}", name, v))
                    }
                })
                .collect();
            stages = parts.join(" · ");
        }
        println!("{:>5} {:>9.0} {:>11} {:>9}  {}", s, ms, packed.len(), w.height.len(), stages);
    }

    // tick throughput at 512
    let mut w = World::generate(4242, 512);
    let t = Instant::now();
    let mut left = 240i64;
    while left > 0 {
        let step = left.min(240);
        w.tick(step);
        left -= step;
    }
    let tick_ms = t.elapsed().as_millis() as f64;
    let rate = 240.0 / (tick_ms / 1000.0);
    println!();
    println!("tick throughput @512 with {} towns: 240 months in {:.0} ms = {:.0} months/s", w.settlements.len(), tick_ms, rate);

    let mut c = Checks::default();
    c.range("512 generation time", gen512, format!("{:.0} ms", gen512), (0.0, 3000.0), (0.0, 8000.0), "sweet ≤3s · hard ≤8s (wasm ≈ 2× native)");
    c.range("tick rate", rate, format!("{:.0} mo/s", rate), (100.0, f64::INFINITY), (25.0, f64::INFINITY), "sweet ≥100 mo/s · hard ≥25");
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
        growth: f64,
        pace: f64,
        era: usize,
        evyr: f64,
        flags: String,
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
        let hs = masked(&w.height, &land);
        let mtn = hs.iter().filter(|&&h| h > 0.5).count() as f64 / land_n.max(1.0);
        let li = ndimage::label(&land, true);
        let bl = border_land(&w);
        let setts0 = w.settlements.len();
        let pop0: i64 = w.settlements.iter().map(|s| s.pop).sum();

        let log = run_years(&mut w, years);

        let pop1: i64 = w.settlements.iter().map(|s| s.pop).sum();
        let growth = pop1 as f64 / pop0.max(1) as f64;
        let pace = if years >= 100 {
            log.rows[years / 2 - 1].1 as f64 / pop1.max(1) as f64
        } else {
            0.0
        };
        let era = w.societies.iter().map(|s| s.era).max().unwrap_or(0);
        let arts: usize = w.societies.iter().map(|s| s.techs.len()).sum();
        let evyr = log.total_events as f64 / years.max(1) as f64;
        let unconnected = w.settlements.iter().filter(|s| s.connections == 0).count();

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

        println!("{:>7} {:>6.1} {:>6.1} {:>6.1} {:>5.1} {:>5} {:>4} {:>2}→{:<2} {:>9} {:>6.2} {:>5} {:>4} {:>4} {:>4} {:>4} {:>4} {:>5.1} {:>6}  {}", seed, 100.0 * land_frac, 100.0 * desert, 100.0 * forest, 100.0 * mtn, li.n, w.deposits.len(), setts0, w.settlements.len(), pop1, growth, era, arts, log.strikes, log.camps, log.wars, w.routes.len(), evyr, gen_ms, flags);

        rows.push(Row { seed, land: land_frac, desert, forest, mtn, camps: log.camps, strikes: log.strikes, growth, pace, era, evyr, flags });
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
    c.range("mean land fraction", m_land, pct(m_land), (0.22, 0.40), (0.15, 0.52), "sweet 22–40%");
    c.range("mean desert share", m_des, pct(m_des), (0.12, 0.28), (0.06, 0.38), "sweet 12–28%");
    c.range("mean forest share", m_for, pct(m_for), (0.25, 0.60), (0.15, 0.75), "sweet 25–60%");
    c.range("mean mountain share", m_mtn, pct(m_mtn), (0.02, 0.14), (0.005, 0.22), "sweet 2–14%");
    c.range("mean growth", m_grw, format!("{:.2}×", m_grw), (2.0, 500.0), (1.05, 2000.0), "sweet 2–500×");
    c.range("mean events/year", m_evy, format!("{:.1}", m_evy), (2.0, 40.0), (0.5, 100.0), "sweet 2–40");
    c.must("all seeds clean of hard flags", clean == rows.len(), if worst_flags.is_empty() { "all clean".into() } else { worst_flags.join(" ") }, "no B/P/R/G/S/U flags on any seed");
    c.want("strikes on every seed", strike_seeds == rows.len(), format!("{}/{}", strike_seeds, rows.len()), "prospecting fires everywhere");
    if years >= 80 {
        c.want("mining camps emerge (≥60% of seeds)", camp_seeds * 10 >= rows.len() * 6, format!("{}/{}", camp_seeds, rows.len()), "ore pull creates colonies");
        c.want("Iron Age reached (≥50% of seeds)", iron_seeds * 2 >= rows.len(), format!("{}/{}", iron_seeds, rows.len()), "history should not stall in bronze");
    }
    if years >= 100 {
        let pacing = rows.iter().filter(|r| r.pace <= 0.92).count();
        c.want("worlds still growing at half-run (≥60%)", pacing * 10 >= rows.len() * 6, format!("{}/{}", pacing, rows.len()), "no century-long plateaus");
    }
    c.print();
}

// ================================================================ main

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
        "determinism" => cmd_determinism(num(2, 12345), sized(3, 512), num(4, 120)),
        "bench" => cmd_bench(),
        "sweep" => {
            let size = sized(2, 512);
            let years = num(3, 100) as usize;
            let mut seeds: Vec<i64> = a.get(4..).unwrap_or(&[]).iter().filter_map(|s| s.parse().ok()).collect();
            if seeds.is_empty() {
                seeds = vec![12345, 777, 31337, 90210, 555];
            }
            cmd_sweep(size, years, seeds);
        }
        _ => {
            println!("usage: diagnose <terrain|climate|hydro|resources|civ|economy|determinism|bench|sweep> [args]");
            println!("  terrain|climate|hydro|resources  <seed=12345> <size=512>");
            println!("  civ <seed> <size> <years=120> · economy <seed> <size> <years=80>");
            println!("  determinism <seed> <size> <months=120> · bench · sweep <size> <years> <seeds…>");
        }
    }
}
