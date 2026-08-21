//! Soil fertility — port of agriculture.py — and the crop packages of
//! M2: every land cell gets the agriculture its temperature, rainfall
//! and irrigation actually support (research/08: GAEZ multiplicative
//! suitability, USDA climate profiles, the <300 mm pastoral boundary).
//!
//! **M51 — soil genesis.** Fertility is no longer one climate scalar
//! with deposits bolted on: the ground itself is classified into soil
//! orders by Jenny's *clorpt* factors — parent material (M18's rock
//! province), climate (warmth and the leaching balance), organisms (the
//! biome standing on it), relief (slope) and a time proxy (how long the
//! surface has stood: young volcanic ash and glacial till versus the
//! deeply weathered tropical shield). Each order carries its own
//! fertility, drainage and depth curves, and that curve — not a guess —
//! modulates the arable index the farms read.

use ndarray::Array2;

use crate::ndimage;


pub fn fertility(
    height: &Array2<f64>,
    tmean: &Array2<f64>,
    precip: &Array2<f64>,
    rivers: &Array2<bool>,
    lakes: &Array2<bool>,
    discharge: &Array2<f64>,
    till: &Array2<f32>,
    loess: &Array2<f32>,
    outwash: &Array2<f32>,
    soil: &Array2<u8>,
) -> Array2<f64> {
    let (rows, cols) = height.dim();
    let hpos = height.mapv(|v| v.max(0.0));
    let (gy, gx) = ndimage::gradient(&hpos);

    let px = [0.0, 150.0, 450.0, 900.0, 1600.0, 2600.0, 4000.0];
    let py = [0.0, 0.08, 0.55, 1.0, 0.9, 0.5, 0.3];

    let mut fert = Array2::<f64>::zeros((rows, cols));
    let mut tgrid = Array2::<f64>::zeros((rows, cols));
    for y in 0..rows {
        for x in 0..cols {
            let t = (-(((tmean[[y, x]] - 17.0) / 11.0).powi(2))).exp();
            tgrid[[y, x]] = t;
            let p = crate::util::interp(precip[[y, x]], &px, &py);
            let slope = gy[[y, x]].hypot(gx[[y, x]]) * rows as f64 / 8.0;
            let sp = 1.0 / (1.0 + (slope * 2.2).powi(2));
            fert[[y, x]] = 0.9 * t * p * sp;
        }
    }

    // alluvial floodplains: big rivers lay down silt as they wander
    let silt_src = Array2::from_shape_fn((rows, cols), |(y, x)| {
        if rivers[[y, x]] {
            (1.0 + discharge[[y, x]]).ln()
        } else {
            0.0
        }
    });
    let silt = ndimage::gaussian_filter(&silt_src, 2.2);

    // lakeshores hold moisture
    let shore_wide = ndimage::binary_dilation(lakes, 2);

    for y in 0..rows {
        for x in 0..cols {
            let mut v = fert[[y, x]];
            v += (silt[[y, x]] * 0.08).clamp(0.0, 0.35) * tgrid[[y, x]];
            if shore_wide[[y, x]] && !lakes[[y, x]] {
                v += 0.08;
            }
            // M30 — the depositional legacy: what the ice ground and
            // dropped feeds the farms that follow, where the climate can
            // use it. Till under the old sheet; loess blown equatorward
            // of it — the belt that actually reaches farm country.
            v += till[[y, x]] as f64 * crate::ice::TILL_FERT * tgrid[[y, x]];
            v += loess[[y, x]] as f64 * crate::ice::LOESS_FERT * tgrid[[y, x]];
            // M32 — outwash plains: glacial silt over gravel, leaner
            // than till or loess but real where the climate can farm.
            v += outwash[[y, x]] as f64 * crate::ice::OUT_FERT * tgrid[[y, x]];
            // M51 — the ground the climate is standing on. The order's
            // own fertility curve scales the arable index, softened by
            // SOIL_GAIN so the soil map bends the farms rather than
            // replacing every band calibrated before it (M53 re-bases
            // the crop tables on the orders in full).
            let m = SoilOrder::from_code(soil[[y, x]]).fertility();
            v *= 1.0 + SOIL_GAIN * (m - 1.0);
            if height[[y, x]] < 0.0 || lakes[[y, x]] {
                v = 0.0;
            }
            fert[[y, x]] = v.clamp(0.0, 1.0);
        }
    }
    fert
}

