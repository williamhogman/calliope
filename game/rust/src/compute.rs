//! The compute lane (M67, ADR-0027) — shared wgpu compute plumbing and
//! the one coast-distance law.
//!
//! The lane is the engine facility the M67 spec asked for: buffer
//! staging, dispatch sizing, readback synchronization and the
//! CPU-fallback contract live here as shared code, so the next compute
//! pass registers a `PassSpec` and a CPU twin instead of duplicating
//! wgpu boilerplate.
//!
//! The first client is coast distance. The old law was a two-pass
//! chamfer (1/1.4 weights, scanline-sequential) copied in render.rs and
//! compositor.js; the law is now one integer jump-flood (JFA): seeds
//! are u32 cell indices, distances exact u32 squares, ties break toward
//! the smaller index. Integer end to end means the WGSL kernel and the
//! CPU twin produce byte-identical seed fields on any conformant device
//! — that byte-parity IS the contract, checked at bring-up on a fixture
//! and natively by `diagnose compute` on real worlds every suite run
//! (the harness sources a software Vulkan adapter when headless, so the
//! GPU leg *executes* in CI rather than being claimed).
//!
//! Production truth never depends on a device: the texture uploaded by
//! render.rs is always the CPU twin's output, which equals the GPU's
//! wherever the contract holds. A per-frame client (Era II's sky is the
//! expected customer) is what flips production dispatch to the GPU;
//! that switch rides on this same contract.
//!
//! Determinism note (ADR-0003/0025): nothing here feeds `hash_state` —
//! coast distance is display-side derived state. The lane refuses f64
//! clients by construction: WGSL has no f64, which is exactly why the
//! displaced GPU-erosion item closed as a decision (ADR-0027) instead
//! of code.

use crate::util::Band;

/// The WGSL side of the coast law — kept beside the twin that mirrors it.
pub const COAST_WGSL: &str = include_str!("shaders/coastdist.wgsl");

/// "No seed reaches this cell yet."
pub const NONE: u32 = 0xffff_ffff;

// ================================================================ the law

/// Seed grid from a height field: land cells (h >= 0) seed themselves.
pub fn coast_seeds(height: &[f32], w: usize, h: usize) -> Vec<u32> {
    let mut s = vec![NONE; w * h];
    for (i, v) in s.iter_mut().enumerate().take(w * h) {
        if height[i] >= 0.0 {
            *v = i as u32;
        }
    }
    s
}

/// The one stride schedule both executors walk: next_pow2(max(w,h))/2 … 1.
pub fn jfa_strides(w: usize, h: usize) -> Vec<u32> {
    let mut n: u32 = 1;
    while (n as usize) < w.max(h) {
        n <<= 1;
    }
    let mut out = Vec::new();
    let mut s = n >> 1;
    while s >= 1 {
        out.push(s);
        if s == 1 {
            break;
        }
        s >>= 1;
    }
    out
}

/// One jump-flood pass — the CPU twin of `coastdist.wgsl`, line for
/// line: nine candidates a stride away, exact u32 squared distances,
/// ties to the smaller seed index. Any edit here edits the shader too.
pub fn jfa_pass_cpu(src: &[u32], dst: &mut [u32], w: usize, h: usize, stride: u32) {
    let s = stride as isize;
    for y in 0..h as isize {
        for x in 0..w as isize {
            let i = (y as usize) * w + x as usize;
            let mut best = src[i];
            let mut bd: u32 = 0;
            if best != NONE {
                let sx = (best as usize % w) as isize;
                let sy = (best as usize / w) as isize;
                let (dx, dy) = (sx - x, sy - y);
                bd = (dx * dx + dy * dy) as u32;
            }
            for oy in -1..=1isize {
                for ox in -1..=1isize {
                    if ox == 0 && oy == 0 {
                        continue;
                    }
                    let nx = x + ox * s;
                    let ny = y + oy * s;
                    if nx < 0 || ny < 0 || nx >= w as isize || ny >= h as isize {
                        continue;
                    }
                    let cand = src[ny as usize * w + nx as usize];
                    if cand == NONE {
                        continue;
                    }
                    let sx = (cand as usize % w) as isize;
                    let sy = (cand as usize / w) as isize;
                    let (dx, dy) = (sx - x, sy - y);
                    let d = (dx * dx + dy * dy) as u32;
                    if best == NONE || d < bd || (d == bd && cand < best) {
                        best = cand;
                        bd = d;
                    }
                }
            }
            dst[i] = best;
        }
    }
}

