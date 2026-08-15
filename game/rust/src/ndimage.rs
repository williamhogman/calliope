//! scipy.ndimage equivalents: gaussian filter, EDT, dilation, labeling,
//! maximum filter, gradients — everything the simulation leaned on.

use ndarray::Array2;

/// scipy 'reflect' boundary: (d c b a | a b c d | d c b a)
#[inline]
pub fn reflect(mut i: isize, n: isize) -> usize {
    while i < 0 || i >= n {
        if i < 0 {
            i = -i - 1;
        }
        if i >= n {
            i = 2 * n - i - 1;
        }
    }
    i as usize
}

/// Separable gaussian blur, truncate=4.0, mode='reflect' (scipy defaults).
pub fn gaussian_filter(a: &Array2<f64>, sigma: f64) -> Array2<f64> {
    let r = (4.0 * sigma + 0.5) as isize;
    let s2 = 2.0 * sigma * sigma;
    let mut k: Vec<f64> = (-r..=r).map(|d| (-(d * d) as f64 / s2).exp()).collect();
    let sum: f64 = k.iter().sum();
    for v in k.iter_mut() {
        *v /= sum;
    }

    let (h, w) = a.dim();
    let mut tmp = Array2::<f64>::zeros((h, w));
    for y in 0..h {
        for x in 0..w {
            let mut acc = 0.0;
            for (j, kv) in k.iter().enumerate() {
                let xx = reflect(x as isize + j as isize - r, w as isize);
                acc += kv * a[[y, xx]];
            }
            tmp[[y, x]] = acc;
        }
    }
    let mut out = Array2::<f64>::zeros((h, w));
    for y in 0..h {
        for x in 0..w {
            let mut acc = 0.0;
            for (j, kv) in k.iter().enumerate() {
                let yy = reflect(y as isize + j as isize - r, h as isize);
                acc += kv * tmp[[yy, x]];
            }
            out[[y, x]] = acc;
        }
    }
    out
}

/// Exact euclidean distance transform (Felzenszwalb–Huttenlocher):
/// distance from every `true` cell to the nearest `false` cell.
pub fn distance_transform_edt(mask: &Array2<bool>) -> Array2<f64> {
    let (h, w) = mask.dim();
    let inf = (h + w) as f64;

    // vertical pass: distance along columns
    let mut g = Array2::<f64>::zeros((h, w));
    for x in 0..w {
        g[[0, x]] = if mask[[0, x]] { inf } else { 0.0 };
        for y in 1..h {
            g[[y, x]] = if mask[[y, x]] {
                (g[[y - 1, x]] + 1.0).min(inf)
            } else {
                0.0
            };
        }
        for y in (0..h - 1).rev() {
            if g[[y + 1, x]] + 1.0 < g[[y, x]] {
                g[[y, x]] = g[[y + 1, x]] + 1.0;
            }
        }
    }

    // horizontal pass: lower envelope of parabolas over f = g^2
    let mut out = Array2::<f64>::zeros((h, w));
    let mut f = vec![0.0f64; w];
    let mut v = vec![0usize; w];
    let mut z = vec![0.0f64; w + 1];
    for y in 0..h {
        for x in 0..w {
            f[x] = g[[y, x]] * g[[y, x]];
        }
        let mut k = 0usize;
        v[0] = 0;
        z[0] = f64::NEG_INFINITY;
        z[1] = f64::INFINITY;
        for q in 1..w {
            loop {
                let s = ((f[q] + (q * q) as f64) - (f[v[k]] + (v[k] * v[k]) as f64))
                    / (2.0 * q as f64 - 2.0 * v[k] as f64);
                if s <= z[k] {
                    if k == 0 {
                        v[0] = q;
                        z[1] = f64::INFINITY;
                        break;
                    }
                    k -= 1;
                } else {
                    k += 1;
                    v[k] = q;
                    z[k] = s;
                    z[k + 1] = f64::INFINITY;
                    break;
                }
            }
        }
        k = 0;
        for q in 0..w {
            while z[k + 1] < q as f64 {
                k += 1;
            }
            let dx = q as f64 - v[k] as f64;
            out[[y, q]] = (dx * dx + f[v[k]]).sqrt();
        }
    }
    out
}

/// Binary dilation with the scipy default cross (4-connected) structure.
pub fn binary_dilation(m: &Array2<bool>, iterations: usize) -> Array2<bool> {
    let (h, w) = m.dim();
    let mut cur = m.clone();
    for _ in 0..iterations {
        let mut next = cur.clone();
        for y in 0..h {
            for x in 0..w {
                if cur[[y, x]] {
                    continue;
                }
                if (y > 0 && cur[[y - 1, x]])
                    || (y + 1 < h && cur[[y + 1, x]])
                    || (x > 0 && cur[[y, x - 1]])
                    || (x + 1 < w && cur[[y, x + 1]])
                {
                    next[[y, x]] = true;
                }
            }
        }
        cur = next;
    }
    cur
}

/// Separable maximum filter, mode='reflect' (scipy default).
pub fn maximum_filter(a: &Array2<f64>, size: usize) -> Array2<f64> {
    let r = (size / 2) as isize;
    let (h, w) = a.dim();
    let mut tmp = Array2::<f64>::zeros((h, w));
    for y in 0..h {
        for x in 0..w {
            let mut m = f64::NEG_INFINITY;
            for d in -r..=r {
                let xx = reflect(x as isize + d, w as isize);
                m = m.max(a[[y, xx]]);
            }
            tmp[[y, x]] = m;
        }
    }
    let mut out = Array2::<f64>::zeros((h, w));
    for y in 0..h {
        for x in 0..w {
            let mut m = f64::NEG_INFINITY;
            for d in -r..=r {
                let yy = reflect(y as isize + d, h as isize);
                m = m.max(tmp[[yy, x]]);
            }
            out[[y, x]] = m;
        }
    }
    out
}