// ----------------------------------------------------------------- soil
// M51 — soil genesis. Jenny (1941): S = f(cl, o, r, p, t). We solve the
// five factors as a precedence ladder rather than a blend, because the
// orders *are* the outcome classes of that ladder: relief and standing
// water veto everything (no profile develops on a scree face or under a
// water table), then parent material where it is young enough to still
// speak (volcanic ash), then the climate's leaching balance (arid,
// tropical, boreal), then the organisms (grassland mollic humus), and
// what is left is the temperate brown earth every textbook calls the
// unremarkable case.

/// How hard the order's fertility curve pulls on the arable index.
/// Below 1.0 deliberately: M51 introduces the soil map, M53 re-bases the
/// crop suitability tables on it — this keeps the pre-soil economy bands
/// meaningful across the transition instead of invalidating them twice.
pub const SOIL_GAIN: f64 = 0.6;

/// Slope (in `fertility`'s scaled units) above which no profile holds:
/// the regolith walks downhill as fast as it forms.
const LITHO_SLOPE: f64 = 1.10;
/// Slope below which water can pond rather than run — the gley gate.
const FLAT_SLOPE: f64 = 0.32;

/// The soil orders (M51). Codes are stable — they ship in the pack.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    strum::Display,
    strum::EnumIter,
    strum::IntoStaticStr,
    strum::EnumCount,
)]
#[strum(serialize_all = "lowercase")]
#[repr(u8)]
pub enum SoilOrder {
    /// Open water and lake floor — no profile.
    None = 0,
    /// Skeletal mountain and ice-scoured rock: relief wins.
    Lithosol = 1,
    /// Acid, leached, ash-grey horizon under cool conifer and heath.
    Podzol = 2,
    /// The temperate brown earth: weathered, unremarkable, workable.
    Cambisol = 3,
    /// Deep mollic humus under continental grassland — the black earth.
    Chernozem = 4,
    /// Hot, wet, ancient: iron and alumina left after the rest leached.
    Laterite = 5,
    /// Young volcanic ash — mineral-rich because it has not had time.
    Andosol = 6,
    /// Waterlogged and airless: the table sits inside the root zone.
    Gley = 7,
    /// Dryland profile: thin, saline-prone, carbonate at depth.
    Aridisol = 8,
}

impl SoilOrder {
    #[inline]
    pub const fn code(self) -> u8 {
        self as u8
    }

    #[inline]
    pub fn from_code(c: u8) -> SoilOrder {
        match c {
            1 => SoilOrder::Lithosol,
            2 => SoilOrder::Podzol,
            3 => SoilOrder::Cambisol,
            4 => SoilOrder::Chernozem,
            5 => SoilOrder::Laterite,
            6 => SoilOrder::Andosol,
            7 => SoilOrder::Gley,
            8 => SoilOrder::Aridisol,
            _ => SoilOrder::None,
        }
    }

