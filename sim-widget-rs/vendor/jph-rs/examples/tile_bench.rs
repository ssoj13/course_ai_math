//! Per-thread tile encode/decode throughput + bit-exactness fingerprint for exv-like codestreams.
//!
//! Mirrors how `exv-rs` (`exv-codec/src/htj2k.rs`) drives jph-rs: one pooled [`Codestream`] that is
//! `restart()`ed per tile, a multi-component reversible 5/3 stream (5 decompositions, 64x64 HT
//! blocks, no RCT, binary-complement NLT on signed "float bit pattern" components) and the
//! interleaved per-line `exchange` / `pull` protocol. Single thread on purpose: the numbers are
//! MB/s **per thread** (raw sample bytes: 1/2/4 B per sample by type).
//!
//! ```text
//! cargo run --release --example tile_bench                      # core matrix, 7 interleaved runs, medians
//! cargo run --release --example tile_bench -- --all             # full matrix (sizes x comps x types x content)
//! cargo run --release --example tile_bench -- --check           # fingerprints only (codestream + decoded samples)
//! cargo run --release --example tile_bench -- --dump DIR        # also write raw inputs + .j2c streams (C++ comparison)
//! cargo run --release --example tile_bench -- --runs 9 --ms 60  # runs per case / target ms per timed sample
//! cargo run --release --example tile_bench -- --filter 256x5    # substring filter on the case name
//! ```
//! `--check` output is the bit-exactness gate for optimisation work: it must not change.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::Instant;

use jph_rs::{Codestream, MemInfile, Point, Ui32, VecOutfile};

/// Counting allocator: per-tile allocation churn (count + bytes) of the steady state.
struct Counting;
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static TRACE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
thread_local!(static IN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
/// `--trace-alloc`: print the (symbolised, deduplicated by caller) call site of every steady-state allocation.
fn trace(size: usize) {
    if TRACE.load(Relaxed) && !IN.with(|c| c.replace(true)) {
        let bt = std::backtrace::Backtrace::force_capture().to_string();
        let site: Vec<&str> = bt.lines().filter(|l| l.contains("jph_rs::") && !l.contains("ojph_mem::")).take(2).collect();
        eprintln!("ALLOC {size:>8} {}", site.join(" <- ").replace("  ", ""));
        IN.with(|c| c.set(false));
    }
}
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        trace(l.size());
        ALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(l.size() as u64, Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(l.size() as u64, Relaxed);
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(n as u64, Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static GLOBAL: Counting = Counting;

/// `(allocations, bytes)` made by one call of `f`.
fn churn(f: impl FnOnce()) -> (u64, u64) {
    let (a, b) = (ALLOCS.load(Relaxed), BYTES.load(Relaxed));
    f();
    (ALLOCS.load(Relaxed) - a, BYTES.load(Relaxed) - b)
}

/// Sample type of a component: J2K bit depth + signedness, and whether it rides the
/// binary-complement NLT (signed = float bit patterns, exactly as exv-rs codes F16/F32).
#[derive(Clone, Copy, PartialEq)]
enum Ty {
    U8,
    U16,
    F16,
    U32,
    F32,
}

impl Ty {
    fn name(self) -> &'static str {
        match self {
            Ty::U8 => "u8",
            Ty::U16 => "u16",
            Ty::F16 => "f16",
            Ty::U32 => "u32",
            Ty::F32 => "f32",
        }
    }
    fn bits(self) -> Ui32 {
        match self {
            Ty::U8 => 8,
            Ty::U16 | Ty::F16 => 16,
            Ty::U32 | Ty::F32 => 32,
        }
    }
    fn signed(self) -> bool {
        matches!(self, Ty::F16 | Ty::F32)
    }
    fn esz(self) -> usize {
        self.bits().div_ceil(8) as usize
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Content {
    Noise,
    Grad,
    Synth,
}

impl Content {
    fn name(self) -> &'static str {
        match self {
            Content::Noise => "noise",
            Content::Grad => "grad",
            Content::Synth => "synth",
        }
    }
}

/// xorshift64* — deterministic, dependency-free.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// f32 -> IEEE half bit pattern (round-to-nearest-even; finite inputs only here).
fn f16_bits(v: f32) -> u16 {
    let b = v.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xff) as i32 - 127 + 15;
    let man = b & 0x7f_ffff;
    if exp <= 0 {
        if exp < -10 {
            return sign;
        }
        let m = (man | 0x80_0000) >> (1 - exp);
        return sign | ((m + 0x1000) >> 13) as u16;
    }
    if exp >= 31 {
        return sign | 0x7c00;
    }
    let h = ((exp as u32) << 10) | (man >> 13);
    let round = (man >> 12) & 1 != 0 && ((man & 0xfff) != 0 || (h & 1) != 0);
    sign | (h + round as u32) as u16
}

/// One component's samples as the i32 values jph sees (sign-extended for signed, zero-extended else).
fn gen_plane(w: usize, h: usize, ty: Ty, content: Content, seed: u64) -> Vec<i32> {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ seed.wrapping_mul(0x1000_0001));
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            // A "value" in [0, 1) driven by the content kind (float types map it to scene-linear-ish).
            let fx = x as f32 / w as f32;
            let fy = y as f32 / h as f32;
            let v = match content {
                Content::Noise => rng.unit(),
                Content::Grad => 0.5 * fx + 0.5 * fy,
                Content::Synth => {
                    let base = 0.5 + 0.35 * ((fx * 9.0).sin() * (fy * 7.0 + seed as f32).cos());
                    (base + 0.02 * (rng.unit() - 0.5)).clamp(0.0, 0.999)
                }
            };
            let s = match ty {
                Ty::U8 => (v * 255.0) as i32,
                Ty::U16 => (v * 65535.0) as i32,
                Ty::U32 => match content {
                    // Object ids: blocky integer regions (realistic for U32 AOVs), noise = random ids.
                    Content::Noise => rng.next() as u32 as i32,
                    _ => ((x / 37) as u32 * 7919 + (y / 29) as u32 * 104_729 + seed as u32) as i32,
                },
                // HDR-ish: [0, 1) -> [0, 8) scene-linear; the bit pattern is the sample (sign-extended).
                Ty::F16 => f16_bits(v * 8.0) as i16 as i32,
                Ty::F32 => (v * 8.0).to_bits() as i32,
            };
            out.push(s);
        }
    }
    out
}