/// The full CPU JFA: walk the stride schedule, ping-pong, return seeds.
pub fn jfa_cpu(mut seeds: Vec<u32>, w: usize, h: usize) -> Vec<u32> {
    let mut back = vec![NONE; w * h];
    for stride in jfa_strides(w, h) {
        jfa_pass_cpu(&seeds, &mut back, w, h, stride);
        std::mem::swap(&mut seeds, &mut back);
    }
    seeds
}

/// Seeds to distances: land 0, sea √(exact integer square) — the squares
/// stay below 2^24 on any legal grid, so the f32 sqrt is bit-exact.
pub fn finalize(seeds: &[u32], w: usize, _h: usize) -> Vec<f32> {
    seeds
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            if s == NONE {
                1e9
            } else if s as usize == i {
                0.0
            } else {
                let (x, y) = ((i % w) as isize, (i / w) as isize);
                let (sx, sy) = ((s as usize % w) as isize, (s as usize / w) as isize);
                let (dx, dy) = (sx - x, sy - y);
                ((dx * dx + dy * dy) as f32).sqrt()
            }
        })
        .collect()
}

/// The production coast law: distance in cells from every sea cell to
/// the nearest land. This is what render.rs uploads; compositor.js
/// mirrors it in JS for the adapterless path.
pub fn coast_distance(height: &[f32], w: usize, h: usize) -> Vec<f32> {
    finalize(&jfa_cpu(coast_seeds(height, w, h), w, h), w, h)
}

// ============================================================ the referee

/// Exact squared Euclidean distance transform (Felzenszwalb–Huttenlocher,
/// two 1-D parabola passes) — the referee `diagnose compute` holds the
/// JFA against. Harness-only: never compiled into the wasm.
#[cfg(not(target_arch = "wasm32"))]
pub fn exact_edt_sq(land: &[bool], w: usize, h: usize) -> Vec<f64> {
    const INF: f64 = 1e20;

    fn dt1(f: &[f64], d: &mut [f64], v: &mut [usize], z: &mut [f64]) {
        let n = f.len();
        let mut k = 0usize;
        v[0] = 0;
        z[0] = -INF;
        z[1] = INF;
        for q in 1..n {
            loop {
                let p = v[k];
                let s = ((f[q] + (q * q) as f64) - (f[p] + (p * p) as f64))
                    / (2.0 * (q as f64 - p as f64));
                if s <= z[k] {
                    if k == 0 {
                        v[0] = q;
                        z[0] = -INF;
                        z[1] = INF;
                        break;
                    }
                    k -= 1;
                } else {
                    k += 1;
                    v[k] = q;
                    z[k] = s;
                    z[k + 1] = INF;
                    break;
                }
            }
        }
        let mut k = 0usize;
        for q in 0..n {
            while z[k + 1] < q as f64 {
                k += 1;
            }
            let p = v[k];
            d[q] = (q as f64 - p as f64) * (q as f64 - p as f64) + f[p];
        }
    }

    let n = w.max(h);
    let mut g = vec![0.0f64; w * h];
    for i in 0..w * h {
        g[i] = if land[i] { 0.0 } else { INF };
    }
    let (mut f, mut d) = (vec![0.0; n], vec![0.0; n]);
    let (mut v, mut z) = (vec![0usize; n], vec![0.0f64; n + 1]);
    // columns
    for x in 0..w {
        for y in 0..h {
            f[y] = g[y * w + x];
        }
        dt1(&f[..h], &mut d[..h], &mut v, &mut z);
        for y in 0..h {
            g[y * w + x] = d[y];
        }
    }
    // rows
    let mut out = vec![0.0f64; w * h];
    for y in 0..h {
        f[..w].copy_from_slice(&g[y * w..y * w + w]);
        dt1(&f[..w], &mut d[..w], &mut v, &mut z);
        out[y * w..y * w + w].copy_from_slice(&d[..w]);
    }
    out
}

// ============================================================= the fixture

/// Deterministic bring-up fixture: three islands, a lone skerry and an
/// empty quarter, sized to exercise ties, long floods and NONE regions.
pub fn fixture(w: usize, h: usize) -> Vec<f32> {
    let mut hgt = vec![-1.0f32; w * h];
    let disk = |hgt: &mut Vec<f32>, cx: isize, cy: isize, r: isize| {
        for y in (cy - r).max(0)..(cy + r + 1).min(h as isize) {
            for x in (cx - r).max(0)..(cx + r + 1).min(w as isize) {
                let (dx, dy) = (x - cx, y - cy);
                if dx * dx + dy * dy <= r * r {
                    hgt[y as usize * w + x as usize] = 1.0;
                }
            }
        }
    };
    disk(&mut hgt, (w / 4) as isize, (h / 3) as isize, (h / 7) as isize);
    disk(&mut hgt, (2 * w / 3) as isize, (2 * h / 3) as isize, (h / 5) as isize);
    disk(&mut hgt, (w / 12) as isize, (5 * h / 6) as isize, 1);
    hgt
}

