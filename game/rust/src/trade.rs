//! Trade — geography-priced commerce. The sea is cheap and the land is
//! dear: a laden cart pays for every climb, every ford, every mile of
//! sand and snow, while a ship on a coastal run carries the same cargo
//! for a fraction. A* works over that truth, so roads thread the
//! passes, lanes hug the shore, barges ride the great rivers, and
//! harbours grow wherever cargo changes from wheel to keel.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap};

use ndarray::Array2;
use serde::Serialize;

use crate::ids::SettlementId;
use crate::world::CellFlags;
use crate::constants as gc;
use crate::hydrology::{DIST, N8};
use crate::ndimage;
use crate::resources::{Abundance, Deposit, Good, Goods};
use crate::settlements::Settlement;

fn rarity_w(ab: Abundance) -> f64 {
    match ab {
        Abundance::Uncommon => 1.6,
        Abundance::Rare => 2.4,
        Abundance::Legendary => 4.0,
        Abundance::Common => 1.0,
    }
}

// Travel modes carried on each route point.
pub const MODE_LAND: u8 = 0;
pub const MODE_SEA: u8 = 1;
pub const MODE_RIVER: u8 = 2;

// The economics of movement (cost per downsampled cell).
pub const COAST_SEA_COST: f64 = 0.35; // cabotage, in sight of land
pub const OPEN_SEA_COST: f64 = 0.55; // blue water: no shelter, no rescue
pub const LAKE_COST: f64 = 0.6; // flat water, short hauls
pub const LAND_BASE: f64 = 3.0; // a cart, an ox, a rutted track
pub const RIVER_BARGE_COST: f64 = 1.1; // a navigable river is a highway
pub const EMBARK_COST: f64 = 10.0; // cranes, porters and harbour dues
pub const NAVIGABLE: f64 = 90.0; // discharge above which barges swim

#[derive(Serialize, Clone)]
pub struct Route {
    pub a: SettlementId,
    pub b: SettlementId,
    pub path: Vec<[i64; 2]>,
    /// travel mode per path point: 0 land, 1 sea, 2 river
    pub m: Vec<u8>,
    pub w: f64,
    /// total terrain cost of the journey
    pub cost: f64,
    /// fraction of the way spent under sail
    pub sea: f64,
    /// signed seasonal swing of the barge legs: high water lifts trade
    pub ramp: f64,
    pub goods: Vec<Option<Good>>,
    /// Disused (M9.4): years without realized flow let the grass grow —
    /// the road stays on the map, drawn faded, a mark history left.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub old: bool,
}

/// Work out what a settlement produces from its hinterland.
pub fn goods_for(s: &mut Settlement, deposits: &[Deposit], fertility: &Array2<f32>) {
    let r = crate::settlements::work_radius(s.pop);
    let r2 = r * r;
    let mut near: Vec<&Deposit> = deposits
        .iter()
        .filter(|d| {
            if !d.known || d.left == 0.0 {
                return false; // unfound seams and dead pits yield nothing
            }
            let dx = (d.x - s.x) as f64;
            let dy = (d.y - s.y) as f64;
            dx * dx + dy * dy <= r2
        })
        .collect();
    near.sort_by(|a, b| {
        let ka = a.rich * rarity_w(a.r.abundance());
        let kb = b.rich * rarity_w(b.r.abundance());
        kb.partial_cmp(&ka).unwrap()
    });
    let mut goods: Goods = Goods::new();
    for d in near {
        if !goods.contains(&d.r) {
            goods.push(d.r);
        }
    }
    let fert = fertility[[s.y as usize, s.x as usize]];
    if fert > 0.45 && !goods.contains(&Good::Grain) {
        let pos = if fert > 0.7 { 0 } else { 1.min(goods.len()) };
        goods.insert(pos, Good::Grain);
    }
    if goods.is_empty() {
        goods.push(if s.coastal { Good::Fish } else { Good::Grain });
    }
    goods.truncate(6);
    s.exports = Some(goods[0]);
    s.goods = goods;
}

pub fn assign_goods(settlements: &mut [Settlement], deposits: &[Deposit], fertility: &Array2<f32>) {
    for s in settlements.iter_mut() {
        goods_for(s, deposits, fertility);
    }
}

// --- the price of the land ------------------------------------------------