struct Case {
    name: String,
    w: usize,
    h: usize,
    tys: Vec<Ty>,
    planes: Vec<Vec<i32>>,
    raw_bytes: usize,
}

fn make_case(size: usize, nc: usize, ty: Ty, content: Content) -> Case {
    let tys = vec![ty; nc];
    let planes = (0..nc).map(|c| gen_plane(size, size, ty, content, c as u64 + 1)).collect();
    Case {
        name: format!("{size}x{nc}c {} {}", ty.name(), content.name()),
        w: size,
        h: size,
        raw_bytes: size * size * nc * ty.esz(),
        tys,
        planes,
    }
}

/// exv-rs `encode_mc_core` (reversible branch) on a pooled codestream.
fn encode(cs: &mut Codestream, c: &Case, out: &mut Vec<u8>) {
    cs.restart();
    let nc = c.tys.len();
    cs.access_siz().set_num_components(nc as Ui32);
    for (ci, ty) in c.tys.iter().enumerate() {
        cs.access_siz().set_comp_info(ci as Ui32, Point::new(1, 1), ty.bits(), ty.signed());
        if ty.signed() {
            cs.access_nlt_mut().set_nonlinear_transform(ci as Ui32, 3).expect("nlt");
        }
    }
    cs.set_planar(false);
    cs.access_siz().set_image_offset(Point::new(0, 0));
    cs.access_siz().set_image_extent(Point::new(c.w as Ui32, c.h as Ui32));
    {
        let cod = cs.access_cod();
        cod.employ_color_transform(0);
        cod.set_reversible(true);
        cod.set_block_dims(64, 64).expect("block dims");
        cod.set_num_decompositions(5).expect("decomp");
    }
    out.clear();
    // Appends straight into the caller's reused Vec: no per-tile output allocation, no copy-out.
    let mut outfile = VecOutfile::new(out);
    cs.write_headers_slice(&mut outfile, &[]).expect("headers");
    let mut next: Ui32 = 0;
    let mut cur = cs.exchange(std::ptr::null_mut(), &mut next).expect("exchange");
    for y in 0..c.h {
        for p in &c.planes {
            cs.fill_and_exchange(&mut cur, &mut next, |lb, _| {
                lb.get_i32_mut(c.w).copy_from_slice(&p[y * c.w..(y + 1) * c.w])
            })
            .expect("fill");
        }
    }
    cs.flush().expect("flush");
}