// ================================================================ the lane

/// Diagnostics bands (E2.7) — measured by `diagnose compute` across the
/// suite seeds; re-derive against the measured run, never guess.
pub const BANDS: &[Band] = &[
    // JFA is an approximation of the true EDT with rare, small misses;
    // the referee (exact_edt_sq) measures the miss. Measured at 512²
    // across the suite seeds: max err 0.00–0.03 cells, wrong-cell share
    // under 0.1% — sweet holds that envelope, hard tolerates a coarse
    // fixture without ever letting a visible artifact through (>1 cell).
    Band { name: "jfa max err cells", sweet: (0.0, 0.25), hard: (0.0, 1.0), target: "sweet ≤0.25 · hard ≤1 (M67: worst |jfa−exact| over sea cells, in cells — display falloff tolerates <1)" },
    Band { name: "jfa wrong cell share", sweet: (0.0, 0.005), hard: (0.0, 0.02), target: "sweet ≤0.5% · hard ≤2% (M67: share of sea cells whose JFA distance differs from the exact EDT)" },
    // Once per world upload, CPU twin. The chamfer it replaced ran ~5 ms;
    // the JFA buys one law across GPU/CPU/JS for a once-per-world cost.
    Band { name: "coast law cpu ms", sweet: (0.0, 120.0), hard: (0.0, 400.0), target: "sweet ≤120 · hard ≤400 (M67: CPU-twin JFA at 512², once per world upload — not per frame)" },
];

/// The wgpu side — compiled wherever wgpu exists: always on wasm (the
/// renderer's dependency), natively only under the `gpu` feature (the
/// harness leg).
#[cfg(any(target_arch = "wasm32", feature = "gpu"))]
mod lane {
    use super::*;
    use std::collections::HashMap;

    /// What a compute pass registers: shader, entry point, workgroup.
    pub struct PassSpec {
        pub label: &'static str,
        pub wgsl: &'static str,
        pub entry: &'static str,
        pub workgroup: (u32, u32),
    }

    /// The coast client's registration.
    pub const COAST_PASS: PassSpec = PassSpec {
        label: "coast-jfa",
        wgsl: COAST_WGSL,
        entry: "jfa",
        workgroup: (8, 8),
    };

    /// Outcome of holding a device to the CPU twin.
    pub struct ContractReport {
        pub matched: bool,
        pub mismatches: usize,
        pub cells: usize,
        pub gpu_ms: f64,
        pub cpu_ms: f64,
    }

    fn now_ms() -> f64 {
        #[cfg(target_arch = "wasm32")]
        {
            js_sys::Date::now()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::sync::OnceLock;
            use std::time::Instant;
            static T0: OnceLock<Instant> = OnceLock::new();
            T0.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
        }
    }

    /// Shared wgpu compute plumbing: pipeline cache, buffer staging,
    /// dispatch sizing, readback synchronization.
    pub struct ComputeLane {
        device: wgpu::Device,
        queue: wgpu::Queue,
        pipelines: HashMap<&'static str, wgpu::ComputePipeline>,
    }

    impl ComputeLane {
        /// Whether an adapter can run the lane at all (WebGL2 downlevel
        /// has no compute shaders; every real WebGPU/Vulkan path does).
        pub fn adapter_supported(adapter: &wgpu::Adapter) -> bool {
            adapter
                .get_downlevel_capabilities()
                .flags
                .contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
        }

        pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
            ComputeLane { device, queue, pipelines: HashMap::new() }
        }

