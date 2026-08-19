//! Sea ice (M37) — winter closes the sea the way it closes the land.
//!
//! A monthly sea-surface temperature proxy (latitude and continentality
//! arrive already folded into `tmean`/`tamp`; the mixed layer damps the
//! swing) freezes any water that drops to −1.8 °C. The result is a
//! 12-bit month mask per sea cell: perennial pack around the poles, a
//! seasonal fringe that shuts in winter and breaks up in spring, open
//! water everywhere else. The trade grid prices the pack off the sea
//! lanes and every route remembers the months its water is shut
//! (`Route::closed`); the economy moves no cargo through an icebound
//! month. All of it runs through the exact `COS12` table, so the masks
//! are a pure function of the climate grids on every runtime
//! (ADR-0025 discipline).

use ndarray::Array2;

use crate::climate::COS12;

/// Seawater freezes at −1.8 °C — salt at ocean strength depresses the
/// freezing point well below fresh water's zero.
pub const SEA_FREEZE_C: f64 = -1.8;

/// The ocean's mixed layer damps the air's seasonal swing: SST swings
/// at about three quarters of the (already maritime) air amplitude.
pub const SST_DAMP: f64 = 0.75;

/// All twelve month bits.
pub const MONTHS_MASK: u16 = 0x0FFF;

/// Perennial pack is no lane at all: dearer per cell than the worst
/// mountain mile, so A* threads around it or stays ashore.
pub const PACK_SEA_COST: f64 = 45.0;

/// A lane that freezes part of the year charges its closed season up
/// front: +150% at eleven months iced, pro rata below — the annualized
/// price of a strait that only sometimes answers.
pub const ICE_LANE_SURCHARGE: f64 = 1.5;

/// The freeze mask for one cell's SST cycle: bit m set = the surface
/// is at or below the freezing point in calendar month m.
pub fn cell_mask(tmean: f64, tamp_signed: f64) -> u16 {
    let a = tamp_signed * SST_DAMP;
    let mut m = 0u16;
    for (i, c) in COS12.iter().enumerate() {
        if tmean + a * c <= SEA_FREEZE_C {
            m |= 1 << i;
        }
    }
    m
}

/// Per-cell month mask of pack-ice cover: 0 on land and open water,
/// `MONTHS_MASK` where the pack never breaks.
pub fn frozen_months(
    height: &Array2<f32>,
    tmean: &Array2<f32>,
    tamp: &Array2<f32>,
) -> Array2<u16> {
    Array2::from_shape_fn(height.dim(), |(y, x)| {
        if height[[y, x]] >= 0.0 {
            0
        } else {
            cell_mask(tmean[[y, x]] as f64, tamp[[y, x]] as f64)
        }
    })
}

/// A well-formed seasonal closure: one contiguous arc of months around
/// the ring, containing the hemisphere's midwinter (month 0 north,
/// month 6 south — `temperature_amplitude`'s sign convention) and free
/// of its midsummer. Empty and perennial masks are not arcs.
pub fn is_winter_arc(mask: u16, southern: bool) -> bool {
    let m = mask & MONTHS_MASK;
    if m == 0 || m == MONTHS_MASK {
        return false;
    }
    let mut trans = 0u32;
    for i in 0..12u32 {
        let a = (m >> i) & 1;
        let b = (m >> ((i + 1) % 12)) & 1;
        if a != b {
            trans += 1;
        }
    }
    if trans != 2 {
        return false;
    }
    let (midwinter, midsummer) = if southern { (6u16, 0u16) } else { (0u16, 6u16) };
    m & (1 << midwinter) != 0 && m & (1 << midsummer) == 0
}

/// Diagnostics bands (E2.7). Extent is judged cos(lat)-weighted so the
/// equirectangular grid's fat polar rows don't inflate the pack; Earth's
/// annual-maximum ice covers ~4–8% of the ocean by area.
pub const BANDS: &[crate::util::Band] = &[
    crate::util::Band {
        name: "ever-frozen share of ocean area",
        sweet: (0.03, 0.25),
        hard: (0.01, 0.40),
        target: "M37 gate: pack ice real, but polar (Earth ~4–8%)",
    },
    crate::util::Band {
        name: "seasonal share of the pack",
        sweet: (0.10, 0.90),
        hard: (0.03, 1.0),
        target: "M37 gate: a fringe that opens in summer",
    },
];
