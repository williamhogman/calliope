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

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use ndarray::Array2;

use calliope::constants as gc;
use calliope::economy;
use calliope::hydrology;
use calliope::naming;
use calliope::ndimage;
use calliope::resources;
use calliope::society;
use calliope::telling;
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
    for &b in w.crops.iter() {
        bytes.push(b);
    }
    for (((&r, &l), &s), &sw) in w
        .rivers
        .iter()
        .zip(w.lakes.iter())
        .zip(w.salt.iter())
        .zip(w.seasonal.iter())
    {
        bytes.push((r as u8) | ((l as u8) << 1) | ((s as u8) << 2) | ((sw as u8) << 3));
    }
    for &o in w.strahler.iter() {
        bytes.push(o);
    }
    for &v in w.pamp.iter().chain(w.flow_amp.iter()) {
        bytes.extend_from_slice(&v.to_bits().to_le_bytes());
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
}

/// Advance `years` in 12-month ticks, logging everything worth judging.
fn run_years(w: &mut World, years: usize) -> RunLog {
    let mut log = RunLog::default();
    let mut last_m = w.month;
    let god_names: Vec<String> = w
        .cultures
        .iter()
        .flat_map(|c| c.pantheon.iter().map(|g| g.name.clone()))
        .collect();
    let alive_count = |w: &World| -> usize {
        (0..w.cultures.len())
            .filter(|&c| w.settlements.iter().any(|s| s.culture == c))
            .count()
    };
    let mut owners: Vec<usize> = w.settlements.iter().map(|s| s.culture).collect();
    let mut n_cultures = w.cultures.len();
    for yr in 1..=years {
        let (evs, _founded, _dep) = w.tick(12);
        for e in &evs {
            *log.census.entry(e.k.clone()).or_default() += 1;
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
            match e.k.as_str() {
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
        for (i, s) in w.settlements.iter().enumerate() {
            if i < owners.len() && owners[i] != s.culture {
                log.transfers += 1;
            }
        }
        owners = w.settlements.iter().map(|s| s.culture).collect();
        if w.cultures.len() > n_cultures {
            log.rebellions += w.cultures.len() - n_cultures;
            n_cultures = w.cultures.len();
        }
        if w.politics.wars.iter().any(|war| !war.allies_a.is_empty() || !war.allies_b.is_empty()) {
            log.coalition_seen = true;
        }
        let vassals = w.politics.vassal_of.iter().filter(|v| v.is_some()).count();
        log.vassals_max = log.vassals_max.max(vassals);
        log.polities.push(alive_count(w));
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

    // monsoon: the ITCZ march should breathe hardest in the tropics
    let mut trop_amp: Vec<f64> = Vec::new();
    let mut mid_amp: Vec<f64> = Vec::new();
    for y in 0..rows {
        for x in 0..cols {
            if !land[[y, x]] {
                continue;
            }
            let a = w.pamp[[y, x]].abs();
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
    for (&cpk, &l) in w.crops.iter().zip(land.iter()) {
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
    c.range("desert share of land", desert, pct(desert), (0.12, 0.28), (0.06, 0.38), "sweet 12–28% · hard 6–38%");
    c.range("tundra+ice share of land", frozen, pct(frozen), (0.05, 0.30), (0.01, 0.45), "sweet 5–30% · hard 1–45%");
    c.range("forest share of land", forest, pct(forest), (0.25, 0.60), (0.15, 0.75), "sweet 25–60% · hard 15–75%");
    c.range("grass+savanna share of land", open, pct(open), (0.10, 0.45), (0.04, 0.60), "sweet 10–45% · hard 4–60%");
    c.range("land mean temperature", t_mean, format!("{:.1}°C", t_mean), (5.0, 20.0), (-2.0, 28.0), "sweet 5–20°C · hard -2–28°C");
    c.range("land mean precipitation", p_mean, format!("{:.0}mm", p_mean), (500.0, 1500.0), (250.0, 2400.0), "sweet 500–1500 · hard 250–2400");
    c.range("mean seasonal swing", a_mean, format!("{:.1}°C", a_mean), (4.0, 14.0), (2.0, 20.0), "sweet 4–14°C · hard 2–20°C");
    c.range("tropical monsoon amplitude", trop_m, format!("{:.2}", trop_m), (0.12, 0.55), (0.05, 0.85), "sweet .12–.55 · hard .05–.85");
    c.want("monsoon lives in the tropics", trop_m > mid_m, format!("{:.2} vs {:.2}", trop_m, mid_m), "ITCZ march beats continental swing");
    c.range("arable share of land", arable, pct(arable), (0.15, 0.65), (0.06, 0.85), "M2.1: wheat+rice+maize belts cover the good land");
    c.range("pastoral share of land", pshare(4), pct(pshare(4)), (0.02, 0.45), (0.005, 0.65), "M2.1: the dry steppe carries herds");
    c.want("rice hugs the water", pk[2] == 0 || pk[2] < pk[1] + pk[3], format!("rice {} vs wheat+maize {}", pk[2], pk[1] + pk[3]), "paddies are the exception, not the rule");
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

    // Strahler orders, wadis, and the dead seas of the dry country
    let mut order_hist = [0usize; 13];
    let mut top_order = 0u8;
    for (&o, &r) in w.strahler.iter().zip(w.rivers.iter()) {
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
    let wadi_n = w.seasonal.iter().filter(|&&s| s).count();
    let salt_cells = w.salt.iter().filter(|&&s| s).count();
    let salt_comp = ndimage::label(&w.salt, true).areas.len();
    let famp: Vec<f64> = w
        .flow_amp
        .iter()
        .zip(w.rivers.iter())
        .filter(|(_, &r)| r)
        .map(|(&a, _)| a.abs())
        .collect();
    let famp_mean = famp.iter().sum::<f64>() / famp.len().max(1) as f64;
    println!(
        "wadis: {} cells · salt basins: {} ({} cells) · river |flow amp| mean {:.2}",
        wadi_n, salt_comp, salt_cells, famp_mean
    );

    let finite = dis.iter().all(|d| d.is_finite());
    let mut c = Checks::default();
    c.range("river share of land", river_n / land_n.max(1.0), pct(river_n / land_n.max(1.0)), (0.008, 0.05), (0.003, 0.10), "sweet 0.8–5% · hard 0.3–10%");
    c.range("lake share of land", lake_n / land_n.max(1.0), pct(lake_n / land_n.max(1.0)), (0.0, 0.03), (0.0, 0.08), "sweet 0–3% · hard 0–8%");
    c.range("river systems", systems as f64, format!("{}", systems), (8.0, 400.0), (3.0, 2000.0), "sweet 8–400 · hard 3–2000");
    c.want("named deltas", deltas >= 1, format!("{}", deltas), "≥1 — great river mouths get names");
    c.must("discharge finite", finite, if finite { "yes".into() } else { "NO".into() }, "no NaN/inf in flow accumulation");
    // dawn towns all on fresh water is history, not a bug — the dry
    // harbours and mining camps arrive with the colonies (checked in civ).
    let rt_share = river_towns as f64 / w.settlements.len().max(1) as f64;
    c.want("dawn towns reach fresh water", rt_share >= 0.2, pct(rt_share), "≥20% — rivers should pull the first peoples");
    c.range("strahler top order", top_order as f64, format!("{}", top_order), (4.0, 9.0), (3.0, 12.0), "sweet 4–9 · hard 3–12");
    c.range("river seasonal swing", famp_mean, format!("{:.2}", famp_mean), (0.05, 0.50), (0.01, 0.90), "mean |amp| · sweet .05–.50 · hard .01–.90");
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
    c.range("population growth ×", growth, format!("{:.2}×", growth), (2.0, 1200.0), (1.05, 3000.0), "M2 crop-package K: filled worlds run ~10⁶ souls from tiny dawns");
    if years >= 100 {
        // pacing: the world should still be becoming in its second half,
        // not sitting on a saturated plateau for a century.
        let half_pop = log.rows[years / 2 - 1].1 as f64;
        let pace = half_pop / pop1.max(1) as f64;
        c.want("still growing at half-run", pace <= 0.92, format!("{:.0}% of final", 100.0 * pace), "pop at half-run ≤92% of final");
    }
    c.want("settlements grew", w.settlements.len() >= setts0, format!("{}→{}", setts0, w.settlements.len()), "colonies should outnumber the dawn towns");
    if years >= 100 {
        // by the century mark the colonies should have broken the river
        // monoculture: dry-coast harbours, cistern towns, mining camps.
        let dry = w.settlements.iter().filter(|s| !s.river).count();
        c.want("dry-country towns exist", dry >= 1, format!("{} of {}", dry, w.settlements.len()), "≥1 town beyond fresh water by the century mark");
    }
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

    // ---- M2.3 rank-size: log(pop) vs log(rank) OLS slope ≈ −1 (Zipf) ----
    let mut pops: Vec<f64> = w.settlements.iter().map(|s| s.pop as f64).filter(|&p| p >= 120.0).collect();
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
        c.range("rank-size slope (Zipf)", slope, format!("{:.2} over {} towns", slope, pops.len()), (-1.3, -0.8), (-1.75, -0.5), "M2.3 gate: −1.3…−0.8");
    }

    // ---- M2.5 spacing: median nearest-neighbour distance in km ----
    let mut nn: Vec<f64> = Vec::new();
    for (i, s) in w.settlements.iter().enumerate() {
        let mut best = f64::INFINITY;
        for (j, o) in w.settlements.iter().enumerate() {
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
        c.range("median town spacing", med, format!("{:.0} km", med), (14.0, 48.0), (8.0, 120.0), "M2.5: market-town band ~15–30 km in settled cores");
    }

    // ---- M2.6 famine: dry years starve somewhere, but not everywhere ----
    if years >= 100 {
        let per_c = log.famines as f64 * 100.0 / years.max(1) as f64;
        c.range("famine events per century", per_c, format!("{:.1}", per_c), (1.0, 60.0), (0.0, 150.0), "M2.6: the rains must fail sometimes");
    }

    // ---- M3 gates: words and ways ----
    // M3.1 label audit: a town's name must classify to the tongue that
    // coined it — it starts with one of the bank's openers and ends with
    // one of its closers. Audited against the NAMER, not the current
    // owner: conquest moves borders, not names (M9.2).
    {
        let mut audited = 0usize;
        let mut hits = 0usize;
        for s in &w.settlements {
            let Some(cu) = w.cultures.get(s.namer) else { continue };
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
            c.range("toponyms classify to culture", share, pct(share), (0.9, 1.0), (0.8, 1.0), "M3 gate: sampled toponyms ≥ 90%");
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
        let s_ety = w.settlements.iter().filter(|s| !s.ety.is_empty()).count();
        let f_ety = w.features.iter().filter(|f| !f.ety.is_empty()).count();
        c.must("settlement etymologies", s_ety == w.settlements.len(), format!("{}/{}", s_ety, w.settlements.len()), "M3.3: every town name reads back");
        c.must("feature etymologies", f_ety == w.features.len(), format!("{}/{}", f_ety, w.features.len()), "M3.3: every feature name reads back");

        // M3 no name-collision regressions: towns, features, peoples.
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut dups = 0usize;
        for n in w
            .settlements
            .iter()
            .map(|s| s.name.as_str())
            .chain(w.features.iter().map(|f| f.name.as_str()))
            .chain(w.cultures.iter().map(|cu| cu.people.as_str()))
        {
            if !seen.insert(n) {
                dups += 1;
            }
        }
        c.must("name collisions", dups == 0, format!("{}", dups), "M3 gate: 0 duplicates");

        // M3.4 exonyms: where two peoples actually share country, a border
        // feature should carry a second name in the other tongue. Peoples
        // on far-apart continents legitimately double nothing — count the
        // candidate features first (same geometry as culture_toponyms) and
        // only demand exonyms when candidates exist.
        if w.cultures.len() >= 2 {
            let mut candidates = 0usize;
            for f in &w.features {
                if matches!(f.t.as_str(), "ocean" | "sea" | "continent" | "river" | "delta") {
                    continue;
                }
                let mut best = vec![f64::INFINITY; w.cultures.len()];
                for s in &w.settlements {
                    let dx = (s.x - f.x) as f64;
                    let dy = (s.y - f.y) as f64;
                    let d2 = dx * dx + dy * dy;
                    if d2 < best[s.culture] {
                        best[s.culture] = d2;
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
        let full = w.cultures.iter().filter(|cu| cu.pantheon.len() >= 3).count();
        c.must("pantheons complete", full == w.cultures.len(), format!("{}/{}", full, w.cultures.len()), "M3.5: ≥ 3 named gods per people");
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

        // Territory sanity: every owned cell names a real culture, and the
        // political map covers a sane share of the land.
        let n_cult = w.cultures.len() as i16;
        let terr = w.territory.as_slice().unwrap_or(&[]);
        let mut owned = 0usize;
        let mut bad = 0usize;
        let mut land_cells = 0usize;
        for (i, &h) in w.height.iter().enumerate() {
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
        c.must("territory owners valid", bad == 0, format!("{} bad cells", bad), "M4.1: every owned cell names a live culture");
        c.range("land under banners", owned_share, pct(owned_share), (0.05, 0.85), (0.01, 0.98), "M4.1: realms claim some — never all — of the wild");

        if years >= 100 {
            c.want("wars kindle", log.wars >= 1, format!("{}", log.wars), "M4: a century without war is a broken game");
            if log.wars >= 8 {
                c.want("land changes hands", log.transfers >= 1, format!("{} transfers / {} war events", log.transfers, log.wars), "M4.2 gate: border change per major war");
            }
            c.want("polity count moves", pol_max > pol_min, format!("{}–{}", pol_min, pol_max), "M4.5 gate: realms must rise and fall");
            let top_share = {
                let total: i64 = w.settlements.iter().map(|s| s.pop).sum();
                let mut by_c: BTreeMap<usize, i64> = BTreeMap::new();
                for s in &w.settlements {
                    *by_c.entry(s.culture).or_default() += s.pop;
                }
                by_c.values().max().copied().unwrap_or(0) as f64 / total.max(1) as f64
            };
            c.range("largest realm pop share", top_share, pct(top_share), (0.1, 0.75), (0.02, 0.92), "M4 gate: no runaway single empire");
            let _ = pol_last;
        }
        if years >= 140 {
            c.want("coalitions form", log.coalition_seen, if log.coalition_seen { "yes".into() } else { "no".into() }, "M4.3 gate: dread should unite the wary");
        }
    }

    // ---- M5 gates: iron and coin ----
    {
        let n_areas = w.areas.markets.len();
        if w.settlements.len() >= 30 {
            c.must("market areas carved", n_areas >= 2, format!("{} areas / {} towns", n_areas, w.settlements.len()), "M5.2: the route web splits into local markets");
        }
        // inter-area price divergence: mean over goods of max/min across areas
        if n_areas >= 2 {
            let goods: std::collections::BTreeSet<String> = w
                .settlements
                .iter()
                .flat_map(|s| s.goods.iter().cloned())
                .collect();
            let mut ratios = Vec::new();
            for g in &goods {
                let ps: Vec<f64> = w.areas.markets.iter().map(|m| m.price(g)).collect();
                let lo = ps.iter().cloned().fold(f64::INFINITY, f64::min);
                let hi = ps.iter().cloned().fold(0.0f64, f64::max);
                if lo > 0.0 {
                    ratios.push(hi / lo);
                }
            }
            if !ratios.is_empty() {
                let mean = ratios.iter().sum::<f64>() / ratios.len() as f64;
                c.range("inter-area price divergence", mean, format!("×{:.2} mean spread", mean), (1.03, 3.0), (1.0, 6.0), "M5.2 gate: local markets disagree, but not madly");
            }
        }
        if years >= 100 {
            let crafts = w
                .settlements
                .iter()
                .filter(|s| s.goods.iter().any(|g| matches!(g.as_str(), "tools" | "weapons" | "jewelry")))
                .count();
            c.want("recipe towns emerge", crafts >= 1, format!("{} towns craft", crafts), "M5.1 gate: ore, fuel and art make finished goods");
            let coin_known = w.societies.iter().any(|so| so.knows("coin"));
            if coin_known {
                c.want("merchants take the roads", !w.merchants.is_empty(), format!("{} ever", w.merchants.len()), "M5.5: coin-wise realms send out traders");
            }
        }
        // M5.4 gravity cross-check: realized route flow should correlate
        // with pop·pop/cost² over the same edges
        {
            let by_id: BTreeMap<i64, &_> = w.settlements.iter().map(|s| (s.id, s)).collect();
            let mut xs = Vec::new();
            let mut ys = Vec::new();
            for (ri, r) in w.routes.iter().enumerate() {
                let (Some(sa), Some(sb)) = (by_id.get(&r.a), by_id.get(&r.b)) else { continue };
                let flow = w.route_flow.get(ri).copied().unwrap_or(0.0);
                if flow <= 0.0 {
                    continue;
                }
                let grav = (sa.pop as f64 * sb.pop as f64) / r.cost.max(1.0).powi(2);
                xs.push(grav.ln());
                ys.push(flow.ln());
            }
            if xs.len() >= 12 {
                let corr = pearson(&xs, &ys);
                c.range("gravity-model correlation", corr, format!("r={:.2} over {} routes", corr, xs.len()), (0.30, 1.0), (0.10, 1.0), "M5.4 gate: big close pairs carry the trade");
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
    let mut means: BTreeMap<&str, f64> = BTreeMap::new();
    for (g, s) in &series {
        let base = economy::base_value(g);
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

    // ---- the mines: does the world work what it knows? ----
    const MINERALS: [&str; 7] = ["stone", "coal", "copper", "iron", "silver", "gold", "mithril"];
    let known: Vec<_> = w.deposits.iter().filter(|d| d.known && d.left > 0.0 && MINERALS.contains(&d.r.as_str())).collect();
    let worked = known.iter().filter(|d| {
        w.settlements.iter().any(|s| {
            let r = calliope::settlements::work_radius(s.pop);
            let dx = (d.x - s.x) as f64;
            let dy = (d.y - s.y) as f64;
            dx * dx + dy * dy <= r * r && s.goods.iter().any(|g| g == &d.r)
        })
    }).count();
    let mut goods_census: BTreeMap<&str, usize> = BTreeMap::new();
    for s in &w.settlements {
        for g in &s.goods {
            *goods_census.entry(g.as_str()).or_default() += 1;
        }
    }
    println!();
    println!("mines: {} live mineral seams known · {} worked by a town", known.len(), worked);
    println!("towns listing each good: {}", goods_census.iter().map(|(g, n)| format!("{} {}", g, n)).collect::<Vec<_>>().join(" · "));
    if !known.is_empty() {
        let share = worked as f64 / known.len() as f64;
        c.range("known seams worked", share, pct(share), (0.35, 1.0), (0.10, 1.0), "found ore must reach the market, not rust in the hills");
    }

    // ---- M2.4 Bettencourt: β from ln(wealth) ~ β·ln(pop) across towns ----
    let pts: Vec<(f64, f64)> = w.settlements.iter()
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
        c.range("wealth~pop scaling β", beta, format!("{:.2} over {} towns", beta, pts.len()), (0.90, 1.60), (0.50, 2.10), "M2.4: superlinear output, target ≈1.15");
    }

    // ---- M2.7 price ratios vs the medieval envelope ----
    if let (Some(&pg), Some(&pi_), Some(&pt)) = (means.get("grain"), means.get("iron"), means.get("timber")) {
        let ig = pi_ / pg.max(1e-9);
        c.range("iron/grain price ratio", ig, format!("{:.1}×", ig), (1.5, 14.0), (0.8, 40.0), "M2.7: metal dear, bread cheap");
        let ok = pg < pi_ && pt < pi_;
        c.want("staples cheaper than metal", ok, format!("grain {:.2} · timber {:.2} · iron {:.2}", pg, pt, pi_), "the ordering of the price lists holds");
    }
    if let (Some(&pg), Some(&pau)) = (means.get("grain"), means.get("gold")) {
        let r = pau / pg.max(1e-9);
        c.range("gold/grain price ratio", r, format!("{:.1}×", r), (2.5, 80.0), (1.2, 300.0), "M2.7: the precious envelope");
    }
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

    let evs = &w.events;
    let reg = &w.registry;
    let n = evs.len().max(1);

    // ---- the log itself
    let with_ids = evs.iter().filter(|e| !e.ids.is_empty()).count();
    let with_xy = evs.iter().filter(|e| e.x >= 0).count();
    let loud = evs.iter().filter(|e| telling::weight(&e.k) >= 3).count();
    let loud_legend = evs
        .iter()
        .filter(|e| telling::weight(&e.k) >= 3 && !e.legend.is_empty())
        .count();
    let bad_ids = evs
        .iter()
        .flat_map(|e| e.ids.iter())
        .filter(|&&id| id < 0 || id as usize >= reg.items.len())
        .count();
    println!("chronicle: {} entries over {}y ({:.1}/y)", evs.len(), years, evs.len() as f64 / years as f64);
    println!("  ids on {:.1}% · coords on {:.1}% · legend layer on {}/{} loud entries", 100.0 * with_ids as f64 / n as f64, 100.0 * with_xy as f64 / n as f64, loud_legend, loud);
    if with_ids < evs.len() {
        let mut orphans: BTreeMap<&str, usize> = BTreeMap::new();
        let mut samples: Vec<&Event> = Vec::new();
        for e in evs.iter().filter(|e| e.ids.is_empty()) {
            *orphans.entry(e.k.as_str()).or_default() += 1;
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
        let k = kinds.entry(e.kind.as_str()).or_default();
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
    let held = w.artifacts.iter().filter(|a| a.holder >= 0).count();
    println!("artifacts: {} wrought · {} held · {} lost", w.artifacts.len(), held, w.artifacts.len() - held);

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
    c.range("events mappable (coords)", with_xy as f64 / n as f64, pct(with_xy as f64 / n as f64), (0.65, 1.0), (0.45, 1.0), "most entries can fly the camera");
    c.must("loud events carry a legend", loud_legend == loud, format!("{}/{}", loud_legend, loud), "M6.9: two-layer telling on weight ≥ 3");
    c.must("closed entities carry a fate", closed_unfated == 0, format!("{} unfated", closed_unfated), "every ending is written");
    c.range("stories per century", per_century, format!("{:.1}", per_century), (5.0, 48.0), (2.0, 60.0), "M6.5 gate: the sifter yields");
    c.must("sifter deterministic", sift_det, if sift_det { "identical".into() } else { "DIVERGED".into() }, "same log ⇒ same stories");
    c.must("no duplicate stories", dup_titles == 0, format!("{} dups", dup_titles), "dedup bounds hold");
    c.want("a reversal story found", stories.iter().any(|s| matches!(s.pattern.as_str(), "rise-fall" | "tide-turned" | "mine-curse")), "yes".into(), "M6.7: fortunes turn on the record");
    c.want("relics wrought", !w.artifacts.is_empty(), format!("{}", w.artifacts.len()), "M6.3: artifacts enter the world");
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
        let evs = &w.events;
        let veiled = evs.iter().filter(|e| e.veiled).count();
        let transfers = evs.iter().filter(|e| e.text.contains("passes from the")).count();
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
        let strata_over = w.settlements.iter().filter(|s| s.formerly.len() > 2).count();
        let ungloseed = w
            .settlements
            .iter()
            .filter(|s| !s.formerly.is_empty() && s.ety.is_empty())
            .count()
            + w.features
                .iter()
                .filter(|f| (!f.formerly.is_empty() || f.t == "battlefield") && f.ety.is_empty())
                .count();
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
        });
    }

    println!(
        "{:>7} {:>6} {:>7} {:>8} {:>7} {:>7} {:>6} {:>6} {:>6} {:>6} {:>7}",
        "seed", "ruins", "late", "veiled%", "rename", "worn", "field", "faded", "wars", "xfers", "rivers"
    );
    for r in &rows {
        println!(
            "{:>7} {:>6} {:>7} {:>8} {:>7} {:>7} {:>6} {:>6} {:>6} {:>6} {:>7}",
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
    c.range(
        "ruins per century (after y100)",
        ruin_rate,
        format!("{:.2}", ruin_rate),
        (1.0, 12.0),
        (0.5, 20.0),
        "M9.1 gate: mature worlds carry ruins",
    );
    c.range(
        "withheld share of the chronicle",
        veil_share,
        pct(veil_share),
        (0.02, 0.08),
        (0.015, 0.10),
        "M9.5 gate: 2-8% of entries veiled",
    );
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
        famines: usize,
        zipf: f64,
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

        // M2.3: per-seed rank-size slope (NaN when too few towns to judge)
        let mut pops: Vec<f64> = w.settlements.iter().map(|s| s.pop as f64).filter(|&p| p >= 120.0).collect();
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

        println!("{:>7} {:>6.1} {:>6.1} {:>6.1} {:>5.1} {:>5} {:>4} {:>2}→{:<2} {:>9} {:>6.2} {:>5} {:>4} {:>4} {:>4} {:>4} {:>4} {:>5.1} {:>6}  {}", seed, 100.0 * land_frac, 100.0 * desert, 100.0 * forest, 100.0 * mtn, li.n, w.deposits.len(), setts0, w.settlements.len(), pop1, growth, era, arts, log.strikes, log.camps, log.wars, w.routes.len(), evyr, gen_ms, flags);

        rows.push(Row { seed, land: land_frac, desert, forest, mtn, camps: log.camps, strikes: log.strikes, famines: log.famines, zipf, growth, pace, era, evyr, flags });
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
    c.range("mean growth", m_grw, format!("{:.2}×", m_grw), (2.0, 1200.0), (1.05, 3000.0), "M2 crop-package K: sweet 2–1200×");
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
        c.range("mean rank-size slope", mz, format!("{:.2} over {} seeds", mz, zipfs.len()), (-1.3, -0.8), (-1.9, -0.45), "M2.3 gate: Zipf holds across the sweep");
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
        let (rows, cols) = w.height.dim();

        // ---- P1: every river cell descends the filled surface to an outlet
        let water = w.height.mapv(|h| h < 0.0);
        let filled = fill_rect(&w.height, &water);
        let dirs = dirs_rect(&filled, &water);
        let mut uphill = 0usize;
        let mut cycles = 0usize;
        let mut dry_terminals = 0usize;
        let mut river_cells = 0usize;
        for y in 0..rows {
            for x in 0..cols {
                if !w.rivers[[y, x]] { continue; }
                river_cells += 1;
                let (mut cy, mut cx) = (y, x);
                let mut steps = 0usize;
                loop {
                    let d = dirs[[cy, cx]];
                    if d < 0 {
                        // legitimate terminals: the basin floor of an
                        // endorheic lake, or a lake cell (ghost-spill)
                        if !(w.lakes[[cy, cx]] || w.salt[[cy, cx]]) { dry_terminals += 1; }
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
        let living: Vec<usize> = (0..w.settlements.len()).filter(|&i| w.settlements[i].pop > 0).collect();
        let mut idx_of: BTreeMap<i64, usize> = BTreeMap::new();
        for (k, &i) in living.iter().enumerate() { idx_of.insert(w.settlements[i].id, k); }
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

        // ---- P3: the pack protocol round-trips byte-perfectly
        let p1 = w.pack();
        let p2 = w.pack();
        c.must(&format!("pack is stable ({})", seed), p1 == p2,
            format!("{} B", p1.len()), "M8.1: same world ⇒ same bytes");
        let hlen = u32::from_le_bytes([p1[0], p1[1], p1[2], p1[3]]) as usize;
        let hdr: serde_json::Value = serde_json::from_slice(&p1[4..4 + hlen]).unwrap();
        let entries = hdr["arrays"].as_array().unwrap();
        let mut ok_layout = true;
        let mut expected_off = 0usize;
        let mut total = 0usize;
        for e in entries {
            let off = e["offset"].as_u64().unwrap() as usize;
            let nb = e["nbytes"].as_u64().unwrap() as usize;
            let shape: Vec<usize> = e["shape"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as usize).collect();
            let cell = match e["dtype"].as_str().unwrap() { "float32" => 4, "int16" => 2, _ => 1 };
            ok_layout &= off == expected_off && nb == shape[0] * shape[1] * cell;
            expected_off = off + nb;
            total += nb;
        }
        ok_layout &= p1.len() == 4 + hlen + total;
        c.must(&format!("unpack layout sound ({})", seed), ok_layout,
            format!("{} arrays", entries.len()), "M8.1: offsets contiguous, sizes exact");
        // decode height + territory back out and compare to the source arrays
        let base = 4 + hlen;
        let mut ok_data = true;
        for e in entries {
            let name = e["name"].as_str().unwrap();
            let off = base + e["offset"].as_u64().unwrap() as usize;
            let nb = e["nbytes"].as_u64().unwrap() as usize;
            match name {
                "height" => {
                    let vals: Vec<f32> = p1[off..off + nb].chunks_exact(4)
                        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
                    ok_data &= vals.len() == w.height.len()
                        && vals.iter().zip(w.height.iter()).all(|(&v, &h)| v == h as f32);
                }
                "territory" => {
                    let vals: Vec<i16> = p1[off..off + nb].chunks_exact(2)
                        .map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
                    ok_data &= vals.iter().zip(w.territory.iter()).all(|(&v, &t)| v == t);
                }
                "flags" => {
                    ok_data &= p1[off..off + nb].iter()
                        .zip(w.rivers.iter().zip(w.lakes.iter()).zip(w.salt.iter()).zip(w.seasonal.iter()))
                        .all(|(&f, (((&r, &l), &s), &sw))| {
                            f == (r as u8) | ((l as u8) << 1) | ((s as u8) << 2) | ((sw as u8) << 3)
                        });
                }
                _ => {}
            }
        }
        c.must(&format!("unpack = world ({})", seed), ok_data,
            "bit-equal".into(), "M8.1: arrays survive the wire");

        // ---- M8.2 metamorphic: more rain must not shrink the rivers
        let dry = World::generate(seed, size);
        let wet = World::generate_scaled(seed, size, 1.25);
        let rc_dry = dry.rivers.iter().filter(|&&r| r).count();
        let rc_wet = wet.rivers.iter().filter(|&&r| r).count();
        let q_dry: f64 = dry.discharge.iter().sum();
        let q_wet: f64 = wet.discharge.iter().sum();
        println!("seed {:>6}: rain ×1.25 ⇒ river cells {} → {} · discharge {:.0} → {:.0}",
            seed, rc_dry, rc_wet, q_dry, q_wet);
        c.must(&format!("rain↑ ⇒ rivers not↓ ({})", seed), rc_wet >= rc_dry,
            format!("{} → {}", rc_dry, rc_wet), "M8.2: metamorphic monotonicity");
        c.must(&format!("rain↑ ⇒ discharge↑ ({})", seed), q_wet > q_dry,
            format!("×{:.2}", q_wet / q_dry.max(1e-9)), "M8.2: more water flows");
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
    let (rows, cols) = w.height.dim();
    let total = (rows * cols) as f64;
    let mut land = 0usize;
    let mut river = 0usize;
    let mut mountain = 0usize;
    let mut coast = 0usize;
    let mut bcount = vec![0f64; 12];
    let mut hyps = vec![0f64; 8];
    for y in 0..rows {
        for x in 0..cols {
            let h = w.height[[y, x]];
            if h < 0.0 { continue; }
            land += 1;
            if w.rivers[[y, x]] { river += 1; }
            if h > 0.45 { mountain += 1; }
            let b = w.biomes[[y, x]] as usize;
            if b < bcount.len() { bcount[b] += 1.0; }
            let bin = ((h.max(0.0).min(0.999)) * 8.0) as usize;
            hyps[bin] += 1.0;
            let mut sea_adj = false;
            for (dy, dx) in [(-1isize, 0isize), (1, 0), (0, -1), (0, 1)] {
                let ny = y as isize + dy;
                let nx = x as isize + dx;
                if ny >= 0 && nx >= 0 && ny < rows as isize && nx < cols as isize
                    && w.height[[ny as usize, nx as usize]] < 0.0 { sea_adj = true; }
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
            if w.height[[y, x]] < 0.0 || seen[[y, x]] { continue; }
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
                        if w.height[[ny, nx]] >= 0.0 && !seen[[ny, nx]] {
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
            if w.height[[y, x]] >= 0.0 {
                let gy = y * 5 / rows;
                let gx = x * 5 / cols;
                grid[gy * 5 + gx] += 1.0;
            }
        }
    }
    // settlement spacing: nearest-neighbour distance, km
    let pts: Vec<(f64, f64)> = w.settlements.iter().filter(|s| s.pop > 0)
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
    c.range("ERA occupancy / seed", occ_mean, format!("{:.2}", occ_mean), (0.45, 1.0), (0.3, 1.0),
        "M8.3: seeds spread across the plates");
    // Collapse alarm: a duplicated pair drives min/mean toward 0 regardless
    // of how many seeds are sampled (min alone shrinks with pair count).
    // Healthy generator reads ~0.10-0.19 across 8-16 seeds; mean ~0.05-0.07.
    c.range("oatmeal min/mean ratio", ratio, format!("{:.3}", ratio), (0.05, 1.0), (0.02, 1.0),
        "M8.4: no two worlds are the same bowl");
    c.range("oatmeal mean distance", mean_d, format!("{:.4}", mean_d), (0.04, 0.75), (0.02, 0.9),
        "M8.4: the family resembles, never repeats");
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
            println!("usage: diagnose <terrain|climate|hydro|resources|civ|economy|telling|determinism|bench|sweep|properties|era|patina> [args]");
            println!("  terrain|climate|hydro|resources  <seed=12345> <size=512>");
            println!("  civ <seed> <size> <years=120> · economy <seed> <size> <years=80> · telling <seed> <size> <years=150>");
            println!("  determinism <seed> <size> <months=120> · bench · sweep <size> <years> <seeds…>");
            println!("  properties <size=512> <years=60> <seeds…> · era <size=256> <years=60> <n=16> <base=12345>");
            println!("  patina <size=512> <years=300> <seeds…>");
        }
    }
}
