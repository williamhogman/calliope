//! M63 — The Atlas Learns: the one source of cartographic truth.
//!
//! The hypsometric ramps, the cross-blend law that keys them to climate,
//! and the categorical palettes for the deep-earth lenses (rock province,
//! soil order, landform) live HERE, in Rust tables. The WGSL the GPU
//! compiles is *generated* from these tables (`wgsl_ramps`), the palette
//! texture the shader samples is *built* from them (`palette_texture`),
//! and the diagnostics gate proves laws against the same functions the
//! renderer uses (`hypso_rgb`) — three consumers, one vocabulary, no
//! drift by construction.

use crate::agriculture::SoilOrder;
use crate::{landform, rock};

// ------------------------------------------------------------- hypsometry

/// M7.4 — the humid hypsometric ladder: green lowlands through straw
/// uplands to grey scree and summit firn. `(elevation 0..1, rgb 0..255)`.
pub const ELEV_STOPS: [(f32, [f32; 3]); 7] = [
    (0.00, [77.0, 124.0, 68.0]),
    (0.15, [125.0, 154.0, 82.0]),
    (0.30, [176.0, 168.0, 107.0]),
    (0.45, [140.0, 122.0, 91.0]),
    (0.62, [152.0, 145.0, 138.0]),
    (0.80, [201.0, 201.0, 201.0]),
    (1.00, [244.0, 244.0, 244.0]),
];

/// M7.4 — the dry-country ladder: ochre lowlands through rust-brown
/// uplands to pale desert-varnish summits. No green anywhere on it —
/// that absence is the law the M63 gate measures.
pub const ELEV_ARID_STOPS: [(f32, [f32; 3]); 7] = [
    (0.00, [163.0, 145.0, 99.0]),
    (0.15, [185.0, 159.0, 104.0]),
    (0.30, [199.0, 168.0, 110.0]),
    (0.45, [178.0, 139.0, 97.0]),
    (0.62, [152.0, 118.0, 92.0]),
    (0.80, [206.0, 197.0, 188.0]),
    (1.00, [246.0, 244.0, 240.0]),
];

/// Piecewise-linear ramp over `(stop, rgb)` control points; returns 0..1.
pub fn ramp(stops: &[(f32, [f32; 3])], v: f32) -> [f32; 3] {
    let (s0, c0) = stops[0];
    if v <= s0 {
        return [c0[0] / 255.0, c0[1] / 255.0, c0[2] / 255.0];
    }
    for w in stops.windows(2) {
        let (a, ca) = w[0];
        let (b, cb) = w[1];
        if v <= b {
            let t = ((v - a) / (b - a)).clamp(0.0, 1.0);
            return [
                (ca[0] + (cb[0] - ca[0]) * t) / 255.0,
                (ca[1] + (cb[1] - ca[1]) * t) / 255.0,
                (ca[2] + (cb[2] - ca[2]) * t) / 255.0,
            ];
        }
    }
    let (_, cl) = stops[stops.len() - 1];
    [cl[0] / 255.0, cl[1] / 255.0, cl[2] / 255.0]
}

/// The cross-blended hypsometric law (M7.4/M63), exactly as the shader
/// applies it on the elevation lens: wet country climbs the green ramp,
/// dry country the ochre one, and frost greys both toward summit firn.
/// This is the native reference the diagnostics gate interrogates.
pub fn hypso_rgb(h: f32, precip_mm: f32, tmean_c: f32) -> [f32; 3] {
    let arid = 1.0 - ((precip_mm - 240.0) / 700.0).clamp(0.0, 1.0);
    let g = ramp(&ELEV_STOPS, h);
    let a = ramp(&ELEV_ARID_STOPS, h);
    let mut c = [
        g[0] + (a[0] - g[0]) * arid,
        g[1] + (a[1] - g[1]) * arid,
        g[2] + (a[2] - g[2]) * arid,
    ];
    let chill = ((-2.0 - tmean_c) / 14.0).clamp(0.0, 1.0) * 0.85;
    let h01 = h.clamp(0.0, 1.0);
    let polar = [
        0.60 + (0.93 - 0.60) * h01,
        0.64 + (0.94 - 0.64) * h01,
        0.69 + (0.96 - 0.69) * h01,
    ];
    for i in 0..3 {
        c[i] += (polar[i] - c[i]) * chill;
    }
    c
}