/// Connected-component labeling. `eight` selects 8-connectivity (_S8).
pub struct Labeled {
    pub lab: Array2<i32>,
    pub n: usize,
    pub areas: Vec<f64>,
    /// (y0, y1_exclusive, x0, x1_exclusive) per label (1-based -> index-1)
    pub bbox: Vec<(usize, usize, usize, usize)>,
}

pub fn label(mask: &Array2<bool>, eight: bool) -> Labeled {
    let (h, w) = mask.dim();
    let mut lab = Array2::<i32>::zeros((h, w));
    let mut areas: Vec<f64> = Vec::new();
    let mut bbox: Vec<(usize, usize, usize, usize)> = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut n: i32 = 0;

    let n4: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    let n8: [(isize, isize); 8] = [
        (-1, -1),
        (-1, 0),
        (-1, 1),
        (0, -1),
        (0, 1),
        (1, -1),
        (1, 0),
        (1, 1),
    ];

    for sy in 0..h {
        for sx in 0..w {
            if !mask[[sy, sx]] || lab[[sy, sx]] != 0 {
                continue;
            }
            n += 1;
            let mut area = 0.0f64;
            let (mut y0, mut y1, mut x0, mut x1) = (sy, sy + 1, sx, sx + 1);
            stack.push((sy, sx));
            lab[[sy, sx]] = n;
            while let Some((y, x)) = stack.pop() {
                area += 1.0;
                y0 = y0.min(y);
                y1 = y1.max(y + 1);
                x0 = x0.min(x);
                x1 = x1.max(x + 1);
                let neigh: &[(isize, isize)] = if eight { &n8 } else { &n4 };
                for &(dy, dx) in neigh {
                    let ny = y as isize + dy;
                    let nx = x as isize + dx;
                    if ny < 0 || nx < 0 || ny >= h as isize || nx >= w as isize {
                        continue;
                    }
                    let (ny, nx) = (ny as usize, nx as usize);
                    if mask[[ny, nx]] && lab[[ny, nx]] == 0 {
                        lab[[ny, nx]] = n;
                        stack.push((ny, nx));
                    }
                }
            }
            areas.push(area);
            bbox.push((y0, y1, x0, x1));
        }
    }
    Labeled {
        lab,
        n: n as usize,
        areas,
        bbox,
    }
}

/// Largest components: (label_index_1based, area), area-desc, >= min_area, capped.
pub fn top_components(l: &Labeled, min_area: f64, cap: usize) -> Vec<(usize, f64)> {
    let mut order: Vec<usize> = (0..l.n).collect();
    order.sort_by(|&a, &b| {
        l.areas[b]
            .partial_cmp(&l.areas[a])
            .unwrap()
            .then(a.cmp(&b))
    });
    order
        .into_iter()
        .filter(|&i| l.areas[i] >= min_area)
        .take(cap)
        .map(|i| (i + 1, l.areas[i]))
        .collect()
}

/// Point deepest inside a component — map edges count as boundaries.
pub fn interior_anchor(l: &Labeled, idx: usize) -> (usize, usize) {
    let (y0, y1, x0, x1) = l.bbox[idx - 1];
    let (hh, ww) = (y1 - y0 + 2, x1 - x0 + 2);
    let mut m = Array2::<bool>::from_elem((hh, ww), false);
    for y in y0..y1 {
        for x in x0..x1 {
            if l.lab[[y, x]] == idx as i32 {
                m[[y - y0 + 1, x - x0 + 1]] = true;
            }
        }
    }
    let d = distance_transform_edt(&m);
    let mut best = f64::NEG_INFINITY;
    let (mut by, mut bx) = (0usize, 0usize);
    for y in 1..hh - 1 {
        for x in 1..ww - 1 {
            if d[[y, x]] > best {
                best = d[[y, x]];
                by = y;
                bx = x;
            }
        }
    }
    (y0 + by - 1, x0 + bx - 1)
}

/// Peak of `field` inside a component (row-major first maximum).
pub fn peak_anchor(l: &Labeled, idx: usize, field: &Array2<f64>) -> (usize, usize) {
    let (y0, y1, x0, x1) = l.bbox[idx - 1];
    let mut best = f64::NEG_INFINITY;
    let (mut by, mut bx) = (y0, x0);
    for y in y0..y1 {
        for x in x0..x1 {
            if l.lab[[y, x]] == idx as i32 && field[[y, x]] > best {
                best = field[[y, x]];
                by = y;
                bx = x;
            }
        }
    }
    (by, bx)
}

/// np.gradient — returns (d/dy, d/dx); central interior, one-sided edges.
pub fn gradient(a: &Array2<f64>) -> (Array2<f64>, Array2<f64>) {
    let (h, w) = a.dim();
    let mut gy = Array2::<f64>::zeros((h, w));
    let mut gx = Array2::<f64>::zeros((h, w));
    for y in 0..h {
        for x in 0..w {
            gy[[y, x]] = if y == 0 {
                a[[1, x]] - a[[0, x]]
            } else if y == h - 1 {
                a[[h - 1, x]] - a[[h - 2, x]]
            } else {
                (a[[y + 1, x]] - a[[y - 1, x]]) / 2.0
            };
            gx[[y, x]] = if x == 0 {
                a[[y, 1]] - a[[y, 0]]
            } else if x == w - 1 {
                a[[y, w - 1]] - a[[y, w - 2]]
            } else {
                (a[[y, x + 1]] - a[[y, x - 1]]) / 2.0
            };
        }
    }
    (gy, gx)
}
