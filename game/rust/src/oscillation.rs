//! M74 — The Seesaw Seas.
//!
//! Some of the sky's year-to-year swing is not white: a warm pool leans
//! east, the trades slacken, and for two to seven years the whole basin
//! sits on one side of its mean before leaning back. This module holds
//! that slow lean — an ENSO-class index — as a *law*, not a table: a
//! period, an amplitude and a phase drawn once from the seed, plus a
//! coloured noise term so the seesaw is never a metronome.
//!
//! The index is a pure function of `(seed, month)`. Nothing here is
//! stored in the world's grids, in keeping with ADR-0003: the year's sky
//! is derived. The identity line therefore carries a probe of the source
//! (see `Oscillation::probe`), exactly as M73 does for the variability
//! lattice.
//!
//! Conventions: positive index = the warm phase (the pool leaned east).
//! The index is dimensionless and normalised so that its realized σ over
//! long spans is the drawn amplitude.

use rand::Rng;
use rand::SeedableRng;
use rand_pcg::Pcg64Mcg;

/// The shortest lean the seas will hold, in months (two years).
pub const OSC_PERIOD_MIN: f64 = 24.0;
/// The longest, in months (seven years).
pub const OSC_PERIOD_MAX: f64 = 84.0;
/// The weakest basin a world may draw (index σ, dimensionless).
pub const OSC_AMP_MIN: f64 = 0.55;
/// The strongest.
pub const OSC_AMP_MAX: f64 = 1.45;
/// Share of the index's variance carried by the coloured noise term
/// rather than the clean sinusoid — the seesaw is irregular, and real
/// basins skip and stall rather than ticking.
pub const OSC_NOISE_SHARE: f64 = 0.30;
/// How slowly the noise term itself wanders, in lattice units per month.
pub const OSC_NOISE_STEP: f64 = 0.055;
/// σ of the fbm lane used for the noise term, so it can be normalised to
/// unit variance before it is mixed in. Measured, not guessed: 200 000
/// consecutive months of `fbm(m·OSC_NOISE_STEP, 0.5, 0.5, 2)` give
/// σ = 0.1897 / 0.1949 / 0.1903 on three lattices (mean ≈ 0.1916). A
/// wrong value here shows up directly as realized σ missing the drawn
/// amplitude, which is what the M74 lane measures.
pub const OSC_FBM_SIGMA: f64 = 0.1916;
/// No basin leans further than this many σ — a hard physical stop, so a
/// single tail draw can never hand the causal path an absurd year.
pub const OSC_CAP_SIGMA: f64 = 3.0;

/// The slow lean of the seas for one world.

pub struct Oscillation {
    period: f64,
    amp: f64,
    phase0: f64,
    noise: crate::noisegen::Perlin3,
}

impl Oscillation {
    /// Draw a basin from the seed. Same seed ⇒ same seesaw, forever.
    pub fn new(seed: i64) -> Self {
        let mut rng = Pcg64Mcg::seed_from_u64((seed as u64) ^ 0x05C1_11A7_0E5E_A5u64);
        let period = rng.gen_range(OSC_PERIOD_MIN..=OSC_PERIOD_MAX);
        let amp = rng.gen_range(OSC_AMP_MIN..=OSC_AMP_MAX);
        let phase0 = rng.gen_range(0.0..std::f64::consts::TAU);
        Self {
            period,
            amp,
            phase0,
            noise: crate::noisegen::Perlin3::new(seed + 9311),
        }
    }

    /// The drawn period, in months.
    pub fn period(&self) -> f64 {
        self.period
    }

    /// The drawn amplitude — the index's σ, dimensionless.
    pub fn amp(&self) -> f64 {
        self.amp
    }

    /// The drawn starting phase, in radians.
    pub fn phase0(&self) -> f64 {
        self.phase0
    }

    /// The index at a given month: positive is the warm phase.
    ///
    /// The clean lean and the coloured noise are mixed at fixed variance
    /// shares, so the realized σ is the drawn amplitude no matter how
    /// the noise share is tuned; the sum is then capped at
    /// `OSC_CAP_SIGMA` amplitudes.
    pub fn index(&self, month: i64) -> f64 {
        let m = month as f64;
        // A sinusoid of unit amplitude has variance 1/2, so √2 makes the
        // clean lane unit-variance before the shares are applied.
        let clean = std::f64::consts::SQRT_2 * (std::f64::consts::TAU * m / self.period + self.phase0).sin();
        let rough = self.noise.fbm(m * OSC_NOISE_STEP, 0.5, 0.5, 2) / OSC_FBM_SIGMA;
        let w_rough = OSC_NOISE_SHARE.sqrt();
        let w_clean = (1.0 - OSC_NOISE_SHARE).sqrt();
        let v = self.amp * (w_clean * clean + w_rough * rough);
        v.clamp(-OSC_CAP_SIGMA * self.amp, OSC_CAP_SIGMA * self.amp)
    }

    /// A fixed, world-independent read of the basin, for the replay
    /// identity line: the drawn law plus the index at spaced months.
    /// A basin whose constants or keying drift breaks replay here.
    pub fn probe(&self) -> u64 {
        let mut b: Vec<u8> = Vec::with_capacity(8 * 16);
        for v in [self.period, self.amp, self.phase0] {
            b.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        for month in [0i64, 7, 41, 199, 1201, 3600] {
            b.extend_from_slice(&self.index(month).to_bits().to_le_bytes());
        }
        crate::util::fnv1a64(&b)
    }
}
