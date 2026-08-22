//! Seeded 3D gradient noise + fBm / ridged — port of noisegen.py.

use rand::Rng;

pub struct Perlin3 {
    perm: [usize; 512],
}

impl Perlin3 {
    pub fn new(seed: i64) -> Self {
        let mut rng = crate::util::rng(seed);
        let mut p: [usize; 256] = [0; 256];
        for (i, v) in p.iter_mut().enumerate() {
            *v = i;
        }
        // Fisher–Yates with a fixed-width draw (u64 multiply-shift), never
        // Uniform<usize>: usize is 32-bit on wasm32 and 64-bit natively,
        // and the sample width changes how many PCG words a range draw
        // consumes — the permutation, and every noise field built on it,
        // would silently differ across runtimes (the M22 replay gate).
        for i in (1..256).rev() {
            let j = ((rng.gen::<u64>() as u128 * (i as u128 + 1)) >> 64) as usize;
            p.swap(i, j);
        }
        let mut perm = [0usize; 512];
        perm[..256].copy_from_slice(&p);
        perm[256..].copy_from_slice(&p);
        Perlin3 { perm }
    }

    #[inline]
    fn fade(t: f64) -> f64 {
        t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
    }

    #[inline]
    fn lerp(a: f64, b: f64, t: f64) -> f64 {
        a + t * (b - a)
    }

    #[inline]
    fn grad(h: usize, x: f64, y: f64, z: f64) -> f64 {
        let h = h & 15;
        let u = if h < 8 { x } else { y };
        let v = if h < 4 {
            y
        } else if h == 12 || h == 14 {
            x
        } else {
            z
        };
        (if h & 1 == 0 { u } else { -u }) + (if h & 2 == 0 { v } else { -v })
    }

    pub fn noise(&self, x: f64, y: f64, z: f64) -> f64 {
        let xf0 = x.floor();
        let yf0 = y.floor();
        let zf0 = z.floor();
        let xi = (xf0 as i64 & 255) as usize;
        let yi = (yf0 as i64 & 255) as usize;
        let zi = (zf0 as i64 & 255) as usize;
        let xf = x - xf0;
        let yf = y - yf0;
        let zf = z - zf0;
        let u = Self::fade(xf);
        let v = Self::fade(yf);
        let w = Self::fade(zf);
        let p = &self.perm;
        let a = p[xi] + yi;
        let aa = p[a] + zi;
        let ab = p[a + 1] + zi;
        let b = p[xi + 1] + yi;
        let ba = p[b] + zi;
        let bb = p[b + 1] + zi;
        let n000 = Self::grad(p[aa], xf, yf, zf);
        let n100 = Self::grad(p[ba], xf - 1.0, yf, zf);
        let n010 = Self::grad(p[ab], xf, yf - 1.0, zf);
        let n110 = Self::grad(p[bb], xf - 1.0, yf - 1.0, zf);
        let x00 = Self::lerp(n000, n100, u);
        let x10 = Self::lerp(n010, n110, u);
        let y0 = Self::lerp(x00, x10, v);
        // z on a lattice plane (every integer-z octave of a constant-z
        // field): lerp(y0, y1, 0.0) = y0 + 0.0·(y1−y0), so the upper-z
        // corners are dead weight — skip 4 grads and 3 lerps. The only
        // possible bit difference is the sign of an exact zero, and
        // every consumer absorbs it: fbm accumulates with += (+0.0 plus
        // ±0.0 is +0.0), ridged takes |n|, and the lone direct caller
        // (geo.rs arc noise, z=2.5) never lands here.
        if w == 0.0 {
            return y0;
        }
        let n001 = Self::grad(p[aa + 1], xf, yf, zf - 1.0);
        let n101 = Self::grad(p[ba + 1], xf - 1.0, yf, zf - 1.0);
        let n011 = Self::grad(p[ab + 1], xf, yf - 1.0, zf - 1.0);
        let n111 = Self::grad(p[bb + 1], xf - 1.0, yf - 1.0, zf - 1.0);
        let x01 = Self::lerp(n001, n101, u);
        let x11 = Self::lerp(n011, n111, u);
        let y1 = Self::lerp(x01, x11, v);
        Self::lerp(y0, y1, w)
    }

    pub fn fbm(&self, x: f64, y: f64, z: f64, octaves: u32) -> f64 {
        let (lacunarity, gain) = (2.0, 0.5);
        let mut total = 0.0;
        let mut amp = 1.0;
        let mut freq = 1.0;
        let mut norm = 0.0;
        for _ in 0..octaves {
            total += amp * self.noise(x * freq, y * freq, z * freq);
            norm += amp;
            amp *= gain;
            freq *= lacunarity;
        }
        total / norm
    }

    /// Ridged multifractal — 1-|noise| per octave; makes mountain ranges.
    pub fn ridged(&self, x: f64, y: f64, z: f64, octaves: u32) -> f64 {
        let (lacunarity, gain) = (2.0, 0.5);
        let mut total = 0.0;
        let mut amp = 1.0;
        let mut freq = 1.0;
        let mut norm = 0.0;
        for _ in 0..octaves {
            total += amp * (1.0 - self.noise(x * freq, y * freq, z * freq).abs());
            norm += amp;
            amp *= gain;
            freq *= lacunarity;
        }
        total / norm
    }
}