    pub fn name(self) -> &'static str {
        self.into()
    }

    /// Fertility curve: multiplier on the climatic arable index, with
    /// the temperate brown earth as the unit case.
    #[inline]
    pub const fn fertility(self) -> f64 {
        match self {
            SoilOrder::None => 0.0,
            SoilOrder::Lithosol => 0.35,
            SoilOrder::Podzol => 0.70,
            SoilOrder::Cambisol => 1.00,
            SoilOrder::Chernozem => 1.35,
            SoilOrder::Laterite => 0.60,
            SoilOrder::Andosol => 1.20,
            SoilOrder::Gley => 0.85,
            SoilOrder::Aridisol => 0.45,
        }
    }

    /// Drainage curve, 0 (waterlogged) .. 1 (freely draining). Read by
    /// M53's per-order crop suitability — paddy rice wants the low end.
    #[inline]
    pub const fn drainage(self) -> f64 {
        match self {
            SoilOrder::None => 0.0,
            SoilOrder::Lithosol => 0.95,
            SoilOrder::Podzol => 0.75,
            SoilOrder::Cambisol => 0.65,
            SoilOrder::Chernozem => 0.60,
            SoilOrder::Laterite => 0.80,
            SoilOrder::Andosol => 0.70,
            SoilOrder::Gley => 0.10,
            SoilOrder::Aridisol => 0.55,
        }
    }

    /// Rooting depth in metres — the profile a plough or a taproot has.
    #[inline]
    pub const fn depth(self) -> f64 {
        match self {
            SoilOrder::None => 0.0,
            SoilOrder::Lithosol => 0.15,
            SoilOrder::Podzol => 0.60,
            SoilOrder::Cambisol => 1.00,
            SoilOrder::Chernozem => 1.60,
            SoilOrder::Laterite => 2.40,
            SoilOrder::Andosol => 0.90,
            SoilOrder::Gley => 1.10,
            SoilOrder::Aridisol => 0.35,
        }
    }
}

/// The climate window a chernozem needs: continental grassland, cold
/// enough that humus outlasts the summer, dry enough that the rain never
/// flushes the profile. The gate reads this same function, so the check
/// cannot drift from the classifier.
#[inline]
pub fn chernozem_climate(tmean: f64, precip: f64) -> bool {
    (-3.0..=16.0).contains(&tmean) && (240.0..=850.0).contains(&precip)
}

/// Classify every land cell into a soil order. Pure function of the
/// finished physical world (ADR-0003) — no RNG, no wall clock.
#[allow(clippy::too_many_arguments)]
pub fn soil_genesis(
    height: &Array2<f64>,
    tmean: &Array2<f64>,
    precip: &Array2<f64>,
    biomes: &Array2<u8>,
    rock: &Array2<u8>,
    rivers: &Array2<bool>,
    lakes: &Array2<bool>,
    discharge: &Array2<f64>,
    till: &Array2<f32>,
    loess: &Array2<f32>,
) -> Array2<u8> {
    use crate::constants as gc;

    let (rows, cols) = height.dim();
    let hpos = height.mapv(|v| v.max(0.0));
    let (gy, gx) = ndimage::gradient(&hpos);
    // The floodplain reach: one ring off the channel is still valley floor.
    let wet_near = ndimage::binary_dilation(
        &Array2::from_shape_fn((rows, cols), |(y, x)| {
            rivers[[y, x]] && discharge[[y, x]] > 40.0
        }),
        1,
    );
    let lake_near = ndimage::binary_dilation(lakes, 1);

    Array2::from_shape_fn((rows, cols), |(y, x)| {
        if height[[y, x]] < 0.0 || lakes[[y, x]] {
            return SoilOrder::None.code();
        }
        let t = tmean[[y, x]];
        let p = precip[[y, x]];
        let b = biomes[[y, x]];
        let slope = gy[[y, x]].hypot(gx[[y, x]]) * rows as f64 / 8.0;

        // (r) relief and the permanent cold: no profile survives either.
        if slope > LITHO_SLOPE || b == gc::ICE || t < -8.0 {
            return SoilOrder::Lithosol.code();
        }
        // (p, t) parent material young enough to still be the story:
        // volcanic ash weathers to andosol wherever the climate works it.
        if rock[[y, x]] == crate::rock::VOLCANIC && t > 2.0 && p > 350.0 {
            return SoilOrder::Andosol.code();
        }
        // (cl, r) standing water: flat valley floor or lakeside, or a
        // flat cold surface where the melt has nowhere to go.
        if slope < FLAT_SLOPE
            && (wet_near[[y, x]] || lake_near[[y, x]] || p > 1900.0 || (t < 0.0 && p > 250.0))
        {
            return SoilOrder::Gley.code();
        }
        // (cl) the dry end: evaporation beats percolation.
        if p < 300.0 || b == gc::DESERT {
            return SoilOrder::Aridisol.code();
        }
        // (cl, t) hot and wet and old: everything soluble is long gone.
        if t >= 19.0 && p >= 1300.0 {
            return SoilOrder::Laterite.code();
        }
        // (cl, o) cool conifer and heath: acid litter, leached horizon.
        if t < 6.0 && p >= 320.0 {
            return SoilOrder::Podzol.code();
        }
        // (o, p) the mollic case: grassland humus, and the loess and
        // till belts whose fresh mineral dust builds the same profile.
        let grassy = b == gc::GRASSLAND
            || b == gc::SAVANNA
            || loess[[y, x]] > 0.15
            || (till[[y, x]] > 0.20 && b == gc::WOODLAND);
        if grassy && chernozem_climate(t, p) && slope < 0.60 {
            return SoilOrder::Chernozem.code();
        }
        SoilOrder::Cambisol.code()
    })
}

