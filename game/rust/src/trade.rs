//! Trade — port of trade.py: goods, A* routes over a terrain-cost grid.

use std::cmp::Ordering;
use std::collections::{BTreeSet, BinaryHeap, HashMap};

use ndarray::Array2;
use serde::Serialize;

use crate::constants as gc;
use crate::hydrology::{DIST, N8};
use crate::resources::{abundance, Deposit};
use crate::settlements::{territory_radius, Settlement};

fn rarity_w(ab: &str) -> f64 {
    match ab {
        "uncommon" => 1.6,
        "rare" => 2.4,
        "legendary" => 4.0,
        _ => 1.0,
    }
}

#[derive(Serialize, Clone)]
pub struct Route {
    pub a: i64,
    pub b: i64,
    pub path: Vec<[i64; 2]>,
    pub w: f64,
    pub goods: Vec<Option<String>>,
}

/// Work out what a settlement produces from its hinterland.
pub fn goods_for(s: &mut Settlement, deposits: &[Deposit], fertility: &Array2<f64>) {
    let r = territory_radius(s.pop) * 1.8;
    let r2 = r * r;
    let mut near: Vec<&Deposit> = deposits
        .iter()
        .filter(|d| {
            let dx = (d.x - s.x) as f64;
            let dy = (d.y - s.y) as f64;
            dx * dx + dy * dy <= r2
        })
        .collect();
    near.sort_by(|a, b| {
        let ka = a.rich * rarity_w(abundance(&a.r));
        let kb = b.rich * rarity_w(abundance(&b.r));
        kb.partial_cmp(&ka).unwrap()
    });
    let mut goods: Vec<String> = Vec::new();
    for d in near {
        if !goods.contains(&d.r) {
            goods.push(d.r.clone());
        }
    }
    let fert = fertility[[s.y as usize, s.x as usize]];
    if fert > 0.45 && !goods.iter().any(|g| g == "grain") {
        let pos = if fert > 0.7 { 0 } else { 1.min(goods.len()) };
        goods.insert(pos, "grain".to_string());
    }
    if goods.is_empty() {
        goods = if s.coastal {
            vec!["fish".to_string()]
        } else {
            vec!["grain".to_string()]
        };
    }
    goods.truncate(6);
    s.exports = Some(goods[0].clone());
    s.goods = goods;
}

pub fn assign_goods(settlements: &mut [Settlement], deposits: &[Deposit], fertility: &Array2<f64>) {
    for s in settlements.iter_mut() {
        goods_for(s, deposits, fertility);
    }
}

// --- routing -------------------------------------------------------------

pub fn cost_grid(
    height: &Array2<f64>,
    rivers: &Array2<bool>,
    lakes: &Array2<bool>,
    biomes: &Array2<u8>,
) -> Array2<f64> {
    let size = height.dim().0;
    let hpos = height.mapv(|h| h.max(0.0));
    let (gy, gx) = crate::ndimage::gradient(&hpos);
    Array2::from_shape_fn((size, size), |(y, x)| {
        if height[[y, x]] < 0.0 {
            return 0.8; // sea lanes are cheap
        }
        let slope = gy[[y, x]].hypot(gx[[y, x]]) * size as f64 / 8.0;
        let mut cost = 1.0 + 6.0 * slope.clamp(0.0, 1.2);
        if rivers[[y, x]] {
            cost += 0.8; // fording
        }
        if lakes[[y, x]] {
            cost += 4.0;
        }
        let b = biomes[[y, x]];
        cost += 1.0 * ((b == gc::DESERT) as u8 as f64)
            + 2.5 * ((b == gc::ICE) as u8 as f64)
            + 0.6 * ((b == gc::TUNDRA) as u8 as f64);
        cost
    })
}

pub fn downsample(a: &Array2<f64>, f: usize) -> Array2<f64> {
    let size = a.dim().0;
    let s = size / f;
    Array2::from_shape_fn((s, s), |(y, x)| {
        let mut sum = 0.0;
        for dy in 0..f {
            for dx in 0..f {
                sum += a[[y * f + dy, x * f + dx]];
            }
        }
        sum / (f * f) as f64
    })
}

/// Min-heap item ordered like Python's heapq tuples (f, g, y, x).
struct AItem(f64, f64, usize, usize);

impl PartialEq for AItem {
    fn eq(&self, o: &Self) -> bool {
        self.0 == o.0 && self.1 == o.1 && self.2 == o.2 && self.3 == o.3
    }
}
impl Eq for AItem {}
impl PartialOrd for AItem {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for AItem {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0
            .partial_cmp(&self.0)
            .unwrap()
            .then_with(|| o.1.partial_cmp(&self.1).unwrap())
            .then_with(|| o.2.cmp(&self.2))
            .then_with(|| o.3.cmp(&self.3))
    }
}

