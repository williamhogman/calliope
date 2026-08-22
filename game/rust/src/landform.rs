//! Landform — the last stage of the fixed pipeline (rock → ice →
//! water → soil → **landform**, ADR-0026): it names what the earlier
//! stages made, and writes nothing back.
//!
//! M26 — drowned and raised coasts: the sea-level history (M25) leaves
//! a legible vocabulary of coastal landforms.
//!
//! The freeze-time offset `dz(y) = isostasy(y) − eustatic` moved the
//! land against the waterline; this module reads the *signature* of
//! that move back out of the pair (final height, pre-offset height):
//!
//! - **Raised beach** — land that stood below the old waterline and
//!   was carried above it (`h0 < 0 ≤ h`): the emerged shelf strip,
//!   densest where the land rose most (the rebound belt — Scandinavia's
//!   strandlines).
//! - **Ria** — sea that was land before the waterline rose over it
//!   (`h0 ≥ 0 > h`) under steep walls: a drowned valley, a firth.
//! - **Skerry** — the same drowned ground in low relief: a scatter of
//!   flooded flats and islets, a skerry field.
//!
//! The grid is pure derived state — a function of the height field and
//! the frozen `SeaLevel` — recomputed identically every generation and
//! folded into `hash_state` to hold the classifier still (the M26
//! gate). Naming reads it to mint firths, skerry fields and strands;
//! the label layer draws them like any other coastal detail.

use ndarray::Array2;

use crate::sealevel::SeaLevel;
use crate::util::fnv1a64;

pub const NONE: u8 = 0;
pub const RAISED: u8 = 1;
pub const RIA: u8 = 2;
pub const SKERRY: u8 = 3;
/// M29 — a drowned glacial trough: the ice overdeepened the valley
/// below the waterline and the sea took it.
pub const FJORD: u8 = 4;
/// M30 — the depositional legacy: ridge of the former ice margin,
/// flow-combed swarm hill, subglacial meltwater ridge.
pub const MORAINE: u8 = 5;
pub const DRUMLIN: u8 = 6;
pub const ESKER: u8 = 7;
/// M31 — an outburst spillway: the oversized abandoned valley a
/// proglacial lake cut below its moraine sill.
pub const SPILLWAY: u8 = 8;
/// M32 — a braided outwash corridor: the flat gravel plain the
/// meltwater planed below the former ice margin.
pub const OUTWASH: u8 = 9;
/// M33 — patterned ground: frost-sorted polygon nets and solifluction
/// stripes where real permafrost meets the surface.
pub const PATTERNED: u8 = 10;
/// M43 — an intertidal flat: ground the tide uncovers and re-covers
/// daily, where a real range meets a low-slope shore.
pub const TIDEFLAT: u8 = 11;
/// M43 — an estuary mouth: a river meeting tidal water.
pub const ESTUARY: u8 = 12;
/// M59/M60 — a delta plain: fan-built land the river laid down where
/// its transport capacity died at the sea.
pub const DELTA: u8 = 13;
/// M44/M60 — the drift-built coast: the hook rooted on the old shore,
/// the offshore bar that grew to stand alone, and the quiet water the
/// new ground pinched off.
pub const SPIT: u8 = 14;
pub const BARRIER: u8 = 15;
pub const LAGOON: u8 = 16;
/// M55/M60 — the dry country's water: arid ground standing over a
/// table within root reach, and the line where the table daylights at
/// a break in slope.
pub const OASIS: u8 = 17;
pub const SPRING: u8 = 18;
/// M29/M60 — the U-valley the ice cut but the sea never took: carved
/// ground that still stands above the waterline.
pub const TROUGH: u8 = 19;
/// Reserved for Karst Country II (Ready queue): no karst pass exists
/// yet, so no cell may carry this tag until limestone country actually
/// behaves like limestone country. Reserving the code point now keeps
/// the wire contract stable when it lands.
pub const KARST: u8 = 20;
/// M60 — the generic relief vocabulary: every land cell the era's
/// stories left untold resolves to a Hammond-style relief class read
/// off the 5×5 (20 km) local relief window, so no ground is nameless.
pub const MOUNTAIN: u8 = 21;
pub const HILLS: u8 = 22;
pub const PLATEAU: u8 = 23;
pub const VALLEY: u8 = 24;
pub const PLAIN: u8 = 25;
/// M60 — open shore water: sea touching land that no coastal story
/// (ria, skerry, fjord, lagoon, flat, estuary) claimed.
pub const SHORE: u8 = 26;

