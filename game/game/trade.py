"""Trade routes: A* over a terrain-cost grid, sea lanes included.

Each settlement links to its nearest neighbours; connected settlements
grow slightly faster. Paths are found on a quarter-resolution grid for
speed and upscaled for display.
"""

import heapq

import numpy as np

from game import constants as gc

_N8 = [(-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1)]
_DIST = [1.4142135, 1.0, 1.4142135, 1.0, 1.0, 1.4142135, 1.0, 1.4142135]


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


def build_routes(world, settlements):
    size = world["height"].shape[0]
    f = max(1, size // 128)
    cost = _downsample(_cost_grid(world), f)

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
        start = (sa["y"] // f, sa["x"] // f)
        goal = (sb["y"] // f, sb["x"] // f)
        path = _astar(cost, start, goal)
        if path is None:
            continue
        # upscale, thin every other point to keep payloads light
        pts = [[int(x * f + f // 2), int(y * f + f // 2)] for y, x in path]
        pts[0] = [sa["x"], sa["y"]]
        pts[-1] = [sb["x"], sb["y"]]
        if len(pts) > 3:
            pts = [pts[0]] + pts[1:-1:2] + [pts[-1]]
        routes.append({"a": a, "b": b, "path": pts})

    conn = {s["id"]: 0 for s in settlements}
    for r in routes:
        conn[r["a"]] += 1
        conn[r["b"]] += 1
    for s in settlements:
        s["connections"] = conn[s["id"]]
    return routes
