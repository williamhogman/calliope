//! Scratch measurement (M80 diagnosis, not part of the suite): is the
//! memory-accumulated drought index actually unit-variance the way
//! `drought::NORM` claims? The renormalization sqrt(1 − MEM²) is exact
//! only for *independent* years; the M74/M76 sky is quasi-periodic, so
//! the true variance is an empirical question.

use calliope::world::World;

fn main() {
    let mut args = std::env::args().skip(1);
    let seed: i64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(12345);
    let size: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(512);
    let years: i64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(300);

    let w = World::new(seed, size);

    let mut n = 0u64;
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    let (mut y1, mut y2) = (0.0f64, 0.0f64);
    let mut d_below = 0u64;
    let mut z_below = 0u64;

    let step = 16usize;
    for year in 0..years {
        for y in (0..size).step_by(step) {
            for x in (0..size).step_by(step) {
                if w.fields.height[[y, x]] < 0.0 {
                    continue;
                }
                let d = w.drought_index(year, y, x);
                let z = w.year_spi(year, y, x);
                n += 1;
                s1 += d;
                s2 += d * d;
                y1 += z;
                y2 += z * z;
                if d <= -1.0 {
                    d_below += 1;
                }
                if z <= -1.0 {
                    z_below += 1;
                }
            }
        }
    }
    let nf = n as f64;
    let dm = s1 / nf;
    let dv = s2 / nf - dm * dm;
    let zm = y1 / nf;
    let zv = y2 / nf - zm * zm;
    println!("seed {seed} · size {size} · {years}y · samples {n}");
    println!("single-year SPI : mean {zm:+.4}  var {zv:.4}  sd {:.4}  P(z<=-1) {:.4}", zv.sqrt(), z_below as f64 / nf);
    println!("drought index D : mean {dm:+.4}  var {dv:.4}  sd {:.4}  P(D<=-1) {:.4}", dv.sqrt(), d_below as f64 / nf);
    println!("ratio sd(D)/sd(z) = {:.4}   (M80 contract: 1.000)", dv.sqrt() / zv.sqrt());
}
