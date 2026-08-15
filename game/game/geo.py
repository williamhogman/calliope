"""Terrain generation — ported from geo.hy and extended.

Height unit: 1.0 == 4000 m, sea level at 0.0 (as in the original).
The base formula is the original's: height = (radial + fbm) / 2,
with added domain warp (organic coastlines) and ridged noise (mountain
ranges that produce rain shadows and ore-bearing highlands).
"""

import numpy as np

from game.noisegen import Perlin3

SIZE = 512


def radial(size: int) -> np.ndarray:
    """Two continental bulges — identical to geo.hy."""
    xc = np.linspace(-np.pi, 3 * np.pi, size)
    yc = np.linspace(0, np.pi, size)
    x, y = np.meshgrid(xc, yc)
    return np.cos(x) * np.sin(y)


def _smoothstep(x, lo, hi):
    t = np.clip((x - lo) / (hi - lo), 0.0, 1.0)
    return t * t * (3 - 2 * t)


def heightmap(seed: int, size: int = SIZE) -> np.ndarray:
    base = Perlin3(seed)
    warp = Perlin3(seed + 101)
    ridge = Perlin3(seed + 202)

    yy, xx = np.mgrid[0:size, 0:size].astype(np.float64)
    fx = xx / size * 5.0  # original frequency: (x / SIZE) * 5
    fy = yy / size * 5.0

    # Domain warp for organic coastlines
    wx = warp.fbm(fx + 13.7, fy + 7.1, np.full_like(fx, 0.5), octaves=3)
    wy = warp.fbm(fx + 3.3, fy + 11.9, np.full_like(fx, 1.5), octaves=3)
    n = base.fbm(fx + 0.35 * wx, fy + 0.35 * wy, np.full_like(fx, 0.0), octaves=6)

    height = (radial(size) + n * 1.15) / 2.0

    # Mountain ranges: ridged noise, applied inland only so coasts stay clean
    r = ridge.ridged(fx * 1.6 + 31.0, fy * 1.6 + 17.0, np.full_like(fx, 3.3), octaves=5)
    inland = _smoothstep(height, 0.05, 0.32)
    height = height + 0.55 * np.maximum(0.0, r - 0.62) * inland

    return np.clip(height, -1.0, 1.0)


def hillshade_basis(height: np.ndarray):
    """Gradients for client-side hillshading (kept server-side for tests)."""
    gy, gx = np.gradient(height)
    return gx, gy
