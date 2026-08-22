use calliope::world::World;
fn main() {
    let w = World::generate(12345, 512);
    std::fs::write("/tmp/pack.bin", w.pack()).unwrap();
    let s = |a: &ndarray::Array2<u8>| a.iter().map(|&v| v as u64).sum::<u64>();
    println!("biomes {} crops {} strahler {} flags {} rock {} soil {} landform {} coastform {}",
        s(&w.fields.biomes), s(&w.fields.crops), s(&w.fields.strahler), s(&w.fields.flags),
        s(&w.fields.rock), s(&w.fields.soil), s(&w.fields.landform), s(&w.fields.coastform));
    let t: i64 = w.fields.territory.iter().map(|&v| v as i64).sum();
    println!("territory {}", t);
}
