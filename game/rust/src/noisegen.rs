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
        // Fisher–Yates
        for i in (1..256).rev() {
            let j = rng.gen_range(0..=i);
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
        let n001 = Self::grad(p[aa + 1], xf, yf, zf - 1.0);
        let n101 = Self::grad(p[ba + 1], xf - 1.0, yf, zf - 1.0);
        let n011 = Self::grad(p[ab + 1], xf, yf - 1.0, zf - 1.0);
        let n111 = Self::grad(p[bb + 1], xf - 1.0, yf - 1.0, zf - 1.0);
        let x00 = Self::lerp(n000, n100, u);
        let x10 = Self::lerp(n010, n110, u);
        let x01 = Self::lerp(n001, n101, u);
        let x11 = Self::lerp(n011, n111, u);
        let y0 = Self::lerp(x00, x10, v);
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
