//! Pack v2 — the binary world payload and the field registry (E11.1).
//!
//! Everything that turns grids into wire bytes lives here: the field
//! registry macro (E2.1), quantization (E3.4), and `World::pack()`
//! (E3.3–E3.6). Moved verbatim out of `world.rs`; the wire format and the
//! determinism hash are unchanged.

use ndarray::Array2;
use serde_json::{json, Value};

use crate::constants;
use crate::politics;
use crate::world::World;

impl World {
    /// Minimal pack-header meta (E3.1): identity, dimensions and physical
    /// constants only — everything entity-shaped rides `bootstrap()`.
    pub(crate) fn pack_meta(&self) -> Value {
        json!({
            "seed": self.seed,
            "size": self.size,
            "width": self.width,
            "height_cells": self.size,
            "month": self.month,
            "months": constants::MONTHS,
            "sea_level": 0.0,
            "metres_per_unit": constants::METRES_PER_UNIT,
            "km_per_cell": constants::KM_PER_CELL,
            "world_name": self.world_name,
        })
    }

    /// Pack v2 (E3.3–E3.6): `[u32 header_len][header json (padded to 4)][blob]`.
    /// The header carries `pack: 2`, a CRC-32 of the blob (E3.6), and the
    /// territory grid as RLE instead of a raw section (E3.5); float grids
    /// ride as quantized u16 where the registry says so (E3.4). The blob is
    /// written once, straight from grid storage — no per-field temporary
    /// buffers (E3.3). Section order comes from the field registry (E2.2).
    pub fn pack(&self) -> Vec<u8> {
        let fields = self.field_decls();
        let cells = self.size * self.width;
        let mut blob: Vec<u8> = Vec::with_capacity(cells * 20 + 64);
        let mut entries: Vec<Value> = Vec::new();
        for f in &fields {
            // territory rides the header as RLE (E3.5): contiguous realms
            // compress ~1000×, and the client already speaks this encoding
            // for tick patches.
            if f.name == "territory" {
                continue;
            }
            let offset = blob.len();
            let mut entry = json!({
                "name": f.name,
                "dtype": f.data.dtype(),
                "shape": [self.size, self.width],
            });
            match (&f.data, f.quant) {
                (FieldData::F32(a), Quant::Linear) => {
                    let s = a.as_slice().expect("registry grids are contiguous");
                    let (lo, hi) = min_max(s);
                    let (scale, inv) = quant_steps(lo, hi);
                    blob.reserve(s.len() * 2);
                    for &v in s {
                        let q = ((v as f64 - lo) * inv).round().clamp(0.0, 65535.0) as u16;
                        blob.extend_from_slice(&q.to_le_bytes());
                    }
                    entry["dtype"] = json!("uint16");
                    entry["q"] = json!({ "scale": scale, "offset": lo, "xform": "linear" });
                }
                (FieldData::F32(a), Quant::Sqrt) => {
                    // 16 bits spent in sqrt-space: low flows keep relative
                    // precision even though discharge spans ~6 decades.
                    let s = a.as_slice().expect("registry grids are contiguous");
                    let mut lo = f64::INFINITY;
                    let mut hi = f64::NEG_INFINITY;
                    for &v in s {
                        let t = (v.max(0.0) as f64).sqrt();
                        if t < lo { lo = t; }
                        if t > hi { hi = t; }
                    }
                    if !lo.is_finite() {
                        lo = 0.0;
                        hi = 0.0;
                    }
                    let (scale, inv) = quant_steps(lo, hi);
                    blob.reserve(s.len() * 2);
                    for &v in s {
                        let t = (v.max(0.0) as f64).sqrt();
                        let q = ((t - lo) * inv).round().clamp(0.0, 65535.0) as u16;
                        blob.extend_from_slice(&q.to_le_bytes());
                    }
                    entry["dtype"] = json!("uint16");
                    entry["q"] = json!({ "scale": scale, "offset": lo, "xform": "sqrt" });
                }
                (FieldData::F32(a), Quant::Linear8) => {
                    // E3.4 — 8-bit lane: normalized, overlay-only fields
                    // (shares in −1..1, indices in 0..1, metre depths read
                    // at map scale) do not earn 16 bits on the wire. Storage
                    // and the determinism hash still see full f32.
                    let s = a.as_slice().expect("registry grids are contiguous");
                    let (lo, hi) = min_max(s);
                    let (scale, inv) = quant_steps8(lo, hi);
                    blob.reserve(s.len());
                    for &v in s {
                        let q = ((v as f64 - lo) * inv).round().clamp(0.0, 255.0) as u8;
                        blob.push(q);
                    }
                    entry["dtype"] = json!("uint8");
                    entry["q"] = json!({ "scale": scale, "offset": lo, "xform": "linear" });
                }
                (data, _) => data.write_into(&mut blob),
            }
            entry["offset"] = json!(offset);
            entry["nbytes"] = json!(blob.len() - offset);
            entries.push(entry);
        }

        let mut header = self.pack_meta();
        header["id"] = json!(format!("{}-{}", self.seed, self.size));
        header["pack"] = json!(PACK_VERSION);
        header["crc32"] = json!(crate::util::crc32(&blob));
        header["territory"] = json!(politics::territory_rle(&self.fields.territory));
        header["arrays"] = Value::Array(entries);
        let mut hjson = serde_json::to_string(&header).unwrap().into_bytes();
        while hjson.len() % 4 != 0 {
            hjson.push(b' ');
        }

        let mut out = Vec::with_capacity(4 + hjson.len() + blob.len());
        out.extend_from_slice(&(hjson.len() as u32).to_le_bytes());
        out.extend_from_slice(&hjson);
        out.extend_from_slice(&blob);
        out
    }
}