pub fn astar(
    cost: &Array2<f64>,
    start: (usize, usize),
    goal: (usize, usize),
) -> Option<Vec<(usize, usize)>> {
    let (hh, ww) = cost.dim();
    let (sy, sx) = start;
    let (gy, gx) = goal;
    let min_cost = 0.75;
    let max_expand = 200_000usize;
    let mut best = Array2::<f64>::from_elem((hh, ww), f64::INFINITY);
    best[[sy, sx]] = 0.0;
    let mut came: HashMap<(usize, usize), (usize, usize)> = HashMap::new();
    let h0 = min_cost * ((gy as f64 - sy as f64).hypot(gx as f64 - sx as f64));
    let mut heap: BinaryHeap<AItem> = BinaryHeap::new();
    heap.push(AItem(h0, 0.0, sy, sx));
    let mut expanded = 0usize;

    while let Some(AItem(_, g, y, x)) = heap.pop() {
        if g > best[[y, x]] {
            continue;
        }
        if y == gy && x == gx {
            let mut path = vec![(y, x)];
            let (mut cy, mut cx) = (y, x);
            while let Some(&(py, px)) = came.get(&(cy, cx)) {
                cy = py;
                cx = px;
                path.push((cy, cx));
            }
            path.reverse();
            return Some(path);
        }
        expanded += 1;
        if expanded > max_expand {
            return None;
        }
        for (&(dy, dx), &dist) in N8.iter().zip(DIST.iter()) {
            let ny = y as isize + dy;
            let nx = x as isize + dx;
            if ny < 0 || nx < 0 || ny >= hh as isize || nx >= ww as isize {
                continue;
            }
            let (ny, nx) = (ny as usize, nx as usize);
            let ng = g + dist * 0.5 * (cost[[y, x]] + cost[[ny, nx]]);
            if ng < best[[ny, nx]] {
                best[[ny, nx]] = ng;
                came.insert((ny, nx), (y, x));
                let f = ng + min_cost * ((gy as f64 - ny as f64).hypot(gx as f64 - nx as f64));
                heap.push(AItem(f, ng, ny, nx));
            }
        }
    }
    None
}

pub fn route_entry(sa: &Settlement, sb: &Settlement, path: &[(usize, usize)], f: usize) -> Route {
    let mut pts: Vec<[i64; 2]> = path
        .iter()
        .map(|&(y, x)| [(x * f + f / 2) as i64, (y * f + f / 2) as i64])
        .collect();
    pts[0] = [sa.x, sa.y];
    let last = pts.len() - 1;
    pts[last] = [sb.x, sb.y];
    if pts.len() > 3 {
        let mut thinned = vec![pts[0]];
        let mut i = 1;
        while i < pts.len() - 1 {
            thinned.push(pts[i]);
            i += 2;
        }
        thinned.push(pts[pts.len() - 1]);
        pts = thinned;
    }
    let w = crate::util::round2(
        (0.5 + (((sa.pop + sb.pop) as f64).log10() - 2.0) * 0.6).clamp(0.5, 2.0),
    );
    Route {
        a: sa.id,
        b: sb.id,
        path: pts,
        w,
        goods: vec![sa.exports.clone(), sb.exports.clone()],
    }
}

pub fn recount_connections(settlements: &mut [Settlement], routes: &[Route]) {
    let mut conn: HashMap<i64, i64> = settlements.iter().map(|s| (s.id, 0)).collect();
    for r in routes {
        *conn.entry(r.a).or_insert(0) += 1;
        *conn.entry(r.b).or_insert(0) += 1;
    }
    for s in settlements.iter_mut() {
        s.connections = *conn.get(&s.id).unwrap_or(&0);
    }
}

/// Candidate pairs: each settlement to its 2 nearest neighbours.
pub fn build_routes(
    cost_ds: &Array2<f64>,
    f: usize,
    settlements: &mut [Settlement],
) -> Vec<Route> {
    let mut pairs: BTreeSet<(i64, i64)> = BTreeSet::new();
    for s in settlements.iter() {
        let mut others: Vec<&Settlement> =
            settlements.iter().filter(|o| o.id != s.id).collect();
        others.sort_by(|a, b| {
            let da = (a.x - s.x).pow(2) + (a.y - s.y).pow(2);
            let db = (b.x - s.x).pow(2) + (b.y - s.y).pow(2);
            da.cmp(&db)
        });
        for o in others.iter().take(2) {
            pairs.insert((s.id.min(o.id), s.id.max(o.id)));
        }
    }

    let by_id: HashMap<i64, usize> = settlements
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id, i))
        .collect();
    let mut routes: Vec<Route> = Vec::new();
    for &(a, b) in pairs.iter() {
        let sa = &settlements[by_id[&a]];
        let sb = &settlements[by_id[&b]];
        if let Some(path) = astar(
            cost_ds,
            (sa.y as usize / f, sa.x as usize / f),
            (sb.y as usize / f, sb.x as usize / f),
        ) {
            routes.push(route_entry(sa, sb, &path, f));
        }
    }
    recount_connections(settlements, &routes);
    routes
}

/// Link a newly founded settlement (at `idx`) into the route network.
pub fn connect_settlement(
    idx: usize,
    settlements: &mut [Settlement],
    routes: &mut Vec<Route>,
    cost_ds: &Array2<f64>,
    f: usize,
) {
    let s = settlements[idx].clone();
    let mut others: Vec<&Settlement> = settlements.iter().filter(|o| o.id != s.id).collect();
    others.sort_by(|a, b| {
        let da = (a.x - s.x).pow(2) + (a.y - s.y).pow(2);
        let db = (b.x - s.x).pow(2) + (b.y - s.y).pow(2);
        da.cmp(&db)
    });
    let mut new_routes: Vec<Route> = Vec::new();
    for o in others.iter().take(2) {
        if let Some(path) = astar(
            cost_ds,
            (s.y as usize / f, s.x as usize / f),
            (o.y as usize / f, o.x as usize / f),
        ) {
            new_routes.push(route_entry(&s, o, &path, f));
        }
    }
    routes.extend(new_routes);
    recount_connections(settlements, routes);
}
