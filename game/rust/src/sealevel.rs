//! M25 — sea-level history: the waterline remembers the ice ages.
//!
//! Every world freezes at one moment of a glacial cycle, drawn from the
//! seed. Two signals reshape the coast at world-genesis, both applied as
//! height offsets before erosion fixes the waterline (`h < 0` is ocean,
//! so raising land is exactly lowering the ocean threshold):
//!
//! - **Eustatic stand** — where the cycle sits between interglacial
//!   highstand (+) and full-glacial lowstand (−). The cycle is the
//!   classic sawtooth: ice builds slowly over nine-tenths of the cycle
//!   (sea falls), then deglaciation returns the water in the last tenth
//!   (sea rises fast). A lowstand world walks on its own shelf; a
//!   highstand world drowns its river mouths.
//! - **Post-glacial isostasy** — where the sheets sat at the last
//!   lowstand (high latitudes), the land is still rising after the
//!   unload; the residual decays with time since deglaciation. Around
//!   the former ice margin the collapsing forebulge sinks a collar of
//!   mid-latitude coast instead. This is the raised-beach /
//!   drowned-coast dial M26 reads.
//!
//! The struct is frozen prehistory in the ADR-0024 sense: generated
//! once, consumed by generation, hashed into the determinism identity,
//! never advanced in tick time. Amplitudes are calibrated so the most
//! extreme stand moves the planet's land fraction by only a few percent
//! relative — coasts change character, continents do not vanish (the
//! M25 gate holds the datum within bands in `diagnose terrain`).

use ndarray::Array2;

use crate::util::fnv1a64;

/// Eustatic swing, height units: full lowstand exposes this much shelf.
/// The land-height scale puts the shelf lip near −0.03..0; 0.018 turns
/// a fat shelf into plains without beaching the abyss.
const EUSTATIC_AMP: f64 = 0.018;

/// Peak residual rebound uplift at the former ice-load latitudes, fresh
/// off a deglaciation. Decays with time since the melt.
const REBOUND_AMP: f64 = 0.030;

/// Forebulge collapse: the mid-latitude collar sinks at ~a third of the
/// rebound rate, with the opposite sign.
const FOREBULGE_FRAC: f64 = -0.35;

/// The sheets' equatorward edge at the last lowstand, degrees |lat|.
/// Uplift ramps in across `ICE_EDGE..ICE_FULL`; the forebulge collar
/// sits just south of it.
const ICE_EDGE: f64 = 46.0;
const ICE_FULL: f64 = 62.0;
const COLLAR_LO: f64 = 33.0;

/// Fraction of the cycle spent building ice (sea falling). The melt
/// takes the remainder — the sawtooth's short edge.
const BUILD: f64 = 0.9;

#[derive(Clone, Debug)]
pub struct SeaLevel {
    /// Where in the glacial cycle the world froze, [0,1). 0 = just
    /// deglaciated; BUILD⁻ = full glacial; (BUILD,1) = mid-melt.
    pub phase: f64,
    /// Normalized stand: +0.10 at interglacial highstand, −1.0 at full
    /// lowstand. Sign matches the sea surface, not the ice.
    pub stand: f64,
    /// Sea-surface offset in height units (`stand × EUSTATIC_AMP`).
    /// Negative = lowstand. Applied as `h −= eustatic`.
    pub eustatic: f64,
    /// Peak isostatic uplift actually applied this freeze (height units).
    pub rebound: f64,
    /// Peak forebulge subsidence actually applied (≤ 0, height units).
    pub forebulge: f64,
    /// Per-row isostatic offset (uplift + forebulge only — the eustatic
    /// term is uniform and kept separate so M26 can tell the two
    /// histories apart). Length = generated rows.
    pub row: Vec<f64>,
}

/// SplitMix64 — one fixed-width draw, identical on every runtime
/// (the same discipline as the noisegen permutation fix, ADR-0025).
fn splitmix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn smoothstep(x: f64, e0: f64, e1: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn generate(seed: i64, size: usize) -> SeaLevel {
    // Two rounds: one is a weak avalanche for small human seeds (the
    // report seeds all landed early-cycle); two spreads them flat.
    let phase = (splitmix64(splitmix64((seed as u64) ^ 0x5EA1_EE1D_C0DE_CAFE)) >> 11) as f64
        / (1u64 << 53) as f64;

    // Ice volume along the sawtooth: slow build, fast melt.
    let ice = if phase < BUILD {
        phase / BUILD
    } else {
        (1.0 - phase) / (1.0 - BUILD)
    };
    let stand = 0.10 - 1.10 * ice;
    let eustatic = EUSTATIC_AMP * stand;

    // Residual rebound: strongest just after the melt completes (phase
    // wraps to 0), fully relaxed by the time the next build-up is old;
    // during the melt window it ramps in with the unload itself. Cubic
    // decay, not exp(): the whole freeze is IEEE-exact arithmetic so
    // the prehistory replays byte-identically on every runtime
    // (ADR-0025 discipline, same as the quake clock).
    let recency = if phase < BUILD {
        let t = 1.0 - phase / BUILD;
        t * t * t
    } else {
        (phase - BUILD) / (1.0 - BUILD)
    };
    let rebound = REBOUND_AMP * recency;
    let forebulge = FOREBULGE_FRAC * rebound;

    // Latitude profile, rows mapped exactly as climate::latitude_deg.
    let n = size as f64;
    let row: Vec<f64> = (0..size)
        .map(|y| {
            let lat = (-90.0 + (y as f64) * 180.0 / (n - 1.0)).abs();
            let ice_w = smoothstep(lat, ICE_EDGE, ICE_FULL);
            // the collar rises to full just below the ice edge and
            // hands over to the uplift zone across the edge itself
            let collar_w = smoothstep(lat, COLLAR_LO, ICE_EDGE) * (1.0 - ice_w);
            rebound * ice_w + forebulge * collar_w
        })
        .collect();

    SeaLevel { phase, stand, eustatic, rebound, forebulge, row }
}

impl SeaLevel {
    /// Apply the freeze-time offsets to the raw heightmap: isostasy by
    /// row, the eustatic stand everywhere (a sea-surface fall is a land
    /// rise against the fixed `h < 0` threshold).
    pub fn apply(&self, h: &mut Array2<f64>) {
        let rows = h.nrows();
        for y in 0..rows {
            let dz = self.row[y.min(self.row.len() - 1)] - self.eustatic;
            if dz == 0.0 {
                continue;
            }
            for v in h.row_mut(y).iter_mut() {
                *v = (*v + dz).clamp(-1.0, 1.0);
            }
        }
    }

    /// FNV-1a over the whole history — scalars and the row profile.
    /// Joins `hash_state` (the M25 gate): two generations of one seed
    /// must carry one waterline.
    pub fn hash(&self) -> u64 {
        let mut b: Vec<u8> = Vec::with_capacity(self.row.len() * 8 + 48);
        for v in [self.phase, self.stand, self.eustatic, self.rebound, self.forebulge] {
            b.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        for v in &self.row {
            b.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        fnv1a64(&b)
    }
}
