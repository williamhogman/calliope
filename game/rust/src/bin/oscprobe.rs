fn main() {
    let n = 200000;
    for seed in [12345i64, 777, 31337] {
        let p = calliope::noisegen::Perlin3::new(seed + 9311);
        let mut s = 0.0; let mut s2 = 0.0;
        for m in 0..n { let v = p.fbm(m as f64 * calliope::oscillation::OSC_NOISE_STEP, 0.5, 0.5, 2); s += v; s2 += v*v; }
        let mean = s / n as f64;
        println!("seed {} noise mean {:.5} sigma {:.5}", seed, mean, (s2/n as f64 - mean*mean).sqrt());
    }
}
