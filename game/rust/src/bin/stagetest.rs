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

    // M28/M29/M30 — the glacial stage, mirroring the GenBuilder:
    // footprint, carve, then the depositional legacy, before climate.
    let t = Instant::now();
    let mut ice = calliope::ice::compute(seed, &height);
    calliope::ice::carve(&mut height, &mut ice);
    calliope::ice::deposit(seed, &mut height, &mut ice);
    calliope::ice::proglacial(&mut height, &mut ice);
    calliope::ice::outwash(&mut height, &mut ice);
    calliope::ice::loess_mantle(&height, &mut ice);
    println!(
        "glacial    {:>6} ms · {} cirques · {} hangs · {} moraine · {} drumlins · {} esker cells · {} lakes/{} chains · {} loess cells · {} outwash cells",
        t.elapsed().as_millis(), ice.cirques.len(), ice.hangs.len(),
        ice.moraines.len(), ice.drumlins.len(), ice.eskers.len(),
        ice.proglacial.len(), ice.chains,
        ice.loess.iter().filter(|&&v| v > 0.0).count(),
        ice.outwash.iter().filter(|&&v| v > 0.0).count()
    );

    let water = height.mapv(|h| h < 0.0);
    let t = Instant::now();
    let lat = climate::latitude_deg(size);
    let cont = climate::continentality(&water);
    let tmean = climate::temperature_mean(&height, &lat);
    let tamp = climate::temperature_amplitude(&lat, &cont);
    let (precip, pamp) = climate::precipitation(&height, &water, &tmean, &lat, &cont);
    // M34/M35 — the modern glacier balance rides the climate stage.
    let modern = calliope::ice::modern_glaciers(&water, &tmean, &tamp, &precip, &pamp);
    println!("climate    {:>6} ms", t.elapsed().as_millis());

    let t = Instant::now();
    let hydro = hydrology::hydrology(
        &height, &water, &precip, &pamp, &tmean, &tamp, &ice.outwash, &modern,
    );
    println!("hydrology  {:>6} ms", t.elapsed().as_millis());

    let t = Instant::now();
    // M38 — the biome pass reads the permafrost table depth off the
    // same continentality, exactly as GenBuilder::stage_biomes does.
    let pf = ndarray::Array2::from_shape_fn(tmean.dim(), |(y, x)| {
        calliope::permafrost::extent_class(tmean[[y, x]], cont[[y, x]])
    });
    let biome_map = biomes_mod::classify(&height, &tmean, &tamp, &precip, &hydro.lakes, &pf);
    println!("biomes     {:>6} ms", t.elapsed().as_millis());

    let t = Instant::now();
    let _fert = agriculture::fertility(&height, &tmean, &precip, &hydro.rivers, &hydro.lakes, &hydro.discharge, &ice.till, &ice.loess, &ice.outwash);
    println!("fertility  {:>6} ms", t.elapsed().as_millis());

    // E3.2 — the human stages read the world's resting f32 grids.
    let height32 = height.mapv(|x| x as f32);
    let tmean32 = tmean.mapv(|x| x as f32);
    let precip32 = precip.mapv(|x| x as f32);
    let discharge32 = hydro.discharge.mapv(|x| x as f32);

    let t = Instant::now();
    // M25/M26 — naming reads the sea-level history for the coastal
    // landform names; the stage bench regenerates it the same way the
    // GenBuilder does.
    let sealevel = calliope::sealevel::generate(seed, height.dim().0.max(height.dim().1));
    let (features, world_name) = naming::name_features(
        &height32, &sealevel, &ice, &biome_map, &hydro.rivers, &hydro.lakes, &discharge32, &tmean32, &precip32, seed,
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
