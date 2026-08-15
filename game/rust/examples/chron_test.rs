use calliope::world::World;
fn main() {
    let mut w = World::generate(1337, 256);
    println!("--- founding events ---");
    for e in w.events.iter().take(10) {
        println!("[{}] {}", e.k, e.text);
    }
    let (evs, _) = w.tick(240); // 20 years
    println!("--- 20 years, {} events ---", evs.len());
    use std::collections::HashMap;
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for e in &evs { *counts.entry(e.k.as_str()).or_default() += 1; }
    println!("{:?}", counts);
    for k in ["ruler", "war", "omen", "festival", "wonder", "trade", "disaster"] {
        if let Some(e) = evs.iter().find(|e| e.k == k) {
            println!("[{}] m={} {}", e.k, e.m, e.text);
        }
    }
    let meta = w.meta();
    println!("culture0: {}", meta["cultures"][0]);
}