/// One name per code point, index-aligned — the single vocabulary the
/// inspector, the namer and the diagnostics all read (M60: nobody
/// guesses from raw scalars again). `NONE` on open water reads as the
/// open sea; on land and shore it must never survive `finish`.
pub const NAMES: [&str; 27] = [
    "open sea",
    "raised beach",
    "ria",
    "skerry field",
    "fjord",
    "moraine",
    "drumlin",
    "esker",
    "spillway",
    "outwash plain",
    "patterned ground",
    "tidal flat",
    "estuary",
    "delta plain",
    "spit",
    "barrier island",
    "lagoon",
    "oasis",
    "springline",
    "glacial trough",
    "karst",
    "mountain",
    "hills",
    "plateau",
    "valley",
    "plain",
    "shore",
];

/// A drowned cell counts as a ria when land at least this tall stands
/// within `WALL_R` cells — valley walls, not open flats.
const RIA_WALL: f32 = 0.12;
const WALL_R: isize = 2;

/// Classify every cell of the (possibly widened) grid. Rows map 1:1 to
/// the sea-level row profile — the widen adds columns only.
///
/// `delta` is M59's fan-built land: fresh deposition that post-dates
/// the sea-level history. A delta plain stands at the tideline because
/// the river filled it there *now* — reading `hv − dz < 0` on it would
/// call every fan a raised beach wherever emergence exceeds one fill
/// height. The classifier reads the sea's story only through ground
/// the sea actually shaped.
pub fn classify(
    height: &Array2<f32>,
    sl: &SeaLevel,
    ice: &crate::ice::Ice,
    delta: &Array2<bool>,
) -> Array2<u8> {
    let (h, w) = height.dim();
    let mut out: Array2<u8> = Array2::zeros((h, w));
    // M29 — fjords first: a drowned cell the ice carved is a fjord no
    // matter what the sea-level ledger says about it.
    if ice.carved.dim() == (h, w) {
        for y in 0..h {
            for x in 0..w {
                if height[[y, x]] < 0.0 && ice.carved[[y, x]] >= crate::ice::FJORD_MIN {
                    out[[y, x]] = FJORD;
                }
            }
        }
    }
    let last = sl.row.len().saturating_sub(1);
    for y in 0..h {
        let dz = (sl.row[y.min(last)] - sl.eustatic) as f32;
        if dz == 0.0 {
            continue;
        }
        for x in 0..w {
            let hv = height[[y, x]];
            let h0 = hv - dz;
            if out[[y, x]] == FJORD {
                continue;
            }
            if hv >= 0.0 && h0 < 0.0 {
                // fan-built ground is the river's work, not the sea's
                if !(delta.dim() == (h, w) && delta[[y, x]]) {
                    out[[y, x]] = RAISED;
                }
            } else if hv < 0.0 && h0 >= 0.0 {
                // drowned ground: walls nearby make it a ria, open
                // low relief makes it a skerry field
                let mut walled = false;
                'scan: for ddy in -WALL_R..=WALL_R {
                    for ddx in -WALL_R..=WALL_R {
                        let ny = y as isize + ddy;
                        let nx = x as isize + ddx;
                        if ny < 0 || nx < 0 || ny >= h as isize || nx >= w as isize {
                            continue;
                        }
                        if height[[ny as usize, nx as usize]] >= RIA_WALL {
                            walled = true;
                            break 'scan;
                        }
                    }
                }
                out[[y, x]] = if walled { RIA } else { SKERRY };
            }
        }
    }
    // M30 — the depositional legacy joins the vocabulary: land cells
    // only, and the coastal story wins where the two overlap.
    for (reg, tag) in [
        (&ice.moraines, MORAINE),
        (&ice.drumlins, DRUMLIN),
        (&ice.eskers, ESKER),
    ] {
        for &(y, x) in reg.iter() {
            let (y, x) = (y as usize, x as usize);
            if y < h && x < w && height[[y, x]] >= 0.0 && out[[y, x]] == NONE {
                out[[y, x]] = tag;
            }
        }
    }
    // M31 — the spillways: outburst valleys on land, same precedence.
    for ch in &ice.spillways {
        for &(y, x) in ch.iter() {
            let (y, x) = (y as usize, x as usize);
            if y < h && x < w && height[[y, x]] >= 0.0 && out[[y, x]] == NONE {
                out[[y, x]] = SPILLWAY;
            }
        }
    }
    // M32 — the outwash corridors: braided plains on land, same
    // precedence (the coastal story and the ridge registries win).
    if ice.outwash.dim() == (h, w) {
        for y in 0..h {
            for x in 0..w {
                if height[[y, x]] >= 0.0
                    && out[[y, x]] == NONE
                    && ice.outwash[[y, x]] >= crate::ice::OUT_BRAID_MIN
                {
                    out[[y, x]] = OUTWASH;
                }
            }
        }
    }
    out
}