// ---------------------------------------------------------------- crops
// M2.1 — crop packages. Codes are stable (they ship in the pack);
// the enum is the single declaration (E1.10) — names, codes and dawn-age
// densities live on the variants, no parallel arrays.

/// Souls per km² a package feeds at dawn-age arts (research/08:
/// hunter-gatherer 0.05–0.4 · pastoral 2 · wheat ~30 · rice ~90–120).
/// Kaplan's T^−0.5 (land per soul shrinks as arts accumulate) raises
/// these through `society::Mods::kaplan`.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    strum::Display,
    strum::EnumIter,
    strum::IntoStaticStr,
    strum::EnumCount,
)]
#[strum(serialize_all = "lowercase")]
#[repr(u8)]
pub enum CropPackage {
    Wildland = 0,
    Wheat = 1,
    Rice = 2,
    Maize = 3,
    Pastoral = 4,
}

impl CropPackage {
    #[inline]
    pub const fn code(self) -> u8 {
        self as u8
    }

    #[inline]
    pub fn from_code(c: u8) -> CropPackage {
        match c {
            1 => CropPackage::Wheat,
            2 => CropPackage::Rice,
            3 => CropPackage::Maize,
            4 => CropPackage::Pastoral,
            _ => CropPackage::Wildland,
        }
    }

    pub fn name(self) -> &'static str {
        self.into()
    }

    #[inline]
    pub const fn density(self) -> f64 {
        match self {
            CropPackage::Wildland => 0.4,
            CropPackage::Wheat => 30.0,
            CropPackage::Rice => 90.0,
            CropPackage::Maize => 22.0,
            CropPackage::Pastoral => 2.0,
        }
    }
}

#[inline]
fn gauss(v: f64, mu: f64, sig: f64) -> f64 {
    (-((v - mu) / sig).powi(2)).exp()
}

/// Trapezoid suitability over precipitation: 0 at `lo`, 1 at `peak`,
/// easing to 0.35 at `hi`, 0 beyond — crops drown slower than they parch.
#[inline]
fn trap(p: f64, lo: f64, peak: f64, hi: f64) -> f64 {
    if p <= lo || p >= hi {
        0.0
    } else if p < peak {
        (p - lo) / (peak - lo)
    } else {
        1.0 - 0.65 * (p - peak) / (hi - peak)
    }
}