/// Pack protocol version — the client refuses any other (E3.6).
pub const PACK_VERSION: u32 = 2;

/// M15.7 — hostile-proof pack reader: everything the client's unpacker
/// trusts, re-checked here with bounds instead of faith. Returns
/// `(array count, blob bytes)` for a well-formed buffer; any truncation,
/// corruption, lying header or overflowing size is an `Err`, never a
/// panic. The assay's fuzz lane hammers this with mutated real packs.
pub fn validate_pack(bytes: &[u8]) -> Result<(usize, usize), String> {
    if bytes.len() < 4 {
        return Err("short buffer: no header length".into());
    }
    let hlen = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let base = 4usize
        .checked_add(hlen)
        .ok_or_else(|| "header length overflows".to_string())?;
    if base > bytes.len() {
        return Err(format!("header runs past buffer: {} > {}", base, bytes.len()));
    }
    let header: Value = serde_json::from_slice(&bytes[4..base])
        .map_err(|e| format!("header is not JSON: {e}"))?;
    if header.get("pack").and_then(Value::as_u64) != Some(PACK_VERSION as u64) {
        return Err("wrong or missing pack version".into());
    }
    let blob = &bytes[base..];
    let crc = header
        .get("crc32")
        .and_then(Value::as_u64)
        .ok_or_else(|| "missing crc32".to_string())?;
    if crate::util::crc32(blob) as u64 != crc {
        return Err("crc32 mismatch: blob corrupt or truncated".into());
    }
    let arrays = header
        .get("arrays")
        .and_then(Value::as_array)
        .ok_or_else(|| "missing arrays table".to_string())?;
    let mut expected_off = 0usize;
    for e in arrays {
        let off = e
            .get("offset")
            .and_then(Value::as_u64)
            .ok_or_else(|| "array entry missing offset".to_string())? as usize;
        let nb = e
            .get("nbytes")
            .and_then(Value::as_u64)
            .ok_or_else(|| "array entry missing nbytes".to_string())? as usize;
        let shape = e
            .get("shape")
            .and_then(Value::as_array)
            .ok_or_else(|| "array entry missing shape".to_string())?;
        if shape.len() != 2 {
            return Err("shape is not 2-D".into());
        }
        let mut cells = 1usize;
        for d in shape {
            let d = d
                .as_u64()
                .ok_or_else(|| "shape dim is not an integer".to_string())?
                as usize;
            cells = cells
                .checked_mul(d)
                .ok_or_else(|| "shape overflows".to_string())?;
        }
        let cell = match e.get("dtype").and_then(Value::as_str) {
            Some("float32") => 4,
            Some("uint16") | Some("int16") => 2,
            Some("uint8") => 1,
            other => return Err(format!("unknown dtype {other:?}")),
        };
        let want = cells
            .checked_mul(cell)
            .ok_or_else(|| "nbytes overflows".to_string())?;
        if nb != want {
            return Err(format!("nbytes {} disagrees with shape ({} expected)", nb, want));
        }
        if off != expected_off {
            return Err(format!("offset {} breaks contiguity (expected {})", off, expected_off));
        }
        expected_off = off
            .checked_add(nb)
            .ok_or_else(|| "offsets overflow".to_string())?;
    }
    if expected_off != blob.len() {
        return Err(format!("blob is {} B but arrays claim {}", blob.len(), expected_off));
    }
    Ok((arrays.len(), blob.len()))
}

