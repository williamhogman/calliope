"""Trade: goods, routes (A* over a terrain-cost grid), and connections.

Each settlement works the deposits inside its hinterland into a goods
list; its best good becomes its export. Routes link each settlement to
its nearest neighbours, remember what flows each way, and carry a
traffic weight so busy roads draw heavier. Connected settlements grow
slightly faster. Paths are found on a quarter-resolution grid for speed.
"""

import heapq

import numpy as np

from game import constants as gc

_N8 = [(-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1)]
_DIST = [1.4142135, 1.0, 1.4142135, 1.0, 1.0, 1.4142135, 1.0, 1.4142135]

_RARITY_W = {"common": 1.0, "uncommon": 1.6, "rare": 2.4, "legendary": 4.0}


# --- goods ---------------------------------------------------------------

def goods_for(s, world):
    """Work out what a settlement produces from its hinterland."""
    from game.resources import abundance
    from game.settlements import territory_radius

    r = territory_radius(s["pop"]) * 1.8
    r2 = r * r
    near = [d for d in world["deposits"]
            if (d["x"] - s["x"]) ** 2 + (d["y"] - s["y"]) ** 2 <= r2]
    near.sort(key=lambda d: d["rich"] * _RARITY_W[abundance(d["r"])],
              reverse=True)
    goods, seen = [], set()
    for d in near:
        if d["r"] not in seen:
            seen.add(d["r"])
            goods.append(d["r"])
    fert = world["fertility"][s["y"], s["x"]]
    if fert > 0.45 and "grain" not in seen:
        goods.insert(0 if fert > 0.7 else min(1, len(goods)), "grain")
    if not goods:
        goods = ["fish"] if s.get("coastal") else ["grain"]
    s["goods"] = goods[:6]
    s["exports"] = s["goods"][0]


def assign_goods(world, settlements):
    for s in settlements:
        goods_for(s, world)


# --- routing -------------------------------------------------------------

def _cost_grid(world):
    h = world["height"]
    size = h.shape[0]
    sea = h < 0
    gy, gx = np.gradient(np.maximum(h, 0.0))
    slope = np.hypot(gx, gy) * size / 8.0
    cost = 1.0 + 6.0 * np.clip(slope, 0.0, 1.2)
    cost = cost + np.where(world["rivers"], 0.8, 0.0)   # fording
    cost = cost + np.where(world["lakes"], 4.0, 0.0)
    b = world["biomes"]
    cost = cost + 1.0 * (b == gc.DESERT) + 2.5 * (b == gc.ICE) + 0.6 * (b == gc.TUNDRA)
    cost = np.where(sea, 0.8, cost)                     # sea lanes are cheap
    return cost


def _downsample(a, f):
    size = a.shape[0]
    s = size // f
    return a[:s * f, :s * f].reshape(s, f, s, f).mean(axis=(1, 3))


def _astar(cost, start, goal, max_expand=200000):
    size = cost.shape[0]
    sy, sx = start
    gy, gx = goal
    min_cost = 0.75
    best = np.full((size, size), np.inf, dtype=np.float64)
    best[sy, sx] = 0.0
    came = {}
    h0 = min_cost * np.hypot(gy - sy, gx - sx)
    heap = [(h0, 0.0, sy, sx)]
    expanded = 0
    while heap:
        _, g, y, x = heapq.heappop(heap)
        if g > best[y, x]:
            continue
        if y == gy and x == gx:
            path = [(y, x)]
            while (y, x) in came:
                y, x = came[(y, x)]
                path.append((y, x))
            return path[::-1]
        expanded += 1
        if expanded > max_expand:
            return None
        for (dy, dx), dist in zip(_N8, _DIST):
            ny, nx = y + dy, x + dx
            if not (0 <= ny < size and 0 <= nx < size):
                continue
            ng = g + dist * 0.5 * (cost[y, x] + cost[ny, nx])
            if ng < best[ny, nx]:
                best[ny, nx] = ng
                came[(ny, nx)] = (y, x)
                f = ng + min_cost * np.hypot(gy - ny, gx - nx)
                heapq.heappush(heap, (f, ng, ny, nx))
    return None


def _route_entry(sa, sb, path, f):
    pts = [[int(x * f + f // 2), int(y * f + f // 2)] for y, x in path]
    pts[0] = [sa["x"], sa["y"]]
    pts[-1] = [sb["x"], sb["y"]]
    if len(pts) > 3:
        pts = [pts[0]] + pts[1:-1:2] + [pts[-1]]
    w = round(float(min(2.0, max(0.5, 0.5 + (np.log10(sa["pop"] + sb["pop"]) - 2.0) * 0.6))), 2)
    return {
        "a": sa["id"], "b": sb["id"], "path": pts, "w": w,
        "goods": [sa.get("exports"), sb.get("exports")],
    }


def _recount_connections(settlements, routes):
    conn = {s["id"]: 0 for s in settlements}
    for r in routes:
        conn[r["a"]] = conn.get(r["a"], 0) + 1
        conn[r["b"]] = conn.get(r["b"], 0) + 1
    for s in settlements:
        s["connections"] = conn.get(s["id"], 0)


def build_routes(world, settlements):
    size = world["height"].shape[0]
    f = max(1, size // 128)
    cost = _downsample(_cost_grid(world), f)
    world["_trade_cost"] = (cost, f)

    # candidate pairs: each settlement to its 2 nearest neighbours
    pairs = set()
    for s in settlements:
        others = sorted(
            (o for o in settlements if o["id"] != s["id"]),
            key=lambda o: (o["x"] - s["x"]) ** 2 + (o["y"] - s["y"]) ** 2)
        for o in others[:2]:
            pairs.add((min(s["id"], o["id"]), max(s["id"], o["id"])))

    by_id = {s["id"]: s for s in settlements}
    routes = []
    for a, b in sorted(pairs):
        sa, sb = by_id[a], by_id[b]
        path = _astar(cost, (sa["y"] // f, sa["x"] // f), (sb["y"] // f, sb["x"] // f))
        if path is None:
            continue
        routes.append(_route_entry(sa, sb, path, f))

    _recount_connections(settlements, routes)
    return routes


def connect_settlement(world, s):
    """Link a newly founded settlement into the route network."""
    settlements = world["settlements"]
    routes = world["routes"]
    cost, f = world.get("_trade_cost", (None, None))
    if cost is None:
        size = world["height"].shape[0]
        f = max(1, size // 128)
        cost = _downsample(_cost_grid(world), f)
        world["_trade_cost"] = (cost, f)

    others = sorted(
        (o for o in settlements if o["id"] != s["id"]),
        key=lambda o: (o["x"] - s["x"]) ** 2 + (o["y"] - s["y"]) ** 2)[:2]
    for o in others:
        path = _astar(cost, (s["y"] // f, s["x"] // f), (o["y"] // f, o["x"] // f))
        if path is not None:
            routes.append(_route_entry(s, o, path, f))
    _recount_connections(settlements, routes)
