"""Hydrology: depression filling, D8 flow routing, rivers and lakes."""

import heapq

import numpy as np

_N8 = [(-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1)]
_DIST = [1.4142135, 1.0, 1.4142135, 1.0, 1.0, 1.4142135, 1.0, 1.4142135]


def fill_depressions(height: np.ndarray, water: np.ndarray, eps: float = 1e-5):
    """Priority-flood fill so every land cell drains to the ocean or map edge."""
    size = height.shape[0]
    filled = height.astype(np.float64).copy()
    visited = water.copy()

    heap = []
    land = ~water
    # seeds: land on the border, and land adjacent to water
    adj = np.zeros_like(land)
    adj[:-1, :] |= water[1:, :]
    adj[1:, :] |= water[:-1, :]
    adj[:, :-1] |= water[:, 1:]
    adj[:, 1:] |= water[:, :-1]
    seeds = land & adj
    seeds[0, :] |= land[0, :]
    seeds[-1, :] |= land[-1, :]
    seeds[:, 0] |= land[:, 0]
    seeds[:, -1] |= land[:, -1]

    ys, xs = np.where(seeds)
    for y, x in zip(ys.tolist(), xs.tolist()):
        heapq.heappush(heap, (filled[y, x], y, x))
        visited[y, x] = True

    while heap:
        hcur, y, x = heapq.heappop(heap)
        for dy, dx in _N8:
            ny, nx = y + dy, x + dx
            if 0 <= ny < size and 0 <= nx < size and not visited[ny, nx]:
                visited[ny, nx] = True
                nh = filled[ny, nx]
                if nh <= hcur:
                    nh = hcur + eps
                    filled[ny, nx] = nh
                heapq.heappush(heap, (nh, ny, nx))
    return filled


def flow_directions(filled: np.ndarray, water: np.ndarray):
    """D8: index 0..7 into _N8 of the steepest downslope neighbour, -1 = terminal."""
    size = filled.shape[0]
    best_drop = np.full((size, size), 0.0)
    best_dir = np.full((size, size), -1, dtype=np.int8)
    for i, ((dy, dx), dist) in enumerate(zip(_N8, _DIST)):
        shifted = np.full_like(filled, np.inf)
        ys = slice(max(0, -dy), size - max(0, dy))
        xs = slice(max(0, -dx), size - max(0, dx))
        ys_src = slice(max(0, dy), size + min(0, dy))
        xs_src = slice(max(0, dx), size + min(0, dx))
        shifted[ys, xs] = filled[ys_src, xs_src]
        drop = (filled - shifted) / dist
        better = drop > best_drop
        best_drop = np.where(better, drop, best_drop)
        best_dir = np.where(better, np.int8(i), best_dir)
    best_dir[water] = -1
    return best_dir


def flow_accumulation(filled: np.ndarray, dirs: np.ndarray, precip: np.ndarray,
                      water: np.ndarray):
    """Accumulate precip downstream; returns discharge (precip-weighted area)."""
    size = filled.shape[0]
    contrib = np.where(water, 0.0, precip / 1000.0)
    acc = contrib.copy()
    order = np.argsort(filled, axis=None)[::-1]  # high to low
    ys, xs = np.unravel_index(order, filled.shape)
    dirs_flat = dirs
    for y, x in zip(ys.tolist(), xs.tolist()):
        d = dirs_flat[y, x]
        if d >= 0:
            dy, dx = _N8[d]
            ny, nx = y + dy, x + dx
            if 0 <= ny < size and 0 <= nx < size:
                acc[ny, nx] += acc[y, x]
    return acc


def hydrology(height: np.ndarray, water: np.ndarray, precip: np.ndarray):
    filled = fill_depressions(height, water)
    dirs = flow_directions(filled, water)
    discharge = flow_accumulation(filled, dirs, precip, water)

    lakes = (~water) & (filled - height > 0.004)
    river_threshold = 28.0
    rivers = (~water) & (~lakes) & (discharge > river_threshold)
    return {
        "filled": filled,
        "dirs": dirs,
        "discharge": discharge.astype(np.float32),
        "rivers": rivers,
        "lakes": lakes,
    }