/// M33 — patterned ground joins the vocabulary after the permafrost
/// pass (which runs post-classify): land cells only, and the coastal
/// story and the glacial registries win where they overlap.
pub fn stamp_patterned(out: &mut Array2<u8>, pattern: &Array2<u8>, height: &Array2<f32>) {
    let (h, w) = out.dim();
    if pattern.dim() != (h, w) {
        return;
    }
    for y in 0..h {
        for x in 0..w {
            if height[[y, x]] >= 0.0 && out[[y, x]] == NONE && pattern[[y, x]] != 0 {
                out[[y, x]] = PATTERNED;
            }
        }
    }
}

/// M43 — the tide must reach mesotidal before it can build a flat or
/// mark a mouth as an estuary.
pub const FLAT_MIN_RANGE: f64 = 2.0;
pub const EST_MIN_RANGE: f64 = 2.0;
/// M43 — a flat forms where the intertidal outcrop spans real ground:
/// range (m) divided by the local slope must stretch at least this
/// many metres of shore. A quarter map cell — a flat you could draw
/// (at 2000 m the fjord-coast seed 777 kept just 2 flat cells; the
/// law's scaling held but the shore read barren).
pub const FLAT_WIDTH_M: f64 = 1000.0;
/// M43 — vertical proximity to the waterline, metres: a candidate
/// cell's mean elevation magnitude must sit within reach of the tide.
pub const FLAT_VERT_M: f64 = 16.0;

