//! Native harness: generate a world, print stats, run years of simulation.

use calliope::world::World;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seed: i64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(12345);
    let size: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(512);
    let months: i64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);

    let mut w = World::generate(seed, size);

    let total = w.height.len() as f64;
    let land = w.height.iter().filter(|&&h| h >= 0.0).count() as f64;
    let rivers = w.rivers.iter().filter(|&&r| r).count();
    let lakes = w.lakes.iter().filter(|&&l| l).count();

    println!("world '{}' seed={} size={}x{}", w.world_name, seed, w.width, size);
    println!("land: {:.1}%  rivers: {} cells  lakes: {} cells", 100.0 * land / total, rivers, lakes);
    println!(
        "deposits: {}  settlements: {}  cultures: {}  features: {}  routes: {}",
        w.deposits.len(),
        w.settlements.len(),
        w.cultures.len(),
        w.features.len(),
        w.routes.len()
    );
    let mut census: std::collections::BTreeMap<&str, usize> = Default::default();
    for f in &w.features {
        *census.entry(f.t.as_str()).or_default() += 1;
    }
    let parts: Vec<String> = census.iter().map(|(k, v)| format!("{k}:{v}")).collect();
    println!("feature census: {}", parts.join(" "));
    for f in w.features.iter().take(6) {
        println!("  feature[{}] {} @({},{})", f.t, f.name, f.x, f.y);
    }
    for f in &w.features {
        if matches!(f.t.as_str(), "bay" | "strait" | "cape" | "peak" | "highland" | "marsh" | "delta") {
            println!("  geo[{}] {} @({},{})", f.t, f.name, f.x, f.y);
        }
    }
    for s in w.settlements.iter().take(5) {
        println!(
            "  {} ({}) pop={} food={} goods={:?}",
            s.name, s.tier, s.pop, s.food, s.goods
        );
    }
    let meta = w.meta();
    println!("timings: {}", meta["timings"]);

    let packed = w.pack();
    println!("packed payload: {} bytes", packed.len());

    if months > 0 {
        let mut all_events = 0usize;
        let mut chunks = months;
        while chunks > 0 {
            let step = chunks.min(240);
            let (evs, _founded) = w.tick(step);
            all_events += evs.len();
            chunks -= step;
        }
        let pop: i64 = w.settlements.iter().map(|s| s.pop).sum();
        println!(
            "after {} months: month={} settlements={} pop={} routes={} events={}",
            months,
            w.month,
            w.settlements.len(),
            pop,
            w.routes.len(),
            all_events
        );
        for e in w.events.iter().rev().take(5) {
            println!("  [m{}] {}", e.m, e.text);
        }
    }
}
