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

    let w = World::generate(seed, size);

    let mut n = 0u64;
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    let (mut y1, mut y2) = (0.0f64, 0.0f64);
    let mut d_below = 0u64;
    let mut z_below = 0u64;

    // lag autocorrelation of the single-year SPI over the same sample
    let lags = 12usize;
    let mut lag_num = vec![0.0f64; lags];
    let mut lag_n = vec![0u64; lags];

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

    // rho(lag) from the same field
    for y in (0..size).step_by(step) {
        for x in (0..size).step_by(step) {
            if w.fields.height[[y, x]] < 0.0 { continue; }
            let zs: Vec<f64> = (0..years).map(|yr| w.year_spi(yr, y, x)).collect();
            let m: f64 = zs.iter().sum::<f64>() / zs.len() as f64;
            for l in 0..lags {
                for t in l..zs.len() {
                    lag_num[l] += (zs[t] - m) * (zs[t - l] - m);
                    lag_n[l] += 1;
                }
            }
        }
    }
    let c0 = lag_num[0] / lag_n[0] as f64;
    let mut rho = vec![0.0f64; lags];
    for l in 0..lags {
        rho[l] = (lag_num[l] / lag_n[l] as f64) / c0;
        println!("rho[{l:2}] = {:+.4}", rho[l]);
    }
    // exact variance of the weighted sum implied by rho
    let mem = 0.5f64;
    let mut v = 0.0f64;
    for j in 0..lags { for k in 0..lags {
        v += mem.powi(j as i32) * mem.powi(k as i32) * rho[(j as i64 - k as i64).abs() as usize];
    }}
    println!("implied Var(sum)/Var(z) = {v:.6}   -> exact NORM = {:.9}", 1.0 / v.sqrt());
}