fn min_max(s: &[f32]) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for &v in s {
        let v = v as f64;
        if v < lo { lo = v; }
        if v > hi { hi = v; }
    }
    if lo.is_finite() { (lo, hi) } else { (0.0, 0.0) }
}

/// `(scale, 1/scale)` for a u16 span over `[lo, hi]`; constant fields get 0.
fn quant_steps8(lo: f64, hi: f64) -> (f64, f64) {
    if hi > lo {
        let scale = (hi - lo) / 255.0;
        (scale, 1.0 / scale)
    } else {
        (0.0, 0.0)
    }
}

fn quant_steps(lo: f64, hi: f64) -> (f64, f64) {
    if hi > lo {
        let scale = (hi - lo) / 65535.0;
        (scale, 1.0 / scale)
    } else {
        (0.0, 0.0)
    }
}

/// One grid's declaration in the field registry (E2.1).
pub struct FieldDecl<'a> {
    /// Wire + registry name; also the JS-side array key.
    pub name: &'static str,
    /// Human units, for docs and generated constants.
    pub units: &'static str,
    /// Included in the diagnostics state hash (mutable-by-tick grids yes;
    /// grids derivable from them or static after generation, no).
    pub in_hash: bool,
    /// True when Orbital uploads this grid as a texture (E2.2: the upload
    /// list on the JS side derives from the generated constants).
    pub gpu: bool,
    /// Wire quantization (E3.4) — storage and the determinism hash always
    /// see full f32; quantization is strictly a wire concern.
    pub quant: Quant,
    pub data: FieldData<'a>,
}

/// Data-free registry row — what codegen and offline tooling see (E2.4).
pub struct FieldSpec {
    pub name: &'static str,
    pub dtype: &'static str,
    pub units: &'static str,
    pub in_hash: bool,
    pub gpu: bool,
}

/// E2.1 — the field registry macro: every per-cell grid the world owns,
/// declared exactly once with name, storage kind, units, hash inclusion and
/// GPU upload flag. Expands to the static `FIELD_SPECS` table (codegen) and
/// `World::fields()` (pack + hash). A grid added here is a grid added
/// everywhere; field-order drift dies structurally (E2.2).
///
/// Order is the pack order and is a versioned contract (ADR-0007).
macro_rules! dtype_name {
    (F32) => { "float32" };
    (U8) => { "uint8" };
    (I16) => { "int16" };
}

// The `wire` column: how the grid crosses WASM→JS (E3.4). `raw` ships
// storage bytes verbatim; `u16` is linear 16-bit quantization over the
// field's live range; `u16sqrt` quantizes in sqrt-space (wide-dynamic-range
// fields keep relative precision at the low end). The client dequantizes
// back to float32 at the unpack edge, so everything downstream is unchanged.
macro_rules! quant_mode {
    (raw) => { Quant::None };
    (u16) => { Quant::Linear };
    (u16sqrt) => { Quant::Sqrt };
    (u8) => { Quant::Linear8 };
}

macro_rules! field_registry {
    ($($field:ident : $kind:ident, units $units:literal, hash $h:literal, gpu $g:literal, wire $wire:ident;)+) => {
        /// Static view of the field registry, in pack order (E2.1/E2.4).
        /// `dtype` is the *decoded* type the client ends up holding.
        pub const FIELD_SPECS: &[FieldSpec] = &[$(
            FieldSpec {
                name: stringify!($field),
                dtype: dtype_name!($kind),
                units: $units,
                in_hash: $h,
                gpu: $g,
            },
        )+];

        impl World {
            /// The live registry: specs bound to this world's grids.
            pub fn field_decls(&self) -> Vec<FieldDecl<'_>> {
                vec![$(
                    FieldDecl {
                        name: stringify!($field),
                        units: $units,
                        in_hash: $h,
                        gpu: $g,
                        quant: quant_mode!($wire),
                        data: FieldData::$kind(&self.fields.$field),
                    },
                )+]
            }
        }
    };
}

