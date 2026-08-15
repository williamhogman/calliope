// Color helpers and gradient palettes.

export function hexRgb(hex) {
  const h = hex.replace("#", "");
  return [
    parseInt(h.slice(0, 2), 16),
    parseInt(h.slice(2, 4), 16),
    parseInt(h.slice(4, 6), 16),
  ];
}

export function gradient(stops) {
  const s = stops.map(([v, c]) => [v, Array.isArray(c) ? c : hexRgb(c)]);
  return (v) => {
    if (v <= s[0][0]) return s[0][1];
    for (let i = 1; i < s.length; i++) {
      if (v <= s[i][0]) {
        const [v0, c0] = s[i - 1];
        const [v1, c1] = s[i];
        const t = (v - v0) / (v1 - v0);
        return [
          c0[0] + (c1[0] - c0[0]) * t,
          c0[1] + (c1[1] - c0[1]) * t,
          c0[2] + (c1[2] - c0[2]) * t,
        ];
      }
    }
    return s[s.length - 1][1];
  };
}

export function hslRgb(h, s, l) {
  h = ((h % 360) + 360) % 360 / 360;
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
  const p = 2 * l - q;
  const f = (t) => {
    t = ((t % 1) + 1) % 1;
    if (t < 1 / 6) return p + (q - p) * 6 * t;
    if (t < 1 / 2) return q;
    if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6;
    return p;
  };
  return [f(h + 1 / 3) * 255, f(h) * 255, f(h - 1 / 3) * 255];
}

export function settlementColor(id) {
  return hslRgb(40 + id * 137.508, 0.58, 0.6);
}

export function settlementCss(id) {
  const [r, g, b] = settlementColor(id);
  return `rgb(${r | 0}, ${g | 0}, ${b | 0})`;
}

// deterministic per-pixel dither
export function hash2(x, y) {
  const v = Math.sin(x * 127.1 + y * 311.7) * 43758.5453;
  return v - Math.floor(v);
}

export const TEMP_GRAD = gradient([
  [-35, "#23306e"], [-20, "#3b5cc4"], [-8, "#6fa3e0"], [0, "#dde9ee"],
  [8, "#f2d580"], [18, "#ee9a3c"], [28, "#d84f2a"], [35, "#a31d1d"],
]);

export const PRECIP_GRAD = gradient([
  [0, "#d9c58e"], [200, "#cfd08a"], [500, "#9ec97f"], [900, "#5fae78"],
  [1500, "#3a8f8f"], [2200, "#2f6bb0"], [3000, "#274db0"],
]);

export const ELEV_LAND_GRAD = gradient([
  [0.0, "#4d7c44"], [0.15, "#7d9a52"], [0.3, "#b0a86b"], [0.45, "#8c7a5b"],
  [0.62, "#98918a"], [0.8, "#c9c9c9"], [1.0, "#f4f4f4"],
]);

export const SEA_GRAD = gradient([
  [0, "#3a77b8"], [0.25, "#2b5c96"], [0.6, "#1c3f6e"], [1, "#122a4d"],
]);

export const HYDRO_GRAD = gradient([
  [0, "#16283e"], [0.35, "#2b6cb0"], [0.75, "#57a8e0"], [1, "#a5ddff"],
]);

export const FERT_GRAD = gradient([
  [0, "#3a3630"], [0.15, "#6b5f42"], [0.35, "#8a7c3e"], [0.55, "#7d9a3c"],
  [0.75, "#4d9c3f"], [1, "#1f8f4d"],
]);