        /// Compile (once) and cache a pass's pipeline. Validation errors
        /// come back as a named Err — a broken future client fails as a
        /// row, not a crash.
        pub async fn pipeline(&mut self, spec: &PassSpec) -> Result<wgpu::ComputePipeline, String> {
            if let Some(p) = self.pipelines.get(spec.label) {
                return Ok(p.clone());
            }
            self.device.push_error_scope(wgpu::ErrorFilter::Validation);
            let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(spec.label),
                source: wgpu::ShaderSource::Wgsl(spec.wgsl.into()),
            });
            let pipe = self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(spec.label),
                layout: None,
                module: &module,
                entry_point: Some(spec.entry),
                compilation_options: Default::default(),
                cache: None,
            });
            #[cfg(not(target_arch = "wasm32"))]
            self.device.poll(wgpu::Maintain::Wait);
            if let Some(e) = self.device.pop_error_scope().await {
                return Err(format!("{}: shader/pipeline validation: {e}", spec.label));
            }
            self.pipelines.insert(spec.label, pipe.clone());
            Ok(pipe)
        }

        /// Staging up: a storage buffer holding `words`.
        pub fn storage(&self, words: &[u32]) -> wgpu::Buffer {
            use wgpu::util::DeviceExt;
            self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("lane-storage"),
                contents: bytemuck::cast_slice(words),
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::COPY_SRC,
            })
        }

        /// An uninitialized storage buffer of `words` u32s (every lane
        /// pass writes all of its output, so garbage never survives).
        pub fn storage_empty(&self, words: usize) -> wgpu::Buffer {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("lane-storage-out"),
                size: (words * 4) as u64,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            })
        }

        pub fn uniform(&self, bytes: &[u8]) -> wgpu::Buffer {
            use wgpu::util::DeviceExt;
            self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("lane-uniform"),
                contents: bytes,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            })
        }

        /// One dispatch over a w×h grid: bind group 0 in binding order,
        /// workgroup counts rounded up from the pass's workgroup size.
        pub fn dispatch(
            &self,
            pipe: &wgpu::ComputePipeline,
            spec: &PassSpec,
            buffers: &[&wgpu::Buffer],
            w: u32,
            h: u32,
        ) {
            let entries: Vec<wgpu::BindGroupEntry> = buffers
                .iter()
                .enumerate()
                .map(|(i, b)| wgpu::BindGroupEntry {
                    binding: i as u32,
                    resource: b.as_entire_binding(),
                })
                .collect();
            let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(spec.label),
                layout: &pipe.get_bind_group_layout(0),
                entries: &entries,
            });
            let mut enc = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(spec.label) });
            {
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some(spec.label),
                    timestamp_writes: None,
                });
                pass.set_pipeline(pipe);
                pass.set_bind_group(0, &bind, &[]);
                pass.dispatch_workgroups(w.div_ceil(spec.workgroup.0), h.div_ceil(spec.workgroup.1), 1);
            }
            self.queue.submit(Some(enc.finish()));
        }

        /// Readback synchronization: copy to a staging buffer, map, wait.
        /// Native waits on the device; wasm awaits the browser's map.
        pub async fn read_u32(&self, src: &wgpu::Buffer, words: usize) -> Result<Vec<u32>, String> {
            let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("lane-readback"),
                size: (words * 4) as u64,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let mut enc = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("lane-readback") });
            enc.copy_buffer_to_buffer(src, 0, &staging, 0, (words * 4) as u64);
            self.queue.submit(Some(enc.finish()));

            let (tx, rx) = futures_channel::oneshot::channel();
            staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
            #[cfg(not(target_arch = "wasm32"))]
            self.device.poll(wgpu::Maintain::Wait);
            #[cfg(target_arch = "wasm32")]
            let _ = self.device.poll(wgpu::Maintain::Poll);
            match rx.await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => return Err(format!("readback map failed: {e}")),
                Err(_) => return Err("readback channel dropped".into()),
            }
            let view = staging.slice(..).get_mapped_range();
            let out: Vec<u32> = bytemuck::cast_slice(&view).to_vec();
            drop(view);
            staging.unmap();
            Ok(out)
        }
    }

    /// The coast client on the GPU: upload seeds, walk the shared stride
    /// schedule ping-pong, read the seed field back.
    pub async fn coast_seeds_gpu(
        lane: &mut ComputeLane,
        seeds: &[u32],
        w: u32,
        h: u32,
    ) -> Result<Vec<u32>, String> {
        let pipe = lane.pipeline(&COAST_PASS).await?;
        let mut a = lane.storage(seeds);
        let mut b = lane.storage_empty(seeds.len());
        for stride in jfa_strides(w as usize, h as usize) {
            let params: [u32; 4] = [w, h, stride, 0];
            let u = lane.uniform(bytemuck::cast_slice(&params));
            lane.dispatch(&pipe, &COAST_PASS, &[&u, &a, &b], w, h);
            std::mem::swap(&mut a, &mut b);
        }
        lane.read_u32(&a, seeds.len()).await
    }

    /// The CPU-fallback contract, executed: run the GPU leg and the twin
    /// on the same seeds and compare every byte.
    pub async fn coast_contract(
        lane: &mut ComputeLane,
        height: &[f32],
        w: usize,
        h: usize,
    ) -> Result<ContractReport, String> {
        let seeds = coast_seeds(height, w, h);
        let t0 = now_ms();
        let gpu = coast_seeds_gpu(lane, &seeds, w as u32, h as u32).await?;
        let t1 = now_ms();
        let cpu = jfa_cpu(seeds, w, h);
        let t2 = now_ms();
        let mismatches = gpu.iter().zip(&cpu).filter(|(a, b)| a != b).count();
        Ok(ContractReport {
            matched: mismatches == 0,
            mismatches,
            cells: cpu.len(),
            gpu_ms: t1 - t0,
            cpu_ms: t2 - t1,
        })
    }
}