/// HSV hue in degrees (0..360) of an rgb triple in 0..1. Grey (zero
/// chroma) reports −1: no hue claim at all.
pub fn hue_deg(c: [f32; 3]) -> f32 {
    let mx = c[0].max(c[1]).max(c[2]);
    let mn = c[0].min(c[1]).min(c[2]);
    let d = mx - mn;
    if d < 1e-6 {
        return -1.0;
    }
    let h = if mx == c[0] {
        60.0 * (((c[1] - c[2]) / d) % 6.0)
    } else if mx == c[1] {
        60.0 * ((c[2] - c[0]) / d + 2.0)
    } else {
        60.0 * ((c[0] - c[1]) / d + 4.0)
    };
    if h < 0.0 { h + 360.0 } else { h }
}

// ------------------------------------------------------------- palettes

/// Geology lens — the four rock provinces (M18), in the tradition of
/// printed geologic maps: crystalline basement rose, sedimentary basin
/// blue, metamorphic belt violet, volcanic country rust.
/// Index-aligned with `rock::NAMES`.
pub const ROCK_COLORS: [[u8; 3]; 4] = [
    [197, 116, 115], // shield — granite rose
    [122, 158, 196], // basin — limestone blue
    [156, 124, 176], // fold belt — marble violet
    [172, 81, 56],   // volcanic — basalt rust
];

/// Soils lens — one swatch per soil order (M51/M52), soil-atlas
/// convention: the black earth nearly black, podzol ash-pale, laterite
/// iron-red, loess gold. Index-aligned with `SoilOrder` codes.
pub const SOIL_COLORS: [[u8; 3]; 11] = [
    [24, 36, 54],    // none — open water
    [141, 137, 130], // lithosol — bare grey
    [164, 158, 172], // podzol — ash lavender
    [166, 128, 87],  // cambisol — brown earth
    [77, 60, 48],    // chernozem — black earth
    [187, 91, 54],   // laterite — iron red
    [104, 82, 76],   // andosol — dark ash umber
    [111, 134, 143], // gley — waterlogged blue-grey
    [212, 186, 138], // aridisol — pale desert tan
    [148, 152, 94],  // fluvisol — silt olive
    [226, 199, 133], // loess — wind-dust gold
];

/// Landform lens — one swatch per vocabulary word (M60), grouped by the
/// story that made the ground: coastal words in sea-and-sand hues, the
/// glacial legacy in violets, the dry country's water in living greens,
/// the generic relief in earth tones. Index-aligned with
/// `landform::NAMES`.
pub const LANDFORM_COLORS: [[u8; 3]; 27] = [
    [14, 25, 42],    // open sea
    [214, 191, 150], // raised beach
    [95, 152, 178],  // ria
    [130, 170, 186], // skerry field
    [58, 110, 160],  // fjord
    [148, 124, 158], // moraine
    [170, 146, 186], // drumlin
    [196, 168, 200], // esker
    [124, 108, 170], // spillway
    [186, 178, 200], // outwash
    [156, 176, 192], // patterned ground
    [168, 162, 122], // tideflat
    [108, 160, 148], // estuary
    [98, 168, 108],  // delta
    [222, 204, 146], // spit
    [232, 216, 162], // barrier
    [86, 178, 190],  // lagoon
    [70, 160, 84],   // oasis
    [110, 196, 168], // spring
    [132, 150, 196], // trough
    [150, 150, 134], // karst (reserved)
    [138, 106, 82],  // mountain
    [172, 140, 96],  // hills
    [190, 158, 106], // plateau
    [136, 158, 92],  // valley
    [196, 184, 128], // plain
    [72, 118, 152],  // shore
];

/// Palette rows, in the order the shader indexes them (`tPal` row =
/// lens): 0 rock, 1 soil, 2 landform.
pub const PAL_ROWS: usize = 3;

/// The 256×3 RGBA8 palette texture the deep-earth lenses sample: row 0
/// rock provinces, row 1 soil orders, row 2 landform vocabulary. Unused
/// code points stay a loud magenta so an id past the vocabulary is
/// visible instead of silently black.
pub fn palette_texture() -> Vec<u8> {
    let mut px = vec![0u8; 256 * PAL_ROWS * 4];
    for x in 0..256 {
        for row in 0..PAL_ROWS {
            let o = (row * 256 + x) * 4;
            px[o] = 226;
            px[o + 1] = 24;
            px[o + 2] = 210;
            px[o + 3] = 255;
        }
    }
    let mut put = |row: usize, x: usize, c: [u8; 3]| {
        let o = (row * 256 + x) * 4;
        px[o] = c[0];
        px[o + 1] = c[1];
        px[o + 2] = c[2];
        px[o + 3] = 255;
    };
    for (i, &c) in ROCK_COLORS.iter().enumerate() {
        put(0, i, c);
    }
    for (i, &c) in SOIL_COLORS.iter().enumerate() {
        put(1, i, c);
    }
    for (i, &c) in LANDFORM_COLORS.iter().enumerate() {
        put(2, i, c);
    }
    px
}

