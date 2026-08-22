use calliope::pack::validate_pack;
use calliope::world::World;
fn main() {
    for seed in [12345i64, 777, 90210] {
        let w = World::generate(seed, 512);
        let b = w.pack();
        let cells = (w.width * w.size) as f64;
        let v = validate_pack(&b).expect("pack validates");
        println!("seed {seed}: {} B total · {:.2} B/cell · arrays {} blob {}", b.len(), b.len() as f64 / cells, v.0, v.1);
    }
}
