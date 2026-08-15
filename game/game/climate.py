"""Climate: temperature (annual mean + seasonal swing) and precipitation.

Temperature keeps the original model's intent (geo.hy): latitude gradient
plus altitude lapse (~10 C/1000 m was the comment; we use the standard
6.5 C/km — 26 C per height unit of 4000 m). The original code applied the
lapse with np.minimum(0, h), which *warmed the oceans* instead of cooling
the mountains; that is fixed here to match its own comment.

Precipitation is new: prevailing winds by latitude band (trades /
westerlies / polar easterlies) advect moisture picked up over water,
raining it out on uplift — producing rain shadows and coastal wet belts —
with an ITCZ boost and subtropical (Hadley-subsidence) suppression.
"""

import numpy as np
from scipy import ndimage


def latitude_deg(size: int) -> np.ndarray:
    """Degrees from equator; equator at the middle row (as in geo.hy)."""
    rows = np.abs(np.linspace(-90.0, 90.0, size))
    return np.tile(rows[:, None], (1, size))


def temperature_mean(height: np.ndarray, lat_deg: np.ndarray) -> np.ndarray:
    """Annual-mean sea-level temperature by latitude, minus altitude lapse."""
    lat = lat_deg / 90.0
    t_sea = 28.0 - 56.0 * lat ** 1.6
    lapse = 26.0 * np.maximum(height, 0.0)  # 6.5 C/km * 4 km per unit
    return t_sea - lapse


def continentality(water: np.ndarray) -> np.ndarray:
    """0.35 (maritime) .. 1.0 (deep continental interior)."""
    d = ndimage.distance_transform_edt(~water)
    return 0.35 + 0.65 * np.clip(d / 70.0, 0.0, 1.0)


def temperature_amplitude(lat_deg: np.ndarray, water: np.ndarray) -> np.ndarray:
    """Signed seasonal swing.

    T(month) = mean + amp_signed * cos(2*pi*month/12), month 0 = Gamelion (Jan).
    Southern hemisphere positive (warm in Gamelion), northern negative.
    """
    lat = lat_deg / 90.0
    amp = (3.0 + 19.0 * lat ** 1.2) * continentality(water)
    size = lat_deg.shape[0]
    south = np.zeros_like(amp)
    south[size // 2:, :] = 1.0
    return np.where(south > 0, amp, -amp)


def month_temperature(tmean, tamp_signed, month: int):
    return tmean + tamp_signed * np.cos(2.0 * np.pi * month / 12.0)


def precipitation(height: np.ndarray, water: np.ndarray, tmean: np.ndarray,
                  lat_deg: np.ndarray) -> np.ndarray:
    """Wind-advected moisture -> annual precipitation in mm/yr."""
    size = height.shape[0]
    h = np.maximum(height, 0.0)
    lat_row = lat_deg[:, 0]
    # trade winds (<30) blow E->W: dx=-1; westerlies (30-60): dx=+1; polar: dx=-1
    dx_row = np.where(lat_row < 30, -1, np.where(lat_row < 60, 1, -1))

    p = np.zeros((size, size))
    cap = np.clip(1.0 + tmean / 22.0, 0.15, 2.3)  # warm air holds more

    for d in (1, -1):
        rows = np.where(dx_row == d)[0]
        if rows.size == 0:
            continue
        w = np.full(rows.size, 0.4)
        wraps = 3
        for step in range(wraps * size):
            xcur = (d * step) % size
            xprev = (xcur - d) % size
            wat = water[rows, xcur]
            t = tmean[rows, xcur]
            evap = np.where(wat, 0.018 + 0.030 * np.clip(t, 0, 30) / 30.0, 0.0035)
            w = w + evap
            uplift = np.clip((h[rows, xcur] - h[rows, xprev]) * size / 40.0, 0.0, 3.0)
            rate = np.where(wat, 0.012, np.clip(0.030 + 0.40 * uplift, 0.0, 0.65))
            rain = w * rate
            over = np.maximum(w - cap[rows, xcur], 0.0)
            rain = rain + 0.5 * over
            w = w - rain
            if step >= (wraps - 1) * size:  # record only the settled final wrap
                p[rows, xcur] += rain

    # ITCZ convective boost, subtropical subsidence suppression
    p *= 1.0 + 0.9 * np.exp(-((lat_deg / 10.0) ** 2))
    p *= 1.0 - 0.38 * np.exp(-(((lat_deg - 25.0) / 8.0) ** 2))
    # cold air rains little
    p *= np.clip(0.25 + (tmean + 20.0) / 40.0, 0.25, 1.0)

    p = ndimage.gaussian_filter(p, sigma=1.4)

    # normalise to mm/yr: land mean ~900 mm
    land = ~water
    mean_land = float(p[land].mean()) if land.any() else 1.0
    p = p * (900.0 / max(mean_land, 1e-9))
    return np.clip(p, 0.0, 4500.0)