/// Compile-time-ish sanity: the palettes cover their vocabularies
/// exactly and no two words inside one lens share a swatch. Called by
/// the diagnostics gate; cheap enough to call anywhere.
pub fn palettes_sound() -> Result<(), String> {
    if LANDFORM_COLORS.len() != landform::NAMES.len() {
        return Err(format!(
            "landform palette carries {} swatches for {} words",
            LANDFORM_COLORS.len(),
            landform::NAMES.len()
        ));
    }
    if ROCK_COLORS.len() != rock::NAMES.len() {
        return Err(format!(
            "rock palette carries {} swatches for {} provinces",
            ROCK_COLORS.len(),
            rock::NAMES.len()
        ));
    }
    if SOIL_COLORS.len() != <SoilOrder as strum::EnumCount>::COUNT {
        return Err(format!(
            "soil palette carries {} swatches for {} orders",
            SOIL_COLORS.len(),
            <SoilOrder as strum::EnumCount>::COUNT
        ));
    }
    for (label, pal) in [
        ("rock", &ROCK_COLORS[..]),
        ("soil", &SOIL_COLORS[..]),
        ("landform", &LANDFORM_COLORS[..]),
    ] {
        for i in 0..pal.len() {
            for j in (i + 1)..pal.len() {
                if pal[i] == pal[j] {
                    return Err(format!("{label} palette repeats a swatch at {i} and {j}"));
                }
            }
        }
    }
    Ok(())
}

// ------------------------------------------------------------- WGSL codegen

fn wgsl_ramp_fn(name: &str, stops: &[(f32, [f32; 3])]) -> String {
    let mut s = format!("fn {name}(v: f32) -> vec3<f32> {{\n");
    let (a0, c0) = stops[0];
    let (a1, c1) = stops[1];
    s.push_str(&format!(
        "  var c = seg(v, {a0:?}, {a1:?}, vec3<f32>({:?}, {:?}, {:?}), vec3<f32>({:?}, {:?}, {:?}));\n",
        c0[0], c0[1], c0[2], c1[0], c1[1], c1[2]
    ));
    for w in stops.windows(2).skip(1) {
        let (a, ca) = w[0];
        let (b, cb) = w[1];
        s.push_str(&format!(
            "  if (v > {a:?}) {{ c = seg(v, {a:?}, {b:?}, vec3<f32>({:?}, {:?}, {:?}), vec3<f32>({:?}, {:?}, {:?})); }}\n",
            ca[0], ca[1], ca[2], cb[0], cb[1], cb[2]
        ));
    }
    s.push_str("  return c / 255.0;\n}\n");
    s
}

/// The generated WGSL prelude: `seg`, both hypsometric ramps and the
/// cross-blend law, emitted from the same tables `hypso_rgb` reads. The
/// renderer concatenates this ahead of its main shader source, so the
/// GPU compiles the numbers the gate proved.
pub fn wgsl_ramps() -> String {
    let mut s = String::with_capacity(2048);
    s.push_str("// GENERATED from atlas.rs tables (M63) — the gate proves these numbers.\n");
    s.push_str(
        "fn seg(v: f32, a: f32, b: f32, ca: vec3<f32>, cb: vec3<f32>) -> vec3<f32> {\n  return mix(ca, cb, clamp((v - a) / (b - a), 0.0, 1.0));\n}\n",
    );
    s.push_str(&wgsl_ramp_fn("elev_ramp", &ELEV_STOPS));
    s.push_str(&wgsl_ramp_fn("elev_arid_ramp", &ELEV_ARID_STOPS));
    s.push_str(
        "fn hypso(h: f32, precipmm: f32, tmeanc: f32) -> vec3<f32> {\n\
         \x20 let arid = 1.0 - clamp((precipmm - 240.0) / 700.0, 0.0, 1.0);\n\
         \x20 var hyp = mix(elev_ramp(h), elev_arid_ramp(h), arid);\n\
         \x20 let chill = clamp((-2.0 - tmeanc) / 14.0, 0.0, 1.0);\n\
         \x20 let polar = mix(vec3<f32>(0.60, 0.64, 0.69), vec3<f32>(0.93, 0.94, 0.96), clamp(h, 0.0, 1.0));\n\
         \x20 return mix(hyp, polar, chill * 0.85);\n}\n",
    );
    s
}
