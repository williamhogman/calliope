// Static UI vocabulary: lenses, overlays, event taxonomy.

export const LAYERS = [
  ["political", "Political", "Realms and their reach"],
  ["biomes", "Terrain", "The land as seen from orbit"],
  ["elevation", "Elevation", "Height above the sea"],
  ["temperature", "Temperature", "Warmth through the seasons"],
  ["precip", "Rainfall", "Where the clouds break"],
  ["hydro", "Hydrology", "Rivers, lakes and their power"],
  ["fertility", "Fertility", "Where fields will feed a city"],
];

export const OVERLAYS = [
  ["settlements", "Settlements"],
  ["labels", "Place names"],
  ["routes", "Trade routes"],
  ["rivers", "Rivers"],
  ["resources", "Resources"],
  ["snow", "Snow & sea ice"],
  ["hillshade", "Relief shading"],
  ["winds", "Winds"],
];

// Every chronicle entry is tinted by what kind of thing happened, and each
// kind belongs to a family used by filters and notification channels.
export const EVENT_KINDS = {
  myth:      { color: "#c9b458", family: "myth" },
  omen:      { color: "#a8d4b8", family: "myth" },
  festival:  { color: "#f0d090", family: "myth" },
  found:     { color: "#d4a94a", family: "realm" },
  growth:    { color: "#8fb6dd", family: "realm" },
  ruler:     { color: "#c9a0e8", family: "realm" },
  realm:     { color: "#e8c07a", family: "realm" },
  society:   { color: "#e0b0d0", family: "realm" },
  tech:      { color: "#7fc4e8", family: "realm" },
  wonder:    { color: "#ffd766", family: "realm" },
  war:       { color: "#e05555", family: "war" },
  trade:     { color: "#9fd0c8", family: "economy" },
  discovery: { color: "#f2c14e", family: "economy" },
  depletion: { color: "#b09a86", family: "economy" },
  disaster:  { color: "#e07a6a", family: "nature" },
};

export const EVENT_FAMILIES = [
  ["realm", "Realm"],
  ["war", "War"],
  ["economy", "Economy"],
  ["myth", "Myth"],
  ["nature", "Nature"],
];

export const eventColor = (e) => EVENT_KINDS[e.k]?.color || "#8a8fa0";
export const eventFamily = (e) => EVENT_KINDS[e.k]?.family || "realm";

export const STYLE_LABEL = {
  hellenic: "coastal south", nordic: "far north", arid: "desert marches",
  sylvan: "deep woods", steppe: "open plains", old: "old tongue",
};

// M6.5 — the sifter's patterns, named for the reader and tinted for the rail.
export const PATTERN_META = {
  "rise-fall":       { label: "Rise & fall",     color: "#d4a94a" },
  "trials":          { label: "Trials",          color: "#e07a6a" },
  "rivalry":         { label: "Rivalry",         color: "#e05555" },
  "mine-curse":      { label: "Mine-curse",      color: "#b09a86" },
  "tide-turned":     { label: "Tide turned",     color: "#c9a0e8" },
  "founders-dream":  { label: "Founder's dream", color: "#f0d090" },
  "roads-of":        { label: "Merchant's road", color: "#9fd0c8" },
  "restless-crowns": { label: "Restless crowns", color: "#e8c07a" },
  "relic-road":      { label: "Relic's road",    color: "#7fc4e8" },
};
export const patternMeta = (p) => PATTERN_META[p] || { label: p, color: "#8a8fa0" };

// M6.1 — how the cast reads in lists: a tint per entity kind.
export const ENTITY_KINDS = {
  person:     { label: "person",     color: "#c9a0e8" },
  ruler:      { label: "ruler",      color: "#c9a0e8" },
  artifact:   { label: "relic",      color: "#7fc4e8" },
  war:        { label: "war",        color: "#e05555" },
  settlement: { label: "settlement", color: "#d4a94a" },
  culture:    { label: "people",     color: "#e8c07a" },
  feature:    { label: "place",      color: "#8fb6dd" },
  ruin:       { label: "ruin",       color: "#b09a86" },
  world:      { label: "world",      color: "#a8d4b8" },
  good:       { label: "good",       color: "#9fd0c8" },
};
export const entityKind = (k) => ENTITY_KINDS[k] || { label: k, color: "#8a8fa0" };

export const FALLBACK_MONTHS = ["I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI", "XII"];

export const fmt = (n) => Math.round(n).toLocaleString("en-US");

export function dateOf(m, months) {
  return `Year ${Math.floor(m / 12) + 1} \u00b7 ${months[((m % 12) + 12) % 12]}`;
}
