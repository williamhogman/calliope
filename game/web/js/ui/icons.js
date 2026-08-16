// Inline SVG icon set — one consistent 24px stroke family for the HUD.

import html from "solid-js/html";

const svg = (body, { fill = "none", sw = 1.7, size = 15 } = {}) => () => html`
  <svg viewBox="0 0 24 24" width=${size} height=${size} fill=${fill}
    stroke=${fill === "none" ? "currentColor" : "none"} stroke-width=${sw}
    stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" innerHTML=${body}></svg>`;

export const I = {
  play: svg(`<path d="M7 5.5v13a1 1 0 0 0 1.5.87l11-6.5a1 1 0 0 0 0-1.74l-11-6.5A1 1 0 0 0 7 5.5z"/>`, { fill: "currentColor", size: 17 }),
  pause: svg(`<rect x="6" y="5" width="4" height="14" rx="1"/><rect x="14" y="5" width="4" height="14" rx="1"/>`, { fill: "currentColor", size: 17 }),
  step: svg(`<path d="M5 5.5v13a1 1 0 0 0 1.5.87l9-6.5a1 1 0 0 0 0-1.74l-9-6.5A1 1 0 0 0 5 5.5z"/><rect x="17" y="5" width="3" height="14" rx="1"/>`, { fill: "currentColor" }),
  search: svg(`<circle cx="11" cy="11" r="7"/><path d="M21 21l-4.5-4.5"/>`),
  dice: svg(`<rect x="3" y="3" width="18" height="18" rx="4"/><circle cx="8.5" cy="8.5" r="1.4" fill="currentColor" stroke="none"/><circle cx="15.5" cy="15.5" r="1.4" fill="currentColor" stroke="none"/><circle cx="15.5" cy="8.5" r="1.4" fill="currentColor" stroke="none"/><circle cx="8.5" cy="15.5" r="1.4" fill="currentColor" stroke="none"/>`),
  globe: svg(`<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c2.7 2.7 4.1 5.7 4.1 9s-1.4 6.3-4.1 9c-2.7-2.7-4.1-5.7-4.1-9S9.3 5.7 12 3z"/>`),
  layers: svg(`<path d="M12 3l9 5-9 5-9-5 9-5z"/><path d="M3 13l9 5 9-5"/>`),
  legend: svg(`<rect x="4" y="5" width="5" height="5" rx="1.2"/><path d="M12 7.5h8"/><rect x="4" y="14" width="5" height="5" rx="1.2"/><path d="M12 16.5h8"/>`),
  bell: svg(`<path d="M6 9.5a6 6 0 0 1 12 0c0 5 2 6 2 6H4s2-1 2-6"/><path d="M10 19a2.2 2.2 0 0 0 4 0"/>`),
  close: svg(`<path d="M6 6l12 12M18 6L6 18"/>`, { sw: 1.9 }),
  chevL: svg(`<path d="M14 6l-6 6 6 6"/>`, { sw: 2 }),
  chevR: svg(`<path d="M10 6l6 6-6 6"/>`, { sw: 2 }),
  pin: svg(`<path d="M12 3l2.2 4.9 5.3.6-4 3.7 1.1 5.3L12 14.9l-4.6 2.6 1.1-5.3-4-3.7 5.3-.6L12 3z"/>`, { sw: 1.5 }),
  pinOn: svg(`<path d="M12 3l2.2 4.9 5.3.6-4 3.7 1.1 5.3L12 14.9l-4.6 2.6 1.1-5.3-4-3.7 5.3-.6L12 3z"/>`, { fill: "currentColor" }),
  fly: svg(`<circle cx="12" cy="12" r="3"/><path d="M12 2v4M12 18v4M2 12h4M18 12h4"/>`),
  war: svg(`<path d="M4 20l6.5-6.5M20 4l-8 8M14.5 4H20v5.5M6 15l3 3-2.5 2.5L4 18z"/>`),
  book: svg(`<path d="M4 5.5A2.5 2.5 0 0 1 6.5 3H20v15.5H6.5A2.5 2.5 0 0 0 4 21z"/><path d="M4 18.5A2.5 2.5 0 0 1 6.5 16H20"/>`),
  people: svg(`<circle cx="9" cy="8" r="3.4"/><path d="M2.8 19.4a6.2 6.2 0 0 1 12.4 0"/><path d="M16 5.4a3.4 3.4 0 0 1 0 5.9M17.8 13.6a6.2 6.2 0 0 1 3.4 5.8"/>`),
  market: svg(`<path d="M4 20V10m16 10V10M2.5 10L5 4h14l2.5 6M2.5 10h19M8 20v-6h8v6"/>`),
  place: svg(`<path d="M12 21s-7-6.1-7-11a7 7 0 0 1 14 0c0 4.9-7 11-7 11z"/><circle cx="12" cy="10" r="2.6"/>`),
  why: svg(`<circle cx="12" cy="12" r="9"/><path d="M9.4 9.2a2.7 2.7 0 0 1 5.3.8c0 1.8-2.7 2.2-2.7 4"/><circle cx="12" cy="17.3" r="0.6" fill="currentColor" stroke="none"/>`),
  gem: svg(`<path d="M7 3h10l4 6-9 12L3 9l4-6z"/><path d="M3 9h18M12 21L8.5 9l2-6M12 21L15.5 9l-2-6"/>`, { sw: 1.3 }),
  ship: svg(`<path d="M4 17c1 1.6 2.5 1.6 3.5 0 1 1.6 2.5 1.6 3.5 0 1 1.6 2.5 1.6 3.5 0 1 1.6 2.5 1.6 3.5 0"/><path d="M5.5 14l1-6h4M12 14l6-5-2.5 5"/><path d="M10.5 5v9"/>`, { sw: 1.5 }),
  quill: svg(`<path d="M20 4c-5.5.3-10.5 3.5-12.5 8L4 20l8-3.5C16.5 14.5 19.7 9.5 20 4z"/><path d="M6.5 17.5L16 8"/>`, { sw: 1.5 }),
};
