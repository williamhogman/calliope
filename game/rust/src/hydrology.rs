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

/// Accumulate precip downstream; returns discharge (precip-weighted area).
pub fn flow_accumulation(
    filled: &Array2<f64>,
    dirs: &Array2<i8>,
    precip: &Array2<f64>,
    water: &Array2<bool>,
) -> Array2<f64> {
    let size = filled.dim().0;
    let mut acc = Array2::from_shape_fn((size, size), |(y, x)| {
        if water[[y, x]] {
            0.0
        } else {
            precip[[y, x]] / 1000.0
        }
    });
    let mut order: Vec<usize> = (0..size * size).collect();
    order.sort_by(|&a, &b| {
        let fa = filled[[a / size, a % size]];
        let fb = filled[[b / size, b % size]];
        fb.partial_cmp(&fa).unwrap().then(a.cmp(&b)) // high to low
    });
    for idx in order {
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

pub struct Hydrology {
    pub filled: Array2<f64>,
    pub discharge: Array2<f64>,
    pub rivers: Array2<bool>,
    pub lakes: Array2<bool>,
}

pub fn hydrology(height: &Array2<f64>, water: &Array2<bool>, precip: &Array2<f64>) -> Hydrology {
    let filled = fill_depressions(height, water);
    let dirs = flow_directions(&filled, water);
    let discharge = flow_accumulation(&filled, &dirs, precip, water);

    let size = height.dim().0;
    // 60.0 keeps the great rivers and their major tributaries (~4% of land)
    // and prunes the minor-stream fuzz the 4 km cells can't honestly carry.
    let river_threshold = 60.0;
    let mut lakes = Array2::<bool>::from_elem((size, size), false);
    let mut rivers = Array2::<bool>::from_elem((size, size), false);
    for y in 0..size {
        for x in 0..size {
            let lake = !water[[y, x]] && (filled[[y, x]] - height[[y, x]] > 0.004);
            lakes[[y, x]] = lake;
            rivers[[y, x]] = !water[[y, x]] && !lake && discharge[[y, x]] > river_threshold;
        }
    }
    Hydrology {
        filled,
        discharge,
        rivers,
        lakes,
    }
}