#[cfg(any(target_arch = "wasm32", feature = "gpu"))]
pub use lane::{coast_contract, coast_seeds_gpu, ComputeLane, ContractReport, PassSpec, COAST_PASS};

// ======================================================== wasm bring-up

/// Browser-side lane status: probed at bring-up, read by the HUD and the
/// browser probe. wasm is single-threaded, so a thread_local is the truth.
#[cfg(target_arch = "wasm32")]
mod wasm_glue {
    use std::cell::RefCell;

    thread_local! {
        static STATUS: RefCell<String> = RefCell::new("not probed".into());
    }

    pub fn status() -> String {
        STATUS.with(|s| s.borrow().clone())
    }

    pub fn set_status(v: String) {
        STATUS.with(|s| *s.borrow_mut() = v);
    }

    /// Run the bring-up contract on the fixture and record the verdict.
    /// `supported` arrives from the adapter probe at Orbital::finish —
    /// WebGL2 downlevel simply reports the twin as the law.
    pub async fn bringup(supported: bool, device: wgpu::Device, queue: wgpu::Queue) -> String {
        if !supported {
            let s = "cpu-twin (no compute: webgl2 downlevel)".to_string();
            set_status(s.clone());
            return s;
        }
        let (w, h) = (96usize, 64usize);
        let fix = super::fixture(w, h);
        let mut lane = super::ComputeLane::new(device, queue);
        let s = match super::coast_contract(&mut lane, &fix, w, h).await {
            Ok(r) if r.matched => format!(
                "gpu: contract byte-parity ok (fixture {w}×{h} · gpu {:.1}ms · cpu {:.1}ms)",
                r.gpu_ms, r.cpu_ms
            ),
            Ok(r) => format!(
                "DEGRADED to cpu-twin: {} of {} cells diverged on the fixture",
                r.mismatches, r.cells
            ),
            Err(e) => format!("DEGRADED to cpu-twin: {e}"),
        };
        set_status(s.clone());
        s
    }
}

#[cfg(target_arch = "wasm32")]
pub use wasm_glue::{bringup as wasm_bringup, status as wasm_status};

// ================================================================== tests

#[cfg(test)]
mod tests {
    use super::*;

    /// The referee is itself refereed: exact EDT against brute force on
    /// the fixture.
    #[test]
    fn edt_matches_brute_force() {
        let (w, h) = (48usize, 32usize);
        let hgt = fixture(w, h);
        let land: Vec<bool> = hgt.iter().map(|&v| v >= 0.0).collect();
        let edt = exact_edt_sq(&land, w, h);
        let seeds: Vec<(isize, isize)> = (0..w * h)
            .filter(|&i| land[i])
            .map(|i| ((i % w) as isize, (i / w) as isize))
            .collect();
        for y in 0..h as isize {
            for x in 0..w as isize {
                let i = y as usize * w + x as usize;
                let brute = seeds
                    .iter()
                    .map(|&(sx, sy)| ((sx - x) * (sx - x) + (sy - y) * (sy - y)) as f64)
                    .fold(f64::INFINITY, f64::min);
                assert_eq!(edt[i], brute, "cell ({x},{y})");
            }
        }
    }

    /// Land cells are their own seed at distance zero; sea distances are
    /// within the JFA error envelope of exact.
    #[test]
    fn jfa_close_to_exact_on_fixture() {
        let (w, h) = (96usize, 64usize);
        let hgt = fixture(w, h);
        let d = coast_distance(&hgt, w, h);
        let land: Vec<bool> = hgt.iter().map(|&v| v >= 0.0).collect();
        let edt = exact_edt_sq(&land, w, h);
        let mut worst = 0.0f64;
        for i in 0..w * h {
            if land[i] {
                assert_eq!(d[i], 0.0);
            } else {
                worst = worst.max((d[i] as f64 - edt[i].sqrt()).abs());
            }
        }
        assert!(worst <= 1.0, "jfa err {worst} cells");
    }
}
