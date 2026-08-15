"""Soil fertility: where fields flourish, and why settlements sit there.

Fertility combines a warmth optimum, a rainfall optimum, a slope penalty,
alluvial silt along the big rivers and a lakeshore bonus. It feeds the
settlement food score (so river-valley cities outgrow hill camps) and
ships to the client as its own map layer.
"""

import numpy as np
from scipy import ndimage


def fertility(height, tmean, precip, rivers, lakes, discharge):
    land = height >= 0
    size = height.shape[0]

    t = np.exp(-(((tmean - 17.0) / 11.0) ** 2))
    p = np.interp(precip,
                  [0, 150, 450, 900, 1600, 2600, 4000],
                  [0.0, 0.08, 0.55, 1.0, 0.9, 0.5, 0.3])

    gy, gx = np.gradient(np.maximum(height, 0.0))
    slope = np.hypot(gx, gy) * size / 8.0
    sp = 1.0 / (1.0 + (slope * 2.2) ** 2)

    fert = 0.9 * t * p * sp

    # alluvial floodplains: big rivers lay down silt as they wander
    silt = ndimage.gaussian_filter(
        rivers.astype(np.float64) * np.log1p(discharge), sigma=2.2)
    fert = fert + np.clip(silt * 0.08, 0.0, 0.35) * t

    # lakeshores hold moisture
    shore = ndimage.binary_dilation(lakes, iterations=2) & ~lakes
    fert = fert + 0.08 * shore

    fert[~land] = 0.0
    fert[lakes] = 0.0
    return np.clip(fert, 0.0, 1.0).astype(np.float32)
