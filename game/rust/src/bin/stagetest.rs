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
    let plates = calliope::plates::generate(seed, size);
    println!("plates     {:>6} ms · {} polygons", t.elapsed().as_millis(), plates.plates.len());

    let t = Instant::now();
    let mut height = geo::heightmap(seed, size, &plates);
    println!("terrain    {:>6} ms", t.elapsed().as_millis());

    let t = Instant::now();
    calliope::erosion::erode(&mut height);
    println!("erosion    {:>6} ms", t.elapsed().as_millis());

    let water = height.mapv(|h| h < 0.0);
    let t = Instant::now();
    let lat = climate::latitude_deg(size);
    let cont = climate::continentality(&water);
    let tmean = climate::temperature_mean(&height, &lat);
    let _tamp = climate::temperature_amplitude(&lat, &cont);
    let (precip, pamp) = climate::precipitation(&height, &water, &tmean, &lat, &cont);
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

    // E3.2 — the human stages read the world's resting f32 grids.
    let height32 = height.mapv(|x| x as f32);
    let tmean32 = tmean.mapv(|x| x as f32);
    let precip32 = precip.mapv(|x| x as f32);
    let discharge32 = hydro.discharge.mapv(|x| x as f32);

    let t = Instant::now();
    let (features, world_name) = naming::name_features(
        &height32, &biome_map, &hydro.rivers, &hydro.lakes, &discharge32, &tmean32, &precip32, seed,
    );
    println!("naming     {:>6} ms · {} features · world {}", t.elapsed().as_millis(), features.len(), world_name);

    let t = Instant::now();
    // M18/M19 — the basement classifies off the sketch and the relief,
    // then the ore roll reads it.
    let rock = calliope::rock::classify(seed, size, &plates, &height32);
    let deposits = resources::place_resources(&biome_map, &height32, &hydro.rivers, &hydro.lakes, &rock, seed);
    println!("resources  {:>6} ms · {} deposits", t.elapsed().as_millis(), deposits.len());

    println!("ALL STAGES OK");
}
