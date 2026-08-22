//! The lattice speaks with one voice — M66.
//!
//! Every module that walks the grid used to carry its own copy of the
//! walk: offset tables, steepest-descent picks, drainage-order sorts,
//! accumulation loops. Era I's recast (ADR-0026) moves the shared law
//! here, exactly as it stood in `hydrology` — same neighbour order,
//! same first-wins tie-break, same high-to-low index-tied drainage
//! sort — because the law's *order* is load-bearing: float sums walk
//! it, and `hash_state` remembers every step. Change the order and
//! every world since M16 becomes a different world.
//!
//! `hydrology` re-exports everything here, so the historical
//! `hydrology::N8` paths remain valid; new code should say `grid::`.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use ndarray::Array2;

/// The canonical 4-neighbourhood, in the order every site already
/// walked it: up, down, left, right.
pub const N4: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

/// The canonical 8-neighbourhood (D8), row-major. Index into this
/// table is the wire meaning of a flow direction — never reorder.
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

/// Step distances matching `N8` index-for-index.
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
                for (dy, dx) in N4 {
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
/// Tie-break is first-wins in N8 order — part of the lattice law.
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
/// The comparator is a total order (index tie-break), so the unstable
/// sort returns the identical permutation without the stable sort's
/// half-array scratch allocation (E5.11).
pub fn drainage_order(filled: &Array2<f64>) -> Vec<usize> {
    let size = filled.dim().0;
    let mut order: Vec<usize> = (0..size * size).collect();
    order.sort_unstable_by(|&a, &b| {
        let fa = filled[[a / size, a % size]];
        let fb = filled[[b / size, b % size]];
        fb.partial_cmp(&fa).unwrap().then(a.cmp(&b)) // high to low
    });
    order
}

/// Accumulate a per-cell weight down the drainage tree. The float sum
/// walks `order` exactly — donors before receivers — so the addition
/// order, and therefore the bits, are fixed by the drainage sort.
pub fn accumulate(
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