/// exv-rs `decode_mc_core` on a pooled codestream: lines go straight into `planes`.
fn decode(cs: &mut Codestream, data: &[u8], w: usize, h: usize, planes: &mut [Vec<i32>]) {
    cs.restart();
    let mut infile = MemInfile::default();
    infile.open(data);
    cs.read_headers(&mut infile).expect("read_headers");
    cs.set_planar(false);
    cs.create().expect("create");
    let mut next: Ui32 = 0;
    for y in 0..h {
        for p in planes.iter_mut() {
            let line = cs.pull_ref(&mut next).expect("pull").get_i32(w);
            p[y * w..(y + 1) * w].copy_from_slice(line);
        }
    }
    infile.close_infile();
}

fn fnv(h: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *h ^= b as u64;
        *h = h.wrapping_mul(0x100_0000_01b3);
    }
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

/// Time `reps` calls of `f`; returns seconds per call.
fn time_per_call(reps: usize, mut f: impl FnMut()) -> f64 {
    let t = Instant::now();
    for _ in 0..reps {
        f();
    }
    t.elapsed().as_secs_f64() / reps as f64
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let flag = |f: &str| args.iter().any(|a| a == f);
    let val = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    let runs: usize = val("--runs").and_then(|s| s.parse().ok()).unwrap_or(7);
    let target_ms: f64 = val("--ms").and_then(|s| s.parse().ok()).unwrap_or(40.0);
    let filter = val("--filter");
    let dump = val("--dump");
    let check = flag("--check");
    // `--only dec|enc` times one direction (cleaner sampling profiles).
    let only = val("--only");
    let (do_dec, do_enc) = (only.as_deref() != Some("enc"), only.as_deref() != Some("dec"));

    let all_ty = [Ty::U8, Ty::U16, Ty::F16, Ty::U32, Ty::F32];
    let all_ct = [Content::Noise, Content::Grad, Content::Synth];
    let mut cases = Vec::new();
    if flag("--all") || check {
        for size in [256, 512] {
            for nc in [1, 3, 5] {
                for ty in all_ty {
                    for ct in all_ct {
                        cases.push(make_case(size, nc, ty, ct));
                    }
                }
            }
        }
    } else {
        // Core matrix: the exv default (F16 x5 synth) at both tile sizes + type/content spread.
        for size in [256, 512] {
            cases.push(make_case(size, 5, Ty::F16, Content::Synth));
        }
        for ty in all_ty {
            for ct in all_ct {
                cases.push(make_case(256, 3, ty, ct));
            }
        }
        cases.push(make_case(256, 1, Ty::F16, Content::Synth));
        cases.push(make_case(512, 1, Ty::U8, Content::Synth));
    }
    if let Some(f) = &filter {
        cases.retain(|c| c.name.contains(f.as_str()));
    }

    let mut cs = Codestream::new();
    let mut streams: Vec<Vec<u8>> = Vec::new();
    let mut total = 0xcbf2_9ce4_8422_2325u64;
    for c in &cases {
        let mut s = Vec::new();
        encode(&mut cs, c, &mut s);
        let mut planes = vec![vec![0i32; c.w * c.h]; c.tys.len()];
        decode(&mut cs, &s, c.w, c.h, &mut planes);
        assert!(planes == c.planes, "{}: lossless round trip failed", c.name);
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        fnv(&mut h, &s);
        fnv(&mut total, &s);
        if check {
            println!("{:<26} bytes={:>8} j2c={h:016x}", c.name, s.len());
        }
        if let Some(dir) = &dump {
            let stem = c.name.replace(' ', "_");
            std::fs::create_dir_all(dir).expect("mkdir");
            std::fs::write(format!("{dir}/{stem}.j2c"), &s).expect("write j2c");
            // Raw input: interleaved-by-line i32 LE (line y of comp 0..nc), what the C++ bench feeds.
            let mut raw = Vec::with_capacity(c.w * c.h * c.tys.len() * 4);
            for y in 0..c.h {
                for p in &c.planes {
                    for v in &p[y * c.w..(y + 1) * c.w] {
                        raw.extend_from_slice(&v.to_le_bytes());
                    }
                }
            }
            std::fs::write(format!("{dir}/{stem}.raw"), raw).expect("write raw");
        }
        streams.push(s);
    }
    if check {
        println!("TOTAL j2c fingerprint {total:016x} ({} cases; decode verified lossless)", cases.len());
        return;
    }

    // `--threads N`: aggregate throughput of N workers (own pooled codestream each) on every case:
    // exposes process-wide serialisation (env lock, allocator) that a 1-thread number hides.
    if let Some(n) = val("--threads").and_then(|s| s.parse::<usize>().ok()) {
        println!("{:<26} {:>12} {:>12}", "case", "dec MB/s agg", "enc MB/s agg");
        for (c, s) in cases.iter().zip(&streams) {
            let reps = 40;
            let run = |enc: bool| {
                let t = Instant::now();
                std::thread::scope(|sc| {
                    for _ in 0..n {
                        sc.spawn(|| {
                            let mut cs = Codestream::new();
                            let mut planes = vec![vec![0i32; c.w * c.h]; c.tys.len()];
                            let mut o = Vec::new();
                            for _ in 0..reps {
                                if enc { encode(&mut cs, c, &mut o) } else { decode(&mut cs, s, c.w, c.h, &mut planes) }
                            }
                        });
                    }
                });
                (c.raw_bytes * reps * n) as f64 / 1e6 / t.elapsed().as_secs_f64()
            };
            let mut d: Vec<f64> = (0..runs).map(|_| run(false)).collect();
            let mut e: Vec<f64> = (0..runs).map(|_| run(true)).collect();
            println!("{:<26} {:>12.0} {:>12.0}", c.name, median(&mut d), median(&mut e));
        }
        return;
    }
    // Calibrate reps per case so each timed sample is ~target_ms.
    let reps: Vec<(usize, usize)> = cases
        .iter()
        .zip(&streams)
        .map(|(c, s)| {
            let mut planes = vec![vec![0i32; c.w * c.h]; c.tys.len()];
            let d = time_per_call(2, || decode(&mut cs, s, c.w, c.h, &mut planes));
            let mut o = Vec::new();
            let e = time_per_call(2, || encode(&mut cs, c, &mut o));
            let r = |t: f64| ((target_ms / 1000.0 / t).ceil() as usize).max(1);
            (r(d), r(e))
        })
        .collect();

    let mut dec = vec![Vec::new(); cases.len()];
    let mut enc = vec![Vec::new(); cases.len()];
    let mut scratch = Vec::new();
    for _ in 0..runs {
        for (i, (c, s)) in cases.iter().zip(&streams).enumerate() {
            let mut planes = vec![vec![0i32; c.w * c.h]; c.tys.len()];
            if do_dec {
                dec[i].push(time_per_call(reps[i].0, || decode(&mut cs, s, c.w, c.h, &mut planes)));
            }
            if do_enc {
                enc[i].push(time_per_call(reps[i].1, || encode(&mut cs, c, &mut scratch)));
            }
        }
    }
    println!(
        "{:<26} {:>9} {:>6} {:>8} {:>8} {:>8} {:>8} {:>13} {:>13}",
        "case", "j2c B", "ratio", "dec ms", "dec MB/s", "enc ms", "enc MB/s", "dec allocs/KB", "enc allocs/KB"
    );
    for (i, (c, s)) in cases.iter().zip(&streams).enumerate() {
        let d = if do_dec { median(&mut dec[i]) } else { f64::NAN };
        let e = if do_enc { median(&mut enc[i]) } else { f64::NAN };
        let mb = c.raw_bytes as f64 / 1e6;
        // Steady state: the pooled codestream has served this geometry already (warm), so this is
        // the per-tile churn a long-running consumer pays.
        let mut planes = vec![vec![0i32; c.w * c.h]; c.tys.len()];
        TRACE.store(flag("--trace-alloc") && !do_enc, Relaxed);
        let (da, db) = churn(|| decode(&mut cs, s, c.w, c.h, &mut planes));
        TRACE.store(false, Relaxed);
        if flag("--cold") {
            let (ca, cb) = churn(|| decode(&mut Codestream::new(), s, c.w, c.h, &mut planes));
            let (ra, rb) = churn(|| {
                let mut cs2 = Codestream::new();
                decode(&mut cs2, s, c.w, c.h, &mut planes);
                cs2.restart();
                decode(&mut cs2, s, c.w, c.h, &mut planes)
            });
            println!("COLD {}: fresh decode {ca} allocs / {} KB ({:.2}x raw); fresh+warm pair {ra} / {} KB", c.name, cb / 1024, cb as f64 / c.raw_bytes as f64, rb / 1024);
        }
        TRACE.store(flag("--trace-alloc") && !do_dec, Relaxed);
        let (ea, eb) = churn(|| encode(&mut cs, c, &mut scratch));
        TRACE.store(false, Relaxed);
        println!(
            "{:<26} {:>9} {:>6.2} {:>8.3} {:>8.1} {:>8.3} {:>8.1} {:>6}/{:<6} {:>6}/{:<6}",
            c.name,
            s.len(),
            c.raw_bytes as f64 / s.len() as f64,
            d * 1e3,
            mb / d,
            e * 1e3,
            mb / e,
            da,
            db / 1024,
            ea,
            eb / 1024
        );
    }
}
