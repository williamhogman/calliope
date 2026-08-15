"""Biome classification.

The biome table is geo.hy's table verbatim (same rows, same 6x6 expansion
via dupe-elements). The original indexed rows by bucketed *height*, but the
row contents — ice, tundra, boreal belt, temperate belt, two tropical
rows — are unmistakably a Whittaker diagram, i.e. temperature bands.
We index rows by temperature band and columns by moisture band, which is
what the table always meant.
"""

import numpy as np

from game import constants as gc


def _dupe(row, rep):
    out = []
    for x in row:
        out.extend([x] * rep)
    return out


def _conv(row):
    n = len(row)
    if n == 6:
        return list(row)
    if n == 3:
        return _dupe(row, 2)
    if n == 2:
        return _dupe(row, 3)
    if n == 1:
        return _dupe(row, 6)
    raise ValueError(f"bad biome row length {n}")


# rows: coldest -> hottest (from geo.hy)
BIOME_TABLE = np.array([
    _conv([gc.ICE]),
    _conv([gc.TUNDRA]),
    _conv([gc.GRASSLAND, gc.GRASSLAND, gc.WOODLAND,
           gc.BOREAL_FOREST, gc.BOREAL_FOREST, gc.BOREAL_FOREST]),
    _conv([gc.DESERT, gc.DESERT, gc.WOODLAND, gc.WOODLAND,
           gc.SEASONAL_RAIN_FOREST, gc.TEMPERATE_RAIN_FOREST]),
    _conv([gc.DESERT, gc.SAVANNA, gc.TROPICAL_RAIN_FOREST]),
    _conv([gc.DESERT, gc.SAVANNA, gc.TROPICAL_RAIN_FOREST]),
], dtype=np.uint8)

TEMP_EDGES = np.array([-10.0, -2.0, 5.0, 13.0, 20.0])       # C, 6 bands
PRECIP_EDGES = np.array([180.0, 420.0, 800.0, 1400.0, 2200.0])  # mm/yr, 6 bands


def classify(height: np.ndarray, tmean: np.ndarray, precip: np.ndarray,
             lakes: np.ndarray) -> np.ndarray:
    water = height < 0.0
    trow = np.digitize(tmean, TEMP_EDGES)
    pcol = np.digitize(precip, PRECIP_EDGES)
    biomes = BIOME_TABLE[trow, pcol]
    biomes = np.where(water | lakes, np.uint8(gc.WATER), biomes)
    return biomes.astype(np.uint8)