/// M43 — the tides join the vocabulary after the tide field is solved
/// (post-widen, like everything coastal). Estuaries first — the mouth
/// outranks the flat on the same cell — then the formation law: the
/// tide builds a flat where its vertical range, spread over the local
/// slope, spans at least `FLAT_WIDTH_M` of shore near the waterline.
/// (Flats are depositional — the tide manufactures them — so the rule
/// reads formation capacity, not pre-existing bathymetry: the strict
/// intertidal-band criterion left 0–6 cells per world at 4 km
/// resolution.) The earlier stories (coastal history, glacial
/// registries, patterned ground) win where they already spoke.
pub fn stamp_tidal(
    out: &mut Array2<u8>,
    tides: &crate::tides::Tides,
    height: &Array2<f32>,
    flags: &Array2<u8>,
) {
    let (h, w) = out.dim();
    if tides.range.dim() != (h, w) || height.dim() != (h, w) || flags.dim() != (h, w) {
        return;
    }
    let river = crate::state::CellFlags::RIVER.bits();
    // Estuary mouths: a river cell on land, touching open tidal water.
    for y in 0..h {
        for x in 0..w {
            if out[[y, x]] != NONE || height[[y, x]] < 0.0 || flags[[y, x]] & river == 0 {
                continue;
            }
            let mut tidal = false;
            for (ny, nx) in [
                (y.wrapping_sub(1), x),
                (y + 1, x),
                (y, x.wrapping_sub(1)),
                (y, x + 1),
            ] {
                if ny < h
                    && nx < w
                    && tides.class[[ny, nx]] == crate::tides::OPEN
                    && tides.range[[ny, nx]] as f64 >= EST_MIN_RANGE
                {
                    tidal = true;
                    break;
                }
            }
            if tidal {
                out[[y, x]] = ESTUARY;
            }
        }
    }
    // Intertidal flats: a waterline cell (open water touching land, or
    // land touching open water) near sea level, where range over slope
    // spans at least FLAT_WIDTH_M of shore.
    let cell_m = crate::constants::KM_PER_CELL * 1000.0;
    for y in 0..h {
        for x in 0..w {
            if out[[y, x]] != NONE {
                continue;
            }
            // Waterline test and the governing range: own range for an
            // open-water cell with a land neighbor, the wettest open
            // neighbor's range for a land cell on the shore.
            let is_open = tides.class[[y, x]] == crate::tides::OPEN;
            let mut r = 0.0f64;
            let mut waterline = false;
            for (ny, nx) in [
                (y.wrapping_sub(1), x),
                (y + 1, x),
                (y, x.wrapping_sub(1)),
                (y, x + 1),
            ] {
                if ny >= h || nx >= w {
                    continue;
                }
                if is_open {
                    if height[[ny, nx]] >= 0.0 {
                        waterline = true;
                    }
                } else if tides.class[[ny, nx]] == crate::tides::OPEN {
                    waterline = true;
                    r = r.max(tides.range[[ny, nx]] as f64);
                }
            }
            if is_open {
                r = tides.range[[y, x]] as f64;
            } else if height[[y, x]] < 0.0 {
                // enclosed water is never a tidal flat
                continue;
            }
            if !waterline || r < FLAT_MIN_RANGE {
                continue;
            }
            // Near the waterline vertically...
            let hv_m = height[[y, x]] as f64 * crate::constants::METRES_PER_UNIT;
            if hv_m.abs() > FLAT_VERT_M {
                continue;
            }
            // ...and gentle enough that the intertidal band spans real
            // ground: slope from the 3×3 relief over 2 cells.
            let mut lo = f32::INFINITY;
            let mut hi = f32::NEG_INFINITY;
            for dy in -1isize..=1 {
                for dx in -1isize..=1 {
                    let ny = y as isize + dy;
                    let nx = x as isize + dx;
                    if ny < 0 || nx < 0 || ny >= h as isize || nx >= w as isize {
                        continue;
                    }
                    let v = height[[ny as usize, nx as usize]];
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
            }
            let slope = ((hi - lo) as f64 * crate::constants::METRES_PER_UNIT / (2.0 * cell_m))
                .max(1e-6);
            if r / slope >= FLAT_WIDTH_M {
                out[[y, x]] = TIDEFLAT;
            }
        }
    }
}

// ------------------------------------------------- M60: the full fold
//
// The era's remaining stories join the one grid, each writing only the
// cells nobody claimed before it. Precedence is the order the dawn
// calls them (world.rs): the sea's history and the ice's registries
// first (classify), then frost (patterned), tide (flats/estuaries),
// river (delta), drift (spit/barrier/lagoon), the dry country's water
// (springs/oases), the ice's dry valleys (troughs) — and finally the
// generic relief vocabulary fills every untold land cell and open
// shore, so no ground is nameless (the M60 totality gate). One
// exception to claim-if-untold: drift-*deposited* ground (spit,
// barrier) overrides earlier claims, because on those cells the drift
// is not a later story about existing ground — it is the ground's
// origin (see stamp_coastforms).

/// M59/M60 — the fan-built plains: land the river laid down where its
/// transport capacity died. Earlier stories keep their cells.
pub fn stamp_delta(out: &mut Array2<u8>, delta: &Array2<bool>, height: &Array2<f32>) {
    let (h, w) = out.dim();
    if delta.dim() != (h, w) || height.dim() != (h, w) {
        return;
    }
    for y in 0..h {
        for x in 0..w {
            if out[[y, x]] == NONE && height[[y, x]] >= 0.0 && delta[[y, x]] {
                out[[y, x]] = DELTA;
            }
        }
    }
}

/// M44/M60 — the drift ledger: spits and barriers are the longshore
/// current's new land, lagoons the world-ocean water that new ground
/// pinched off.
///
/// SPIT and BARRIER overwrite whatever an earlier story claimed: the
/// M44 deposit gate proves those form cells are exactly the cells the
/// drift deposited — ground that did not exist before the current
/// built it. A raised-beach or delta word there is a lie about the
/// ground's origin (measured before this law, every spit and barrier
/// cell on three seeds was claimed first by the sea-history or delta
/// stories and the words never reached the map). LAGOON keeps the
/// claim-if-untold rule: it names standing water, not built ground.
pub fn stamp_coastforms(out: &mut Array2<u8>, form: &Array2<u8>, height: &Array2<f32>) {
    let (h, w) = out.dim();
    if form.dim() != (h, w) || height.dim() != (h, w) {
        return;
    }
    for y in 0..h {
        for x in 0..w {
            let land = height[[y, x]] >= 0.0;
            match form[[y, x]] {
                crate::coast::SPIT if land => out[[y, x]] = SPIT,
                crate::coast::BARRIER if land => out[[y, x]] = BARRIER,
                crate::coast::LAGOON if !land && out[[y, x]] == NONE => out[[y, x]] = LAGOON,
                _ => {}
            }
        }
    }
}

/// M55/M60 — the dry country's water: the springline where the solved
/// table daylights, the oasis where the desert's water actually
/// gathers. M55's `oases` mask is a *reach* law — every arid cell a
/// well can water, deliberately broad because founding prices it —
/// but the landform word names the grove, not the reach: within the
/// mask, OASIS stamps only where depth to the table is a local
/// minimum over the masked 8-neighborhood — the low point the
/// phreatophyte roots find first. Measured before this law, the
/// shallow band alone painted 8–11k cells per world (whole discharge
/// basins — that ground is sabkha/playa country, a word of its own in
/// the Ready queue, not oasis); the low-point law leaves the ~10²
/// pointlike groves an oasis actually is. The siting law reads the
/// mask untouched; only the vocabulary narrows.
pub fn stamp_dry_water(
    out: &mut Array2<u8>,
    springs: &Array2<bool>,
    oases: &Array2<bool>,
    aquifer: &Array2<f32>,
    height: &Array2<f32>,
) {
    let (h, w) = out.dim();
    if springs.dim() != (h, w)
        || oases.dim() != (h, w)
        || aquifer.dim() != (h, w)
        || height.dim() != (h, w)
    {
        return;
    }
    for y in 0..h {
        for x in 0..w {
            if out[[y, x]] != NONE || height[[y, x]] < 0.0 {
                continue;
            }
            if springs[[y, x]] {
                out[[y, x]] = SPRING;
                continue;
            }
            if !oases[[y, x]] {
                continue;
            }
            let d = aquifer[[y, x]];
            // STRICT local minimum of depth over masked neighbors: every
            // masked neighbor sits strictly farther from the water (an
            // isolated masked cell counts — the pointlike case). Strict,
            // because the solve clamps daylighted basins to one flat
            // depth: a `≤ + <` law stamps the entire rim band of every
            // such basin (measured: ~5k ring cells per world), while
            // ties failing outright leaves flat playa floors wordless
            // (they are sabkha country, a Ready-queue word) and keeps
            // the grove at the basin's one nearest approach.
            let mut strict_min = true;
            'nbrs: for dy in -1isize..=1 {
                for dx in -1isize..=1 {
                    if dy == 0 && dx == 0 {
                        continue;
                    }
                    let ny = y as isize + dy;
                    let nx = x as isize + dx;
                    if ny < 0 || nx < 0 || ny >= h as isize || nx >= w as isize {
                        continue;
                    }
                    let (ny, nx) = (ny as usize, nx as usize);
                    if !oases[[ny, nx]] {
                        continue;
                    }
                    if aquifer[[ny, nx]] <= d {
                        strict_min = false;
                        break 'nbrs;
                    }
                }
            }
            if strict_min {
                out[[y, x]] = OASIS;
            }
        }
    }
}

