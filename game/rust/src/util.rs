//! Shared helpers: clocks, seeded RNG, numpy-style scalar utilities.

use rand::SeedableRng;
use rand_pcg::Pcg64Mcg;

/// Milliseconds since epoch — native and wasm.
#[cfg(target_arch = "wasm32")]
pub fn now_ms() -> f64 {
    js_sys::Date::now()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn now_ms() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
        * 1000.0
}

/// Deterministic RNG from a (possibly offset) world seed.
pub fn rng(seed: i64) -> Pcg64Mcg {
    Pcg64Mcg::seed_from_u64(seed as u64)
}

/// np.quantile with linear interpolation (numpy's default).
pub fn quantile(vals: &[f64], q: f64) -> f64 {
    let mut v: Vec<f64> = vals.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if v.is_empty() {
        return 0.0;
    }
    let h = (v.len() - 1) as f64 * q;
    let lo = h.floor() as usize;
    let hi = h.ceil() as usize;
    let frac = h - lo as f64;
    v[lo] + (v[hi] - v[lo]) * frac
}

/// np.interp: piecewise-linear, clamped at the ends.
pub fn interp(x: f64, xs: &[f64], ys: &[f64]) -> f64 {
    if x <= xs[0] {
        return ys[0];
    }
    if x >= xs[xs.len() - 1] {
        return ys[ys.len() - 1];
    }
    for i in 1..xs.len() {
        if x <= xs[i] {
            let t = (x - xs[i - 1]) / (xs[i] - xs[i - 1]);
            return ys[i - 1] + t * (ys[i] - ys[i - 1]);
        }
    }
    ys[ys.len() - 1]
}

pub fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

pub fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

// ---------------------------------------------------------------- bands
//
// E2.7 — diagnostics bands. Each measured quantity's tuning range is
// declared once, beside the system that produces it (`BANDS` in geo,
// climate, biomes, agriculture, hydrology, resources, chronicle, economy,
// settlements, world), and the `diagnose` harness consumes them by name.
// PASS inside `sweet`, WARN inside `hard`, FAIL outside.

pub struct Band {
    pub name: &'static str,
    pub sweet: (f64, f64),
    pub hard: (f64, f64),
    pub target: &'static str,
}

/// Every band in the engine, in system order.
pub fn all_bands() -> impl Iterator<Item = &'static Band> {
    crate::geo::BANDS
        .iter()
        .chain(crate::climate::BANDS)
        .chain(crate::biomes::BANDS)
        .chain(crate::agriculture::BANDS)
        .chain(crate::hydrology::BANDS)
        .chain(crate::resources::BANDS)
        .chain(crate::chronicle::BANDS)
        .chain(crate::economy::BANDS)
        .chain(crate::settlements::BANDS)
        .chain(crate::world::BANDS)
}

/// Look one band up by name; an unknown name is a programmer error.
pub fn band(name: &str) -> &'static Band {
    all_bands()
        .find(|b| b.name == name)
        .unwrap_or_else(|| panic!("no diagnostics band named {name:?}"))
}

// ------------------------------------------------------------------ crc32

/// CRC-32 (IEEE 802.3, reflected) — the pack v2 integrity stamp (E3.6).
/// Mirrored bit-for-bit by `crc32()` in `game/web/js/net.js`; a stale or
/// truncated payload fails loudly at the unpack edge instead of rendering
/// garbage.
pub fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *slot = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

/// FNV-1a 64 — tiny, deterministic, allocation-free; gates wire sections
/// (E4.2/E4.3): a section reships only when its serialized bytes moved.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ------------------------------------------------------- wire precision

/// Serialize an `f64` at one decimal of wire precision (E4.2): display
/// resolution for the food/wealth heartbeats — shorter payloads, and the
/// delta hashes stay put while sub-display noise drifts underneath.
pub fn ser_f1<S: serde::Serializer>(v: &f64, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_f64((v * 10.0).round() / 10.0)
}

/// Serialize an `f64` as whole units (E4.2): carrying capacity is souls;
/// fractional souls are noise on the wire.
pub fn ser_round_i64<S: serde::Serializer>(v: &f64, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_i64(v.round() as i64)
}
