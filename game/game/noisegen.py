"""Seeded, vectorised 3D gradient noise + fBm.

Replaces the C `noise` library (snoise3) the .hy original depended on,
with full seed control (the original was permanently locked to one world).
"""

import numpy as np


class Perlin3:
    def __init__(self, seed: int = 0):
        rng = np.random.default_rng(seed)
        p = rng.permutation(256).astype(np.int64)
        self.perm = np.concatenate([p, p])

    @staticmethod
    def _fade(t):
        return t * t * t * (t * (t * 6 - 15) + 10)

    @staticmethod
    def _lerp(a, b, t):
        return a + t * (b - a)

    @staticmethod
    def _grad(h, x, y, z):
        h = h & 15
        u = np.where(h < 8, x, y)
        v = np.where(h < 4, y, np.where((h == 12) | (h == 14), x, z))
        return np.where((h & 1) == 0, u, -u) + np.where((h & 2) == 0, v, -v)

    def noise(self, x, y, z):
        x = np.asarray(x, dtype=np.float64)
        y = np.asarray(y, dtype=np.float64)
        z = np.asarray(z, dtype=np.float64)
        xf0 = np.floor(x); yf0 = np.floor(y); zf0 = np.floor(z)
        xi = xf0.astype(np.int64) & 255
        yi = yf0.astype(np.int64) & 255
        zi = zf0.astype(np.int64) & 255
        xf = x - xf0; yf = y - yf0; zf = z - zf0
        u = self._fade(xf); v = self._fade(yf); w = self._fade(zf)
        p = self.perm
        a = p[xi] + yi
        aa = p[a] + zi
        ab = p[a + 1] + zi
        b = p[xi + 1] + yi
        ba = p[b] + zi
        bb = p[b + 1] + zi
        n000 = self._grad(p[aa], xf, yf, zf)
        n100 = self._grad(p[ba], xf - 1, yf, zf)
        n010 = self._grad(p[ab], xf, yf - 1, zf)
        n110 = self._grad(p[bb], xf - 1, yf - 1, zf)
        n001 = self._grad(p[aa + 1], xf, yf, zf - 1)
        n101 = self._grad(p[ba + 1], xf - 1, yf, zf - 1)
        n011 = self._grad(p[ab + 1], xf, yf - 1, zf - 1)
        n111 = self._grad(p[bb + 1], xf - 1, yf - 1, zf - 1)
        x00 = self._lerp(n000, n100, u)
        x10 = self._lerp(n010, n110, u)
        x01 = self._lerp(n001, n101, u)
        x11 = self._lerp(n011, n111, u)
        y0 = self._lerp(x00, x10, v)
        y1 = self._lerp(x01, x11, v)
        return self._lerp(y0, y1, w)

    def fbm(self, x, y, z, octaves=4, lacunarity=2.0, gain=0.5):
        x = np.asarray(x, dtype=np.float64)
        total = np.zeros_like(x)
        amp = 1.0
        freq = 1.0
        norm = 0.0
        for _ in range(octaves):
            total += amp * self.noise(x * freq, np.asarray(y) * freq, np.asarray(z) * freq)
            norm += amp
            amp *= gain
            freq *= lacunarity
        return total / norm

    def ridged(self, x, y, z, octaves=4, lacunarity=2.0, gain=0.5):
        """Ridged multifractal — 1-|fbm| per octave; makes mountain ranges."""
        x = np.asarray(x, dtype=np.float64)
        total = np.zeros_like(x)
        amp = 1.0
        freq = 1.0
        norm = 0.0
        for _ in range(octaves):
            total += amp * (1.0 - np.abs(self.noise(x * freq, np.asarray(y) * freq, np.asarray(z) * freq)))
            norm += amp
            amp *= gain
            freq *= lacunarity
        return total / norm