/// M29/M60 — the U-valleys: ground the ice carved at fjord depth that
/// still stands above the waterline. The drowned ones became fjords in
/// `classify`; these are their dry siblings.
pub fn stamp_trough(out: &mut Array2<u8>, carved: &Array2<f32>, height: &Array2<f32>) {
    let (h, w) = out.dim();
    if carved.dim() != (h, w) || height.dim() != (h, w) {
        return;
    }
    for y in 0..h {
        for x in 0..w {
            if out[[y, x]] == NONE
                && height[[y, x]] >= 0.0
                && carved[[y, x]] >= crate::ice::FJORD_MIN
            {
                out[[y, x]] = TROUGH;
            }
        }
    }
}

/// The relief window: ±2 cells (a 20 km square at 4 km/cell) — the
/// scale Hammond's landform classes read local relief at (~10–20 km
/// neighborhoods over 300 m/90 m relief breaks).
const RELIEF_R: isize = 2;
/// Hammond's mountain break: ≥300 m of local relief.
const MOUNTAIN_RELIEF: f32 = (300.0 / crate::constants::METRES_PER_UNIT) as f32;
/// Hammond's hill break: ≥90 m of local relief.
const HILLS_RELIEF: f32 = (90.0 / crate::constants::METRES_PER_UNIT) as f32;
/// A plateau is high, flat ground: ≥1000 m elevation under hill-class
/// relief.
const PLATEAU_ELEV: f32 = (1000.0 / crate::constants::METRES_PER_UNIT) as f32;
/// A valley is the floor of pronounced relief: the window climbs
/// ≥160 m above the cell while the cell sits within 40 m of the
/// window's floor, and the window itself carries ≥200 m of relief.
const VALLEY_RELIEF: f32 = (200.0 / crate::constants::METRES_PER_UNIT) as f32;
const VALLEY_FLOOR: f32 = (40.0 / crate::constants::METRES_PER_UNIT) as f32;
const VALLEY_WALL: f32 = (160.0 / crate::constants::METRES_PER_UNIT) as f32;

