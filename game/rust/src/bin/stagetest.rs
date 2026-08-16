//! Minimal stage-by-stage smoke test: runs each generation stage directly
//! and prints wall-clock per stage, so a hang names its stage immediately.

use calliope::{agriculture, biomes as biomes_mod, climate, geo, hydrology, naming, resources};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seed: i64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(12345);
    let size: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(320);
    println!("stagetest seed {} size {}", seed, size);

    let t = Instant::now();
    let mut height = geo::heightmap(seed, size);
    println!("terrain    {:>6} ms", t.elapsed().as_millis());

    let t = Instant::now();
    calliope::erosion::erode(&mut height);
    println!("erosion    {:>6} ms", t.elapsed().as_millis());

    let water = height.mapv(|h| h < 0.0);
    let t = Instant::now();
    let lat = climate::latitude_deg(size);
    let tmean = climate::temperature_mean(&height, &lat);
    let _tamp = climate::temperature_amplitude(&lat, &water);
    let (precip, pamp) = climate::precipitation(&height, &water, &tmean, &lat);
    println!("climate    {:>6} ms", t.elapsed().as_millis());

    let t = Instant::now();
    let hydro = hydrology::hydrology(&height, &water, &precip, &pamp, &tmean);
    println!("hydrology  {:>6} ms", t.elapsed().as_millis());

    let t = Instant::now();
    let biome_map = biomes_mod::classify(&height, &tmean, &precip, &hydro.lakes);
    println!("biomes     {:>6} ms", t.elapsed().as_millis());

    let t = Instant::now();
    let _fert = agriculture::fertility(&height, &tmean, &precip, &hydro.rivers, &hydro.lakes, &hydro.discharge);
    println!("fertility  {:>6} ms", t.elapsed().as_millis());

    let t = Instant::now();
    let (features, world_name) = naming::name_features(
        &height, &biome_map, &hydro.rivers, &hydro.lakes, &hydro.discharge, &tmean, &precip, seed,
    );
    println!("naming     {:>6} ms · {} features · world {}", t.elapsed().as_millis(), features.len(), world_name);

    let t = Instant::now();
    let deposits = resources::place_resources(&biome_map, &height, &hydro.rivers, &hydro.lakes, seed);
    println!("resources  {:>6} ms · {} deposits", t.elapsed().as_millis(), deposits.len());

    println!("ALL STAGES OK");
}