/// Classify every cell into the crop package that wins it. Deterministic,
/// pure function of climate + water adjacency; rivers and lakeshores count
/// as irrigable floodplain (paddies and canal-fed fields ignore a dry sky).
pub fn crop_packages(
    height: &Array2<f64>,
    tmean: &Array2<f64>,
    precip: &Array2<f64>,
    rivers: &Array2<bool>,
    lakes: &Array2<bool>,
) -> Array2<u8> {
    let (rows, cols) = height.dim();
    let riv = ndimage::binary_dilation(rivers, 1);
    let lak = ndimage::binary_dilation(lakes, 1);
    Array2::from_shape_fn((rows, cols), |(y, x)| {
        if height[[y, x]] < 0.0 || lakes[[y, x]] {
            return CropPackage::Wildland.code();
        }
        let t = tmean[[y, x]];
        let p = precip[[y, x]];
        let irrigated = riv[[y, x]] || lak[[y, x]];
        // no growing season at all: the high ice and the deep tundra
        if t < 1.5 {
            return if t >= -4.0 && p >= 130.0 {
                CropPackage::Pastoral.code()
            } else {
                CropPackage::Wildland.code()
            };
        }
        let mut wheat = gauss(t, 12.0, 8.0) * trap(p, 270.0, 700.0, 1400.0);
        let mut maize = gauss(t, 21.0, 6.0) * trap(p, 430.0, 950.0, 2000.0);
        let mut rice = gauss(t, 26.0, 5.5) * trap(p, 900.0, 1700.0, 4200.0);
        if irrigated {
            if t > 17.0 {
                rice = rice.max(0.75 * gauss(t, 26.0, 6.5));
            }
            wheat = wheat.max(0.70 * gauss(t, 12.0, 8.0));
            maize = maize.max(0.60 * gauss(t, 21.0, 6.0));
        } else if p < 300.0 {
            // the pastoral boundary: below ~300 mm farming fails (research/08)
            wheat = 0.0;
            maize = 0.0;
            rice = 0.0;
        }
        let pastoral = 0.32 * gauss(t, 12.0, 18.0) * trap(p, 110.0, 420.0, 1500.0);
        let best = wheat.max(maize).max(rice).max(pastoral);
        if best < 0.07 {
            CropPackage::Wildland.code()
        } else if rice >= best {
            CropPackage::Rice.code()
        } else if maize >= best {
            CropPackage::Maize.code()
        } else if wheat >= best {
            CropPackage::Wheat.code()
        } else {
            CropPackage::Pastoral.code()
        }
    })
}

// ---------------------------------------------------------------- bands

use crate::util::Band;

/// Diagnostics bands (E2.7): where fields can feed a city.
pub const BANDS: &[Band] = &[
    Band { name: "arable share of land", sweet: (0.15, 0.65), hard: (0.06, 0.85), target: "M2.1: wheat+rice+maize belts cover the good land" },
    Band { name: "famine events per century", sweet: (1.0, 60.0), hard: (0.0, 150.0), target: "M2.6: the rains must fail sometimes" },
    // M51 — the soil-order mix. Bands are shares of *land*, read against
    // the Whittaker distribution the same world already classifies: no
    // order may vanish (a world with no black earth has no breadbasket)
    // and none may eat the map.
    Band { name: "lithosol share of land", sweet: (0.05, 0.45), hard: (0.01, 0.65), target: "M51: the steep and the frozen carry no profile" },
    Band { name: "podzol share of land", sweet: (0.02, 0.35), hard: (0.003, 0.55), target: "M51: cool conifer country leaches acid" },
    Band { name: "cambisol share of land", sweet: (0.05, 0.45), hard: (0.01, 0.65), target: "M51: the temperate brown earth is the ordinary case" },
    Band { name: "chernozem share of land", sweet: (0.01, 0.18), hard: (0.002, 0.30), target: "M51: black earth is rare and continental" },
    Band { name: "laterite share of land", sweet: (0.02, 0.30), hard: (0.003, 0.50), target: "M51: the hot wet tropics weather deep and poor" },
    Band { name: "andosol share of land", sweet: (0.01, 0.25), hard: (0.001, 0.40), target: "M51: ash country tracks the volcanic province" },
    Band { name: "gley share of land", sweet: (0.01, 0.25), hard: (0.001, 0.40), target: "M51: valley floors and cold flats pond" },
    Band { name: "aridisol share of land", sweet: (0.03, 0.35), hard: (0.005, 0.55), target: "M51: the dry share tracks the desert share" },
    Band { name: "soil fertility rank correlation", sweet: (0.30, 1.0), hard: (0.10, 1.0), target: "M51: the order's curve must order the farms" },
];