/// M60 — the totality pass: every land cell the era's stories left
/// untold resolves to a generic relief class (valley first, so a
/// mountain's floor says valley; then mountain, hills, plateau,
/// plain), and every open-water cell touching land that no coastal
/// story claimed reads as plain shore. After this pass, `NONE`
/// survives only on open sea — the M60 gate's totality clause.
pub fn finish(out: &mut Array2<u8>, height: &Array2<f32>) {
    let (h, w) = out.dim();
    if height.dim() != (h, w) {
        return;
    }
    // Separable 5×5 window min/max: rows, then columns.
    let mut rmin = Array2::from_elem((h, w), 0.0f32);
    let mut rmax = Array2::from_elem((h, w), 0.0f32);
    for y in 0..h {
        for x in 0..w {
            let mut lo = f32::INFINITY;
            let mut hi = f32::NEG_INFINITY;
            for dx in -RELIEF_R..=RELIEF_R {
                let nx = x as isize + dx;
                if nx < 0 || nx >= w as isize {
                    continue;
                }
                let v = height[[y, nx as usize]];
                lo = lo.min(v);
                hi = hi.max(v);
            }
            rmin[[y, x]] = lo;
            rmax[[y, x]] = hi;
        }
    }
    for y in 0..h {
        for x in 0..w {
            if out[[y, x]] != NONE {
                continue;
            }
            let hv = height[[y, x]];
            if hv < 0.0 {
                // open water: shore iff a 4-neighbor is land
                let mut shore = false;
                for (ny, nx) in [
                    (y.wrapping_sub(1), x),
                    (y + 1, x),
                    (y, x.wrapping_sub(1)),
                    (y, x + 1),
                ] {
                    if ny < h && nx < w && height[[ny, nx]] >= 0.0 {
                        shore = true;
                        break;
                    }
                }
                if shore {
                    out[[y, x]] = SHORE;
                }
                continue;
            }
            let mut lo = f32::INFINITY;
            let mut hi = f32::NEG_INFINITY;
            for dy in -RELIEF_R..=RELIEF_R {
                let ny = y as isize + dy;
                if ny < 0 || ny >= h as isize {
                    continue;
                }
                lo = lo.min(rmin[[ny as usize, x]]);
                hi = hi.max(rmax[[ny as usize, x]]);
            }
            let relief = hi - lo;
            out[[y, x]] = if relief >= VALLEY_RELIEF && hv - lo <= VALLEY_FLOOR && hi - hv >= VALLEY_WALL {
                VALLEY
            } else if relief >= MOUNTAIN_RELIEF {
                MOUNTAIN
            } else if relief >= HILLS_RELIEF {
                HILLS
            } else if hv >= PLATEAU_ELEV {
                PLATEAU
            } else {
                PLAIN
            };
        }
    }
}