field_registry! {
    height:    F32, units "rel. elevation (0 = sea)",        hash true,  gpu true,  wire u16;
    tmean:     F32, units "°C annual mean",                  hash false, gpu true,  wire u16;
    tamp:      F32, units "°C seasonal amplitude",           hash false, gpu true,  wire u16;
    precip:    F32, units "mm/yr",                           hash false, gpu true,  wire u16;
    pamp:      F32, units "signed monsoon share −1..1",      hash true,  gpu false, wire u8;
    discharge: F32, units "flow accumulation (cells·rain)",  hash false, gpu true,  wire u16sqrt;
    flow_amp:  F32, units "signed seasonal swing −1..1",     hash true,  gpu false, wire u8;
    fertility: F32, units "0..1 arable index",               hash false, gpu true,  wire u16;
    biomes:    U8,  units "biome id",                        hash true,  gpu false, wire raw;
    crops:     U8,  units "crop package id",                 hash true,  gpu false, wire raw;
    strahler:  U8,  units "stream order, 0 off-river",       hash true,  gpu true,  wire raw;
    flags:     U8,  units "CellFlags bits",                  hash true,  gpu true,  wire raw;
    territory: I16, units "owner realm, −1 wild",            hash false, gpu false, wire raw;
    rock:      U8,  units "rock province id (M18)",          hash true,  gpu true,  wire raw;
    soil:      U8,  units "soil order id (M51)",              hash true,  gpu true,  wire raw;
    upwelling: F32, units "0..1 coastal upwelling (M47)",    hash true,  gpu false, wire u8;
    aquifer:   F32, units "m depth to water table (M54)",    hash true,  gpu false, wire u8;
    landform:  U8,  units "landform vocabulary id (M60)",    hash true,  gpu true,  wire raw;
    // M68 — the era's last two hand-wired grids come home. Both are
    // already hashed by their ledgers (`Coast::hash`, `Sediment::hash`,
    // which read these very arrays), so the registry declares them
    // `hash false`: one grid, one hash, no double counting and no
    // churn in the replay identity the era sealed against.
    coastform: U8,  units "coast form id (M44)",             hash false, gpu false, wire raw;
    silt:      F32, units "deposition depth, height units",  hash false, gpu false, wire u16sqrt;
}

/// Wire quantization mode for a registry field (E3.4).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Quant {
    /// Storage bytes ride verbatim.
    None,
    /// Linear u16 over the field's live `[min, max]` span.
    Linear,
    /// Linear u16 in sqrt-space — for wide-dynamic-range fields.
    Sqrt,
    /// Linear u8 over the field's live `[min, max]` span — normalized,
    /// overlay-only grids (E3.4).
    Linear8,
}

/// Borrowed grid storage behind a registry entry. Storage is f32 at rest
/// (E3.2); the wire may narrow further via `Quant` (E3.4).
pub enum FieldData<'a> {
    F32(&'a Array2<f32>),
    U8(&'a Array2<u8>),
    I16(&'a Array2<i16>),
}

impl FieldData<'_> {
    /// Decoded dtype name as the JS client ends up holding it.
    pub fn dtype(&self) -> &'static str {
        match self {
            FieldData::F32(_) => "float32",
            FieldData::U8(_) => "uint8",
            FieldData::I16(_) => "int16",
        }
    }

    /// Append raw little-endian bytes straight from grid storage — the
    /// no-temporaries path of pack v2 (E3.3).
    pub fn write_into(&self, out: &mut Vec<u8>) {
        match self {
            FieldData::F32(a) => out.extend_from_slice(bytemuck::cast_slice(
                a.as_slice().expect("registry grids are contiguous"),
            )),
            FieldData::U8(a) => {
                out.extend_from_slice(a.as_slice().expect("registry grids are contiguous"))
            }
            FieldData::I16(a) => out.extend_from_slice(bytemuck::cast_slice(
                a.as_slice().expect("registry grids are contiguous"),
            )),
        }
    }

    /// Exact-width storage bytes for the determinism hash — the hash sees
    /// every bit the simulation sees (f32 at rest since E3.2).
    pub fn hash_bytes(&self, out: &mut Vec<u8>) {
        match self {
            FieldData::F32(a) => {
                for &v in a.iter() {
                    out.extend_from_slice(&v.to_bits().to_le_bytes());
                }
            }
            FieldData::U8(a) => out.extend(a.iter().cloned()),
            FieldData::I16(a) => {
                for &v in a.iter() {
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
    }
}
