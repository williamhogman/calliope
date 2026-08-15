"""Cultures: the peoples of the world.

Settlements cluster into a handful of cultures by geography. Each culture
takes its naming style from the land its people inhabit — tundra folk
sound northern, desert folk arid — and colours the political map. Colonies
inherit their mother city's culture.
"""

import numpy as np

from game import constants as gc
from game import naming

CULTURE_COLORS = ["#d4a94a", "#6f9ceb", "#c86b6b", "#7fb069", "#a06fd4", "#5bc0be"]

_STYLE_BY_BIOME = {
    gc.TUNDRA: "nordic", gc.BOREAL_FOREST: "nordic", gc.ICE: "nordic",
    gc.DESERT: "arid", gc.SAVANNA: "arid",
    gc.TROPICAL_RAIN_FOREST: "sylvan", gc.TEMPERATE_RAIN_FOREST: "sylvan",
    gc.SEASONAL_RAIN_FOREST: "sylvan", gc.WOODLAND: "sylvan",
    gc.GRASSLAND: "steppe",
}

_DEMONYM = {
    "hellenic": "ians", "nordic": "folk", "arid": "im",
    "sylvan": "kin", "steppe": "aks", "old": "ites",
}

_ALL_STYLES = ["hellenic", "steppe", "nordic", "sylvan", "arid"]


def _kmeans(pts, k, rng):
    """Deterministic k-means with greedy max-min init."""
    n = len(pts)
    centers = [pts[int(rng.integers(n))]]
    while len(centers) < k:
        d = np.min([np.hypot(*(pts - c).T) for c in centers], axis=0)
        centers.append(pts[int(np.argmax(d))])
    centers = np.array(centers, dtype=float)
    lab = np.zeros(n, dtype=int)
    for _ in range(16):
        d = np.array([np.hypot(*(pts - c).T) for c in centers])
        lab = np.argmin(d, axis=0)
        for i in range(k):
            if (lab == i).any():
                centers[i] = pts[lab == i].mean(axis=0)
    return lab


def assign_cultures(world, settlements, seed):
    """Cluster settlements into cultures, rename them in-style."""
    if not settlements:
        return []
    rng = np.random.default_rng(seed + 4242)
    n = len(settlements)
    k = int(np.clip(n // 4, 2, min(6, n)))
    pts = np.array([[s["y"], s["x"]] for s in settlements], dtype=float)
    lab = _kmeans(pts, k, rng)

    biomes = world["biomes"]
    taken = world.setdefault("taken_names", set())
    used_styles = set()
    cultures = []
    for cid in range(k):
        members = [s for s, l in zip(settlements, lab) if l == cid]
        if not members:
            members = [settlements[0]]
        # dominant biome of the homeland decides the tongue
        counts = {}
        for s in members:
            b = int(biomes[s["y"], s["x"]])
            counts[b] = counts.get(b, 0) + 1
        dom = max(counts, key=counts.get)
        style = _STYLE_BY_BIOME.get(dom, "hellenic")
        if style in used_styles:
            style = next((st for st in _ALL_STYLES if st not in used_styles),
                         style)
        used_styles.add(style)
        root = naming.make_word(rng, style, taken)
        cultures.append({
            "id": cid,
            "name": root,
            "people": f"{root}{_DEMONYM.get(style, 'ites')}",
            "style": style,
            "color": CULTURE_COLORS[cid % len(CULTURE_COLORS)],
        })

    # rename settlements in their culture's tongue
    for s, l in zip(settlements, lab):
        s["culture"] = int(l)
        s["name"] = naming.make_word(rng, cultures[int(l)]["style"], taken)

    return cultures