fn smoothstep(x: f64, lo: f64, hi: f64) -> f64 {
    let t = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The downsampled world the caravans plan over: what each step costs,
/// and which cells are open water (so embarking can be charged).
pub struct TradeGrid {
    pub cost: Array2<f64>,
    pub sea: Array2<bool>,
    pub f: usize,
}

impl TradeGrid {
    pub fn build(
        height: &Array2<f32>,
        flags: &Array2<u8>,
        biomes: &Array2<u8>,
        discharge: &Array2<f32>,
        f: usize,
    ) -> TradeGrid {
        let (rows, cols) = height.dim();
        let sea_mask = height.mapv(|h| h < 0.0);
        // distance (on water) to the nearest shore — cabotage vs blue water
        let shore_d = ndimage::distance_transform_edt(&sea_mask);
        let hpos = height.mapv(|h| h.max(0.0) as f64);
        let (gy, gx) = ndimage::gradient(&hpos);

        let full = Array2::from_shape_fn((rows, cols), |(y, x)| {
            let h = height[[y, x]] as f64;
            if h < 0.0 {
                return if shore_d[[y, x]] <= 6.0 {
                    COAST_SEA_COST
                } else {
                    OPEN_SEA_COST
                };
            }
            if flags[[y, x]] & CellFlags::LAKE.bits() != 0 {
                return LAKE_COST;
            }
            let slope = gy[[y, x]].hypot(gx[[y, x]]) * rows as f64 / 8.0;
            let mut cost = LAND_BASE + 14.0 * slope.clamp(0.0, 1.2);
            // thin air, scree and snowline above the tree country
            cost += 4.0 * smoothstep(h, 0.42, 0.8);
            let b = biomes[[y, x]];
            cost += 3.0 * ((b == gc::DESERT) as u8 as f64)
                + 9.0 * ((b == gc::ICE) as u8 as f64)
                + 1.5 * ((b == gc::TUNDRA) as u8 as f64)
                + 2.5 * ((b == gc::TROPICAL_RAIN_FOREST) as u8 as f64)
                + 0.8 * ((b == gc::WOODLAND
                    || b == gc::SEASONAL_RAIN_FOREST
                    || b == gc::TEMPERATE_RAIN_FOREST
                    || b == gc::BOREAL_FOREST) as u8 as f64);
            if flags[[y, x]] & CellFlags::RIVER.bits() != 0 {
                if discharge[[y, x]] as f64 > NAVIGABLE {
                    // a broad river carries barges: the cheapest road inland
                    cost = cost.min(RIVER_BARGE_COST);
                } else {
                    cost += 1.8; // wet boots and lost cargo at the ford
                }
            }
            cost
        });

        let cost = downsample(&full, f);
        let s = rows / f;
        let sc = cols / f;
        let sea = Array2::from_shape_fn((s, sc), |(y, x)| {
            let mut n = 0usize;
            for dy in 0..f {
                for dx in 0..f {
                    if sea_mask[[y * f + dy, x * f + dx]] {
                        n += 1;
                    }
                }
            }
            2 * n > f * f
        });
        TradeGrid { cost, sea, f }
    }
}

pub fn downsample(a: &Array2<f64>, f: usize) -> Array2<f64> {
    let (rows, cols) = a.dim();
    let s = rows / f;
    let sc = cols / f;
    Array2::from_shape_fn((s, sc), |(y, x)| {
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

/// A* over the trade grid. Crossing the shoreline pays the harbour fee,
/// so short hops stay ashore and long hauls take ship. Returns the path
/// and its total cost.
pub fn astar(
    grid: &TradeGrid,
    start: (usize, usize),
    goal: (usize, usize),
) -> Option<(Vec<(usize, usize)>, f64)> {
    let cost = &grid.cost;
    let (hh, ww) = cost.dim();
    let (sy, sx) = start;
    let (gy, gx) = goal;
    let min_cost = 0.3;
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
            return Some((path, g));
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
            let mut ng = g + dist * 0.5 * (cost[[y, x]] + cost[[ny, nx]]);
            if grid.sea[[y, x]] != grid.sea[[ny, nx]] {
                ng += EMBARK_COST; // cargo changes from wheel to keel
            }
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

/// Classify one full-resolution point: under sail, on a barge, or afoot.
fn point_mode(
    x: i64,
    y: i64,
    height: &Array2<f32>,
    flags: &Array2<u8>,
    discharge: &Array2<f32>,
) -> u8 {
    let (hh, ww) = height.dim();
    if x < 0 || y < 0 || x >= ww as i64 || y >= hh as i64 {
        return MODE_LAND;
    }
    if height[[y as usize, x as usize]] < 0.0 {
        return MODE_SEA;
    }
    for dy in -1i64..=1 {
        for dx in -1i64..=1 {
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0 || ny < 0 || nx >= ww as i64 || ny >= hh as i64 {
                continue;
            }
            let (nxu, nyu) = (nx as usize, ny as usize);
            if flags[[nyu, nxu]] & CellFlags::RIVER.bits() != 0 && discharge[[nyu, nxu]] as f64 > NAVIGABLE {
                return MODE_RIVER;
            }
        }
    }
    MODE_LAND
}

#[allow(clippy::too_many_arguments)]
pub fn route_entry(
    sa: &Settlement,
    sb: &Settlement,
    path: &[(usize, usize)],
    f: usize,
    total_cost: f64,
    height: &Array2<f32>,
    flags: &Array2<u8>,
    discharge: &Array2<f32>,
    flow_amp: &Array2<f32>,
) -> Route {
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
    let n = pts.len();
    let mut modes: Vec<u8> = pts
        .iter()
        .map(|p| point_mode(p[0], p[1], height, flags, discharge))
        .collect();
    modes[0] = MODE_LAND; // journeys begin and end at the town gate
    modes[n - 1] = MODE_LAND;
    let sea_frac = modes.iter().filter(|&&m| m == MODE_SEA).count() as f64 / n as f64;

    // The barge legs remember their river's seasons: a route that rides
    // monsoon water swells and shrinks with it.
    let (hh, ww) = flow_amp.dim();
    let mut amp_sum = 0.0f64;
    let mut nriv = 0usize;
    for (p, &m) in pts.iter().zip(modes.iter()) {
        if m == MODE_RIVER
            && p[0] >= 0
            && p[1] >= 0
            && (p[0] as usize) < ww
            && (p[1] as usize) < hh
        {
            amp_sum += flow_amp[[p[1] as usize, p[0] as usize]] as f64;
            nriv += 1;
        }
    }
    let ramp = if nriv > 0 {
        crate::util::round2(amp_sum / nriv as f64 * (nriv as f64 / n as f64))
    } else {
        0.0
    };

    let base_w = (0.5 + (((sa.pop + sb.pop) as f64).log10() - 2.0) * 0.6).clamp(0.5, 2.0);
    // the sea multiplies what a route can carry
    let w = crate::util::round2((base_w * (0.8 + 0.55 * sea_frac)).clamp(0.4, 2.4));
    Route {
        a: sa.id,
        b: sb.id,
        path: pts,
        m: modes,
        w,
        cost: crate::util::round2(total_cost),
        sea: crate::util::round2(sea_frac),
        ramp,
        goods: vec![sa.exports.clone(), sb.exports.clone()],
        old: false,
    }
}

pub fn recount_connections(settlements: &mut [Settlement], routes: &[Route]) {
    let mut conn: HashMap<SettlementId, i64> = settlements.iter().map(|s| (s.id, 0)).collect();
    for r in routes {
        *conn.entry(r.a).or_insert(0) += 1;
        *conn.entry(r.b).or_insert(0) += 1;
    }
    for s in settlements.iter_mut() {
        s.connections = *conn.get(&s.id).unwrap_or(&0);
    }
}

/// A harbour is where a settlement's trade takes to the water: coastal,
/// with at least one route that goes under sail close to its gates.
pub fn mark_ports(settlements: &mut [Settlement], routes: &[Route]) {
    let mut ports: BTreeSet<SettlementId> = BTreeSet::new();
    for r in routes {
        if r.sea < 0.05 {
            continue;
        }
        let n = r.m.len();
        let head = r.m.iter().take(4.min(n)).any(|&m| m == MODE_SEA);
        let tail = r.m.iter().rev().take(4.min(n)).any(|&m| m == MODE_SEA);
        if head {
            ports.insert(r.a);
        }
        if tail {
            ports.insert(r.b);
        }
        // a mostly-sea route makes harbours of both ends
        if r.sea > 0.5 {
            ports.insert(r.a);
            ports.insert(r.b);
        }
    }
    for s in settlements.iter_mut() {
        s.port = s.coastal && ports.contains(&s.id);
    }
}

/// Is a journey worth making at all? Impassable country strands towns.
fn viable(total_cost: f64, start: (usize, usize), goal: (usize, usize)) -> bool {
    let d = (goal.0 as f64 - start.0 as f64).hypot(goal.1 as f64 - start.1 as f64);
    total_cost <= 8.0 * d + 40.0
}

/// Candidate pairs: each settlement courts its 2 nearest neighbours, and
/// every coastal town also seeks a far harbour to trade with under sail.
pub fn build_routes(
    grid: &TradeGrid,
    settlements: &mut [Settlement],
    height: &Array2<f32>,
    flags: &Array2<u8>,
    discharge: &Array2<f32>,
    flow_amp: &Array2<f32>,
) -> Vec<Route> {
    let f = grid.f;
    let rows = grid.cost.dim().0 * f;
    let far2 = ((rows as f64) / 5.0).powi(2);
    let mut pairs: BTreeSet<(SettlementId, SettlementId)> = BTreeSet::new();
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
        // the sea link: the nearest fellow harbour town beyond the horizon
        if s.coastal {
            if let Some(o) = others.iter().find(|o| {
                o.coastal
                    && ((o.x - s.x).pow(2) + (o.y - s.y).pow(2)) as f64 > far2
            }) {
                pairs.insert((s.id.min(o.id), s.id.max(o.id)));
            }
        }
    }

    let by_id: HashMap<SettlementId, usize> = settlements
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id, i))
        .collect();
    let mut routes: Vec<Route> = Vec::new();
    for &(a, b) in pairs.iter() {
        let sa = &settlements[by_id[&a]];
        let sb = &settlements[by_id[&b]];
        let start = (sa.y as usize / f, sa.x as usize / f);
        let goal = (sb.y as usize / f, sb.x as usize / f);
        if let Some((path, cost)) = astar(grid, start, goal) {
            if viable(cost, start, goal) {
                routes.push(route_entry(sa, sb, &path, f, cost, height, flags, discharge, flow_amp));
            }
        }
    }
    recount_connections(settlements, &routes);
    rescue_unconnected(settlements, &mut routes, grid, height, flags, discharge, flow_amp);
    bridge_components(settlements, &mut routes, grid, height, flags, discharge, flow_amp);
    routes
}

/// Any town the viability cap left stranded still gets one lifeline: the
/// cheapest path to a near neighbour, taken at whatever price the terrain
/// asks. Frontier mining camps in hungry country stay on the map of trade.
pub fn rescue_unconnected(
    settlements: &mut [Settlement],
    routes: &mut Vec<Route>,
    grid: &TradeGrid,
    height: &Array2<f32>,
    flags: &Array2<u8>,
    discharge: &Array2<f32>,
    flow_amp: &Array2<f32>,
) {
    let f = grid.f;
    let lonely: Vec<usize> = settlements
        .iter()
        .enumerate()
        .filter(|(_, s)| s.connections == 0)
        .map(|(i, _)| i)
        .collect();
    if lonely.is_empty() {
        return;
    }
    for idx in lonely {
        let s = settlements[idx].clone();
        let mut others: Vec<&Settlement> =
            settlements.iter().filter(|o| o.id != s.id).collect();
        others.sort_by(|a, b| {
            let da = (a.x - s.x).pow(2) + (a.y - s.y).pow(2);
            let db = (b.x - s.x).pow(2) + (b.y - s.y).pow(2);
            da.cmp(&db)
        });
        let mut best: Option<(Route, f64)> = None;
        for o in others.iter().take(6) {
            let start = (s.y as usize / f, s.x as usize / f);
            let goal = (o.y as usize / f, o.x as usize / f);
            if let Some((path, cost)) = astar(grid, start, goal) {
                if best.as_ref().map_or(true, |(_, c)| cost < *c) {
                    best = Some((
                        route_entry(&s, o, &path, f, cost, height, flags, discharge, flow_amp),
                        cost,
                    ));
                }
            }
        }
        if let Some((r, _)) = best {
            routes.push(r);
        }
    }
    recount_connections(settlements, routes);
}

/// Link a newly founded settlement (at `idx`) into the route network.
#[allow(clippy::too_many_arguments)]
pub fn connect_settlement(
    idx: usize,
    settlements: &mut [Settlement],
    routes: &mut Vec<Route>,
    grid: &TradeGrid,
    height: &Array2<f32>,
    flags: &Array2<u8>,
    discharge: &Array2<f32>,
    flow_amp: &Array2<f32>,
) {
    let f = grid.f;
    let s = settlements[idx].clone();
    let mut others: Vec<&Settlement> = settlements.iter().filter(|o| o.id != s.id).collect();
    others.sort_by(|a, b| {
        let da = (a.x - s.x).pow(2) + (a.y - s.y).pow(2);
        let db = (b.x - s.x).pow(2) + (b.y - s.y).pow(2);
        da.cmp(&db)
    });
    let mut new_routes: Vec<Route> = Vec::new();
    for o in others.iter().take(2) {
        let start = (s.y as usize / f, s.x as usize / f);
        let goal = (o.y as usize / f, o.x as usize / f);
        if let Some((path, cost)) = astar(grid, start, goal) {
            if viable(cost, start, goal) {
                new_routes.push(route_entry(&s, o, &path, f, cost, height, flags, discharge, flow_amp));
            }
        }
    }
    routes.extend(new_routes);
    recount_connections(settlements, routes);
    rescue_unconnected(settlements, routes, grid, height, flags, discharge, flow_amp);
    bridge_components(settlements, routes, grid, height, flags, discharge, flow_amp);
    mark_ports(settlements, routes);
}

/// One world, one web (M8.1). The loneliness rescue works at the level of
/// a single town's degree — it cannot see an archipelago pair that trades
/// only with itself, a component of two invisible to every degree count.
/// Union the route graph and, while more than one component stands, bridge
/// the smallest one to the largest by the cheapest lifeline crossing,
/// taken at whatever price the terrain asks.
#[allow(clippy::too_many_arguments)]
pub fn bridge_components(
    settlements: &mut [Settlement],
    routes: &mut Vec<Route>,
    grid: &TradeGrid,
    height: &Array2<f32>,
    flags: &Array2<u8>,
    discharge: &Array2<f32>,
    flow_amp: &Array2<f32>,
) {
    let n = settlements.len();
    if n < 2 {
        return;
    }
    let f = grid.f;
    let idx_of: HashMap<SettlementId, usize> = settlements
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id, i))
        .collect();

    fn find(uf: &mut Vec<usize>, mut i: usize) -> usize {
        while uf[i] != i {
            uf[i] = uf[uf[i]];
            i = uf[i];
        }
        i
    }

    let mut bridged = false;
    // Each pass joins two components; n passes always suffice.
    for _ in 0..n {
        let mut uf: Vec<usize> = (0..n).collect();
        for r in routes.iter() {
            if let (Some(&ia), Some(&ib)) = (idx_of.get(&r.a), idx_of.get(&r.b)) {
                let (ra, rb) = (find(&mut uf, ia), find(&mut uf, ib));
                if ra != rb {
                    uf[ra] = rb;
                }
            }
        }
        let mut comps: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for i in 0..n {
            let r = find(&mut uf, i);
            comps.entry(r).or_default().push(i);
        }
        if comps.len() <= 1 {
            break;
        }
        // Largest component is the mainland web; smallest is next to join.
        // BTreeMap order + (len, root) keys keep the choice deterministic.
        let main_root = *comps
            .iter()
            .max_by_key(|(root, m)| (m.len(), usize::MAX - **root))
            .map(|(root, _)| root)
            .unwrap();
        let minor_root = *comps
            .iter()
            .filter(|(root, _)| **root != main_root)
            .min_by_key(|(root, m)| (m.len(), **root))
            .map(|(root, _)| root)
            .unwrap();
        let main = &comps[&main_root];
        let minor = &comps[&minor_root];

        // For each stranded town, its nearest mainland partner; then court
        // the closest few crossings and keep the cheapest path that answers.
        let mut cands: Vec<(i64, usize, usize)> = minor
            .iter()
            .map(|&mi| {
                let s = &settlements[mi];
                let (bj, d2) = main
                    .iter()
                    .map(|&mj| {
                        let o = &settlements[mj];
                        (mj, (o.x - s.x).pow(2) + (o.y - s.y).pow(2))
                    })
                    .min_by_key(|&(mj, d2)| (d2, settlements[mj].id))
                    .unwrap();
                (d2, mi, bj)
            })
            .collect();
        cands.sort_by_key(|&(d2, a, b)| (d2, settlements[a].id, settlements[b].id));
        cands.truncate(6);

        let mut best: Option<(Route, f64)> = None;
        for &(_, mi, mj) in &cands {
            let s = &settlements[mi];
            let o = &settlements[mj];
            let start = (s.y as usize / f, s.x as usize / f);
            let goal = (o.y as usize / f, o.x as usize / f);
            if let Some((path, cost)) = astar(grid, start, goal) {
                if best.as_ref().map_or(true, |(_, c)| cost < *c) {
                    best = Some((
                        route_entry(s, o, &path, f, cost, height, flags, discharge, flow_amp),
                        cost,
                    ));
                }
            }
        }
        match best {
            Some((r, _)) => {
                routes.push(r);
                bridged = true;
            }
            // No crossing answered at all — impassable by every measure;
            // leave the world as it is rather than loop forever.
            None => break,
        }
    }
    if bridged {
        recount_connections(settlements, routes);
    }
}
