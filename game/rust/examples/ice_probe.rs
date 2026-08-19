// throwaway M29 calibration probe — deleted after tuning
use calliope::{geo, plates, erosion, hydrology};
use ndarray::Array2;

fn main() {
    let seed = 12345i64;
    let size = 512usize;
    let pl = plates::generate(seed, size);
    let mut h = geo::heightmap(seed, size, &pl);
    erosion::erode(&mut h);
    let ice = calliope::ice::compute(seed, &h);
    let water = h.mapv(|v| v < 0.0);
    let filled = hydrology::fill_depressions(&h, &water);
    let dirs = hydrology::flow_directions(&filled, &water);
    let ones = Array2::from_elem(h.dim(), 1000.0);
    let acc = hydrology::flow_accumulation(&filled, &dirs, &ones, &water);
    let (rows, cols) = h.dim();
    let n = rows as f64;
    let mut cand = 0usize; // alpine iced land, acc<=3
    let mut band = 0usize; // + in ELA window
    let mut walls: Vec<f64> = Vec::new();
    for y in 0..rows {
        let lat = (-90.0 + (y as f64) * 180.0 / (n - 1.0)).abs();
        if !(40.0..62.0).contains(&lat) { continue; }
        let e = ice.ela_row[y];
        for x in 0..cols {
            if ice.thickness[[y, x]] <= 0.0 || water[[y, x]] || acc[[y, x]] > 3.0 { continue; }
            cand += 1;
            let hv = h[[y, x]];
            if hv < e - 0.02 || hv > e + 0.10 { continue; }
            band += 1;
            let mut best = f64::MIN;
            for (dy, dx) in [(-1isize,-1isize),(-1,0),(-1,1),(0,-1),(0,1),(1,-1),(1,0),(1,1)] {
                let ny = y as isize + dy; let nx = x as isize + dx;
                if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize { continue; }
                let d = h[[ny as usize, nx as usize]] - hv;
                if d > best { best = d; }
            }
            walls.push(best);
        }
    }
    walls.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pct = |p: f64| walls[((walls.len() - 1) as f64 * p) as usize];
    println!("alpine iced headwater cells: {cand} · in ELA band: {band}");
    if !walls.is_empty() {
        println!("wall (max N8 rise) p50 {:.4} · p80 {:.4} · p90 {:.4} · p95 {:.4} · p99 {:.4} · max {:.4}",
            pct(0.5), pct(0.8), pct(0.9), pct(0.95), pct(0.99), walls[walls.len()-1]);
        for t in [0.01, 0.02, 0.03, 0.04, 0.06] {
            println!("wall >= {:.2}: {}", t, walls.iter().filter(|&&w| w >= t).count());
        }
    }
    // hang side: distribution of trunk/trib cut ratios needs the carve — count junction geometry only
    let mut juncs = 0usize;
    let mut diffs: Vec<f64> = Vec::new();
    for y in 0..rows {
        for x in 0..cols {
            if ice.thickness[[y,x]] <= 0.0 || water[[y,x]] { continue; }
            let a = acc[[y,x]];
            if a < 24.0 || a >= 240.0 { continue; }
            let d = dirs[[y,x]];
            if d < 0 { continue; }
            let (dy, dx) = hydrology::N8[d as usize];
            let ny = y as isize + dy; let nx = x as isize + dx;
            if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize { continue; }
            let at = acc[[ny as usize, nx as usize]];
            if at < 4.0 * a { continue; }
            juncs += 1;
            // centerline depth law from ice::carve, both sides
            let tf_a = ice.thickness[[y, x]] as f64 / 4000.0;
            let tf_t = ice.thickness[[ny as usize, nx as usize]] as f64 / 4000.0;
            let af_a = (a / 240.0).sqrt().min(1.0);
            let af_t = (at / 240.0).sqrt().min(1.0);
            let da = 0.055 * tf_a * (0.35 + 0.65 * af_a);
            let dt = 0.055 * tf_t * (0.35 + 0.65 * af_t);
            diffs.push(dt - da);
        }
    }
    diffs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("iced junctions with 4x trunk: {juncs}");
    if !diffs.is_empty() {
        let pd = |p: f64| diffs[((diffs.len() - 1) as f64 * p) as usize];
        println!("centerline depth diff p10 {:.5} p50 {:.5} p90 {:.5} max {:.5}", pd(0.1), pd(0.5), pd(0.9), diffs[diffs.len()-1]);
        for t in [0.0005, 0.001, 0.002, 0.004, 0.008] {
            println!("diff >= {:.4}: {}", t, diffs.iter().filter(|&&d| d >= t).count());
        }
    }
}
