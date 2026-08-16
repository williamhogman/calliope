//! Hydrology — port of hydrology.py: priority-flood fill, D8 routing, rivers.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use ndarray::Array2;

pub const N8: [(isize, isize); 8] = [
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, -1),
    (0, 1),
    (1, -1),
    (1, 0),
    (1, 1),
];
pub const DIST: [f64; 8] = [
    1.4142135, 1.0, 1.4142135, 1.0, 1.0, 1.4142135, 1.0, 1.4142135,
];

/// Min-heap item ordered like Python's heapq tuples (h, y, x).
struct HeapItem(f64, usize, usize);

impl PartialEq for HeapItem {
    fn eq(&self, o: &Self) -> bool {
        self.0 == o.0 && self.1 == o.1 && self.2 == o.2
    }
}
impl Eq for HeapItem {}
impl PartialOrd for HeapItem {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for HeapItem {
    fn cmp(&self, o: &Self) -> Ordering {
        // reversed: BinaryHeap is a max-heap, we want the smallest first
        o.0
            .partial_cmp(&self.0)
            .unwrap()
            .then_with(|| o.1.cmp(&self.1))
            .then_with(|| o.2.cmp(&self.2))
    }
}

/// Priority-flood fill so every land cell drains to the ocean or map edge.
pub fn fill_depressions(height: &Array2<f64>, water: &Array2<bool>) -> Array2<f64> {
    let eps = 1e-5;
    let size = height.dim().0;
    let mut filled = height.clone();
    let mut visited = water.clone();
    let mut heap: BinaryHeap<HeapItem> = BinaryHeap::new();

    // seeds: land on the border, and land adjacent to water
    for y in 0..size {
        for x in 0..size {
            if water[[y, x]] {
                continue;
            }
            let border = y == 0 || y == size - 1 || x == 0 || x == size - 1;
            let mut adj = false;
            if !border {
                adj = water[[y - 1, x]] || water[[y + 1, x]] || water[[y, x - 1]]
                    || water[[y, x + 1]];
            } else {
                for (dy, dx) in [(-1isize, 0isize), (1, 0), (0, -1), (0, 1)] {
                    let ny = y as isize + dy;
                    let nx = x as isize + dx;
                    if ny >= 0 && nx >= 0 && ny < size as isize && nx < size as isize {
                        adj |= water[[ny as usize, nx as usize]];
                    }
                }
            }
            if border || adj {
                heap.push(HeapItem(filled[[y, x]], y, x));
                visited[[y, x]] = true;
            }
        }
    }

    while let Some(HeapItem(hcur, y, x)) = heap.pop() {
        for &(dy, dx) in N8.iter() {
            let ny = y as isize + dy;
            let nx = x as isize + dx;
            if ny < 0 || nx < 0 || ny >= size as isize || nx >= size as isize {
                continue;
            }
            let (ny, nx) = (ny as usize, nx as usize);
            if visited[[ny, nx]] {
                continue;
            }
            visited[[ny, nx]] = true;
            let mut nh = filled[[ny, nx]];
            if nh <= hcur {
                nh = hcur + eps;
                filled[[ny, nx]] = nh;
            }
            heap.push(HeapItem(nh, ny, nx));
        }
    }
    filled
}

/// D8: index 0..7 into N8 of the steepest downslope neighbour, -1 = terminal.
pub fn flow_directions(filled: &Array2<f64>, water: &Array2<bool>) -> Array2<i8> {
    let size = filled.dim().0;
    Array2::from_shape_fn((size, size), |(y, x)| {
        if water[[y, x]] {
            return -1i8;
        }
        let mut best_drop = 0.0f64;
        let mut best_dir = -1i8;
        for (i, (&(dy, dx), &dist)) in N8.iter().zip(DIST.iter()).enumerate() {
            let ny = y as isize + dy;
            let nx = x as isize + dx;
            if ny < 0 || nx < 0 || ny >= size as isize || nx >= size as isize {
                continue;
            }
            let drop = (filled[[y, x]] - filled[[ny as usize, nx as usize]]) / dist;
            if drop > best_drop {
                best_drop = drop;
                best_dir = i as i8;
            }
        }
        best_dir
    })
}

/// Cells sorted by filled height, high to low — donors before receivers.
pub fn drainage_order(filled: &Array2<f64>) -> Vec<usize> {
    let size = filled.dim().0;
    let mut order: Vec<usize> = (0..size * size).collect();
    order.sort_by(|&a, &b| {
        let fa = filled[[a / size, a % size]];
        let fb = filled[[b / size, b % size]];
        fb.partial_cmp(&fa).unwrap().then(a.cmp(&b)) // high to low
    });
    order
}

/// Accumulate a per-cell weight down the drainage tree.
fn accumulate(
    order: &[usize],
    dirs: &Array2<i8>,
    weight: impl Fn(usize, usize) -> f64,
    size: usize,
) -> Array2<f64> {
    let mut acc = Array2::from_shape_fn((size, size), |(y, x)| weight(y, x));
    for &idx in order {
        let (y, x) = (idx / size, idx % size);
        let d = dirs[[y, x]];
        if d >= 0 {
            let (dy, dx) = N8[d as usize];
            let ny = y as isize + dy;
            let nx = x as isize + dx;
            if ny >= 0 && nx >= 0 && ny < size as isize && nx < size as isize {
                let v = acc[[y, x]];
                acc[[ny as usize, nx as usize]] += v;
            }
        }
    }
    acc
}

/// Accumulate precip downstream; returns discharge (precip-weighted area).
pub fn flow_accumulation(
    filled: &Array2<f64>,
    dirs: &Array2<i8>,
    precip: &Array2<f64>,
    water: &Array2<bool>,
) -> Array2<f64> {
    let size = filled.dim().0;
    let order = drainage_order(filled);
    accumulate(
        &order,
        dirs,
        |y, x| if water[[y, x]] { 0.0 } else { precip[[y, x]] / 1000.0 },
        size,
    )
}

pub struct Hydrology {
    pub filled: Array2<f64>,
    pub dirs: Array2<i8>,
    pub discharge: Array2<f64>,
    pub rivers: Array2<bool>,
    pub lakes: Array2<bool>,
    /// Endorheic basins: lakes with no road to the sea, crusted white.
    pub salt: Array2<bool>,
    /// Rivers that fail their threshold in the dry season — wadis.
    pub seasonal: Array2<bool>,
    /// Strahler stream order, 0 for non-river cells.
    pub strahler: Array2<u8>,
    /// Signed seasonal discharge swing, -1..1 (positive peaks month 0).
    pub flow_amp: Array2<f64>,
}

// 60.0 keeps the great rivers and their major tributaries (~4% of land)
// and prunes the minor-stream fuzz the 4 km cells can't honestly carry.
pub const RIVER_THRESHOLD: f64 = 60.0;

pub fn hydrology(
    height: &Array2<f64>,
    water: &Array2<bool>,
    precip: &Array2<f64>,
    pamp: &Array2<f64>,
    tmean: &Array2<f64>,
) -> Hydrology {
    let size = height.dim().0;
    let filled = fill_depressions(height, water);
    let mut dirs = flow_directions(&filled, water);
    let order = drainage_order(&filled);

    let mut lakes = Array2::<bool>::from_elem((size, size), false);
    for y in 0..size {
        for x in 0..size {
            lakes[[y, x]] = !water[[y, x]] && (filled[[y, x]] - height[[y, x]] > 0.004);
        }
    }

    // --- endorheic basins: in dry warm country, a lake evaporates what
    // its rivers bring and never reaches the sea. Flow terminates in the
    // basin floor; downstream of the ghost-spill the channel runs dry.
    let mut salt = Array2::<bool>::from_elem((size, size), false);
    let mut seen = Array2::<bool>::from_elem((size, size), false);
    let mut any_terminal = false;
    for y in 0..size {
        for x in 0..size {
            if !lakes[[y, x]] || seen[[y, x]] {
                continue;
            }
            // flood-fill this lake component (4-connectivity, scan order)
            let mut comp = vec![(y, x)];
            seen[[y, x]] = true;
            let mut qi = 0usize;
            while qi < comp.len() {
                let (cy, cx) = comp[qi];
                qi += 1;
                for (dy, dx) in [(-1isize, 0isize), (1, 0), (0, -1), (0, 1)] {
                    let ny = cy as isize + dy;
                    let nx = cx as isize + dx;
                    if ny < 0 || nx < 0 || ny >= size as isize || nx >= size as isize {
                        continue;
                    }
                    let (ny, nx) = (ny as usize, nx as usize);
                    if lakes[[ny, nx]] && !seen[[ny, nx]] {
                        seen[[ny, nx]] = true;
                        comp.push((ny, nx));
                    }
                }
            }
            let m = comp.len() as f64;
            let p_mean: f64 = comp.iter().map(|&(a, b)| precip[[a, b]]).sum::<f64>() / m;
            let t_mean: f64 = comp.iter().map(|&(a, b)| tmean[[a, b]]).sum::<f64>() / m;
            let d_mean: f64 =
                comp.iter().map(|&(a, b)| filled[[a, b]] - height[[a, b]]).sum::<f64>() / m;
            if comp.len() >= 2 && p_mean < 520.0 && t_mean > 8.0 && d_mean > 0.006 {
                any_terminal = true;
                for &(a, b) in &comp {
                    salt[[a, b]] = true;
                    dirs[[a, b]] = -1; // the water stops here and rises as haze
                }
            }
        }
    }

    let w_precip = |y: usize, x: usize| {
        if water[[y, x]] {
            0.0
        } else {
            precip[[y, x]] / 1000.0
        }
    };
    let discharge = accumulate(&order, &dirs, w_precip, size);
    // second accumulation, weighted by the signed seasonal share: the
    // ratio to total discharge says how hard each river breathes.
    let acc_season = accumulate(
        &order,
        &dirs,
        |y, x| {
            if water[[y, x]] {
                0.0
            } else {
                precip[[y, x]] * pamp[[y, x]] / 1000.0
            }
        },
        size,
    );
    let _ = any_terminal;

    let mut rivers = Array2::<bool>::from_elem((size, size), false);
    for y in 0..size {
        for x in 0..size {
            rivers[[y, x]] = !water[[y, x]]
                && !lakes[[y, x]]
                && discharge[[y, x]] > RIVER_THRESHOLD;
        }
    }

    // --- Strahler order over the whole drainage net: every land cell
    // carries a rill of order 1, and a stream steps up only where two
    // branches of its own order meet. The visible rivers then wear the
    // true orders of their basins — mainstems come out 6th, 7th order.
    let mut strahler = Array2::<u8>::from_elem((size, size), 0);
    for &idx in &order {
        let (y, x) = (idx / size, idx % size);
        if water[[y, x]] {
            continue;
        }
        let mut top = 0u8;
        let mut top_n = 0usize;
        for (i, &(dy, dx)) in N8.iter().enumerate() {
            let ny = y as isize + dy;
            let nx = x as isize + dx;
            if ny < 0 || nx < 0 || ny >= size as isize || nx >= size as isize {
                continue;
            }
            let (ny, nx) = (ny as usize, nx as usize);
            if water[[ny, nx]] {
                continue;
            }
            // does this neighbour flow into us?
            let d = dirs[[ny, nx]];
            if d < 0 {
                continue;
            }
            let (ddy, ddx) = N8[d as usize];
            if (ny as isize + ddy, nx as isize + ddx) != (y as isize, x as isize) {
                continue;
            }
            let o = strahler[[ny, nx]];
            if o > top {
                top = o;
                top_n = 1;
            } else if o == top && o > 0 {
                top_n += 1;
            }
            let _ = i;
        }
        strahler[[y, x]] = if top == 0 {
            1
        } else if top_n >= 2 {
            (top + 1).min(12)
        } else {
            top
        };
    }

    // --- seasonal swing and wadis
    let mut flow_amp = Array2::<f64>::zeros((size, size));
    let mut seasonal = Array2::<bool>::from_elem((size, size), false);
    for y in 0..size {
        for x in 0..size {
            if discharge[[y, x]] > 1e-9 {
                flow_amp[[y, x]] = (acc_season[[y, x]] / discharge[[y, x]]).clamp(-1.0, 1.0);
            }
            if rivers[[y, x]] {
                // at low water the year's rain leans away: a channel that
                // no longer clears the river bar is a wadi, full half the
                // year and a ribbon of cracked mud the other half.
                let dry = discharge[[y, x]] * (1.0 - flow_amp[[y, x]].abs());
                seasonal[[y, x]] = dry < RIVER_THRESHOLD;
            }
        }
    }

    Hydrology {
        filled,
        dirs,
        discharge,
        rivers,
        lakes,
        salt,
        seasonal,
        strahler,
        flow_amp,
    }
}