/// FNV-1a over the tag grid — joins `hash_state` so the classifier
/// cannot drift silently between generations or runtimes.
pub fn hash(grid: &Array2<u8>) -> u64 {
    fnv1a64(grid.as_slice().expect("landform grid is contiguous"))
}

// ---------------------------------------------------------------- bands

use crate::util::Band;

/// Diagnostics bands (E2.7): coastal-landform frequency, normalized by
/// the sea-level-curve amplitude that made it (the M26 gate) — a world
/// that moved its waterline twice as far should show roughly twice the
/// coast rewritten. Ranges calibrated on the three report seeds.
pub const BANDS: &[Band] = &[
    Band { name: "raised coast per stand", sweet: (70.0, 150.0), hard: (40.0, 250.0), target: "sweet 70–150 · hard 40–250 (share of coast per mean emergence, ≈1/coastal slope)" },
    Band { name: "drowned coast per stand", sweet: (60.0, 150.0), hard: (25.0, 300.0), target: "sweet 60–150 · hard 25–300 (share of coast per mean submergence)" },
    Band { name: "oasis cells stay pointlike", sweet: (50.0, 800.0), hard: (10.0, 2000.0), target: "sweet 50–800 · hard 10–2000 (M60 gate: strict depth minima of the well-reach mask — groves, not basins; measured 228–305 ×5 seeds)" },
    // M64 — calibration vs Earth: the coast census and the expressive
    // range of the vocabulary itself. Earth's coast-type frequencies
    // are shares of coastline LENGTH, so the census counts shoreline
    // frontage — areal words only where they front the sea. Barrier
    // islands alone front ~10% of Earth's open-ocean coast (Stutz &
    // Pilkey 2011); spits and lagoons ride the same drift-built belt.
    // The entropy/dominance/JSD floors keep any seed from collapsing
    // to one bland word.
    Band { name: "built belt share of coast %", sweet: (1.5, 30.0), hard: (0.3, 55.0), target: "M64: Earth's drift-built belt ≈10% of open coast by length (Stutz & Pilkey 2011) — frontage census" },
    Band { name: "coast with a story %", sweet: (10.0, 90.0), hard: (2.0, 98.0), target: "M64: most coast frontage carries a named form; open shore is the remainder, not the rule" },
    Band { name: "landform entropy floor", sweet: (1.0, 3.3), hard: (0.6, 3.3), target: "M64: worst seed keeps a mixed vocabulary (Shannon entropy over the 27 words; ln 27 ≈ 3.3)" },
    Band { name: "dominant landform share", sweet: (0.15, 0.65), hard: (0.05, 0.85), target: "M64: no seed pinned to one word — the commonest landform stays under ~2/3 of land" },
    Band { name: "landform oatmeal floor", sweet: (0.004, 1.0), hard: (0.0015, 1.0), target: "M64: the landform mix differs between worlds — the closest pair stays distinct" },
];
