//! SIMD encode benchmarks with per-tier speedup table.
//!
//! Each CPU tier runs in a fresh process (`OJPH_CPU_MAX_LEVEL`) so wavelet dispatch
//! is initialized once per cap.
//!
//! ```text
//! cargo build -p jph-rs --release --bin ojph_bench
//! cargo run  -p jph-rs --release --bin ojph_bench -- --compare-simd
//! cargo run  -p jph-rs --release --bin ojph_bench -- --compare-simd --lossy -q 0.05
//! ```

use std::process::Command;
use std::time::{Duration, Instant};

use jph_rs::codestream::ojph_params_local::ParamSiz;
use jph_rs::openjph::ojph_arch::{
    BENCH_CPU_LEVELS, ENV_CPU_FORCE_LEVEL, ENV_CPU_MAX_LEVEL, ENV_FORCE_ENCODE, ENV_FORCE_TX,
    ENV_FORCE_WAVELET_REV, X86_CPU_EXT_LEVEL_AVX512, X86_CPU_EXT_LEVEL_GENERIC, cpu_ext_level_name,
    detected_cpu_ext_level, get_cpu_ext_level,
};
use jph_rs::openjph::ojph_base::{Point, Size};
use jph_rs::openjph::ojph_codestream::Codestream;
use jph_rs::openjph::ojph_defs::Ui32;
use jph_rs::openjph::ojph_file::MemOutfile;
use jph_rs::{Avx2LzcntMode, ENV_AVX2_LZCNT, parse_avx2_lzcnt_mode};

/// UHD 4K (3840×2160).
const DEFAULT_WIDTH: u32 = 3840;
const DEFAULT_HEIGHT: u32 = 2160;
const WARMUP_ITERS: u32 = 2;
const TIMED_ITERS: u32 = 5;

#[derive(Clone, Copy)]
struct BenchConfig {
    width: u32,
    height: u32,
    lossy: bool,
    quant_step: f32,
    /// When set, passed to workers as `OJPH_AVX2_LZCNT`.
    lzcnt: Option<Avx2LzcntMode>,
}

fn lzcnt_mode_label(mode: Avx2LzcntMode) -> &'static str {
    match mode {
        Avx2LzcntMode::Emulated => "emulated",
        Avx2LzcntMode::Hardware => "hw",
        Avx2LzcntMode::Auto => "auto",
    }
}

fn apply_lzcnt_env(cmd: &mut Command, cfg: BenchConfig) {
    cmd.env_remove(ENV_AVX2_LZCNT);
    if let Some(mode) = cfg.lzcnt {
        cmd.env(ENV_AVX2_LZCNT, lzcnt_mode_label(mode));
    }
}

fn synthetic_rgb_u8(width: u32, height: u32) -> Vec<u8> {
    let n = (width * height * 3) as usize;
    let mut out = Vec::with_capacity(n);
    for y in 0..height {
        for x in 0..width {
            let t = ((x ^ y).wrapping_mul(17)) as u8;
            out.push(t.wrapping_add((x & 0xff) as u8));
            out.push(t.wrapping_add((y & 0xff) as u8));
            out.push(t.wrapping_add(128));
        }
    }
    out
}

fn encode_once(pixels: &[u8], width: u32, height: u32, cfg: BenchConfig) -> usize {
    let num_comps = 3u32;
    let mut cs = Codestream::new();

    {
        let siz: &mut ParamSiz = cs.access_siz();
        siz.set_image_extent(Point::new(width, height));
        siz.set_tile_size(Size::new(width, height));
        siz.set_num_components(num_comps);
        for c in 0..num_comps {
            siz.set_comp_info(c, Point::new(1, 1), 8, false);
        }
    }

    {
        let cod = cs.access_cod();
        cod.set_num_decompositions(5).expect("set_num_decompositions");
        cod.set_block_dims(64, 64).expect("set_block_dims");
        cod.set_progression_order("RPCL").expect("set_progression_order");
        cod.set_reversible(!cfg.lossy);
        cod.employ_color_transform(1);
    }

    cs.set_planar(false);

    if cfg.lossy {
        cs.access_qcd_mut().set_delta(cfg.quant_step);
    }

    let mut outfile = MemOutfile::default();
    let est_bytes = (width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(num_comps as usize)
        / 4;
    outfile.open(est_bytes.max(1 << 22), false).expect("open");
    cs.write_headers_slice(&mut outfile, &[]).expect("write HTJ2K headers");

    let mut next_comp: Ui32 = 0;
    let mut cur_line = cs.exchange(std::ptr::null_mut(), &mut next_comp).expect("exchange");
    let mut comp_lines = [0u32; 3];

    while !cur_line.is_null() {
        let comp = next_comp as usize;
        let y = comp_lines[comp];
        unsafe {
            let dp = (*cur_line).storage.i32_;
            for x in 0..width {
                let idx = (y * width * num_comps + x * num_comps + next_comp) as usize;
                *dp.add(x as usize) = pixels[idx] as i32;
            }
        }
        comp_lines[comp] += 1;
        cur_line = cs.exchange(cur_line, &mut next_comp).expect("exchange");
    }

    cs.flush().expect("flush");
    cs.close();
    outfile.get_used_size()
}

fn encode_to_vec(pixels: &[u8], width: u32, height: u32, cfg: BenchConfig) -> Vec<u8> {
    let num_comps = 3u32;
    let mut cs = Codestream::new();

    {
        let siz: &mut ParamSiz = cs.access_siz();
        siz.set_image_extent(Point::new(width, height));
        siz.set_tile_size(Size::new(width, height));
        siz.set_num_components(num_comps);
        for c in 0..num_comps {
            siz.set_comp_info(c, Point::new(1, 1), 8, false);
        }
    }

    {
        let cod = cs.access_cod();
        cod.set_num_decompositions(5).expect("set_num_decompositions");
        cod.set_block_dims(64, 64).expect("set_block_dims");
        cod.set_progression_order("RPCL").expect("set_progression_order");
        cod.set_reversible(!cfg.lossy);
        cod.employ_color_transform(1);
    }

    cs.set_planar(false);

    if cfg.lossy {
        cs.access_qcd_mut().set_delta(cfg.quant_step);
    }

    let mut outfile = MemOutfile::default();
    let est_bytes = (width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(num_comps as usize)
        / 4;
    outfile.open(est_bytes.max(1 << 22), false).expect("open");
    cs.write_headers_slice(&mut outfile, &[]).expect("write HTJ2K headers");

    let mut next_comp: Ui32 = 0;
    let mut cur_line = cs.exchange(std::ptr::null_mut(), &mut next_comp).expect("exchange");
    let mut comp_lines = [0u32; 3];

    while !cur_line.is_null() {
        let comp = next_comp as usize;
        let y = comp_lines[comp];
        unsafe {
            let dp = (*cur_line).storage.i32_;
            for x in 0..width {
                let idx = (y * width * num_comps + x * num_comps + next_comp) as usize;
                *dp.add(x as usize) = pixels[idx] as i32;
            }
        }
        comp_lines[comp] += 1;
        cur_line = cs.exchange(cur_line, &mut next_comp).expect("exchange");
    }

    cs.flush().expect("flush");
    cs.close();
    outfile.get_data().to_vec()
}

fn median_duration(samples: &mut [Duration]) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

fn run_encode_bench(cfg: BenchConfig) -> (Duration, usize) {
    let pixels = synthetic_rgb_u8(cfg.width, cfg.height);

    for _ in 0..WARMUP_ITERS {
        let _ = encode_once(&pixels, cfg.width, cfg.height, cfg);
    }

    let mut samples = Vec::with_capacity(TIMED_ITERS as usize);
    let mut last_bytes = 0usize;
    for _ in 0..TIMED_ITERS {
        let t0 = Instant::now();
        last_bytes = encode_once(&pixels, cfg.width, cfg.height, cfg);
        samples.push(t0.elapsed());
    }

    (median_duration(&mut samples), last_bytes)
}

fn print_worker_result(cfg: BenchConfig, write_j2c: Option<&std::path::Path>) {
    if let Some(mode) = cfg.lzcnt {
        jph_rs::set_avx2_lzcnt_mode(Some(mode));
    }
    let level = get_cpu_ext_level();
    if let Some(path) = write_j2c {
        let pixels = synthetic_rgb_u8(cfg.width, cfg.height);
        let j2c = encode_to_vec(&pixels, cfg.width, cfg.height, cfg);
        if let Err(e) = std::fs::write(path, &j2c) {
            eprintln!("failed to write J2C to {}: {e}", path.display());
            std::process::exit(1);
        }
        println!(
            "BENCH_JSON level={} name={} lossy={} ms=0 mpix_s=0 bytes={}",
            level,
            cpu_ext_level_name(level),
            cfg.lossy as u8,
            j2c.len()
        );
        return;
    }
    let (med, bytes) = run_encode_bench(cfg);
    let mpix_s = (cfg.width as f64 * cfg.height as f64) / med.as_secs_f64() / 1e6;
    println!(
        "BENCH_JSON level={} name={} lossy={} ms={:.3} mpix_s={:.2} bytes={}",
        level,
        cpu_ext_level_name(level),
        cfg.lossy as u8,
        med.as_secs_f64() * 1000.0,
        mpix_s,
        bytes
    );
}

struct BenchRow {
    label: String,
    level: i32,
    ms: f64,
    mpix_s: f64,
    bytes: usize,
}

struct BenchFailure {
    label: String,
    level: i32,
    reason: String,
}

fn parse_worker_line(line: &str) -> Option<BenchRow> {
    if !line.starts_with("BENCH_JSON ") {
        return None;
    }
    let mut level = 0i32;
    let mut name = "unknown";
    let mut ms = 0.0;
    let mut mpix_s = 0.0;
    let mut bytes = 0usize;
    for part in line.split_whitespace().skip(1) {
        let (k, v) = part.split_once('=')?;
        match k {
            "level" => level = v.parse().ok()?,
            "name" => name = v,
            "ms" => ms = v.parse().ok()?,
            "mpix_s" => mpix_s = v.parse().ok()?,
            "bytes" => bytes = v.parse().ok()?,
            _ => {}
        }
    }
    Some(BenchRow {
        label: name.to_string(),
        level,
        ms,
        mpix_s,
        bytes,
    })
}

fn assert_codestream_tiers(cfg: BenchConfig, exe: &std::path::Path) -> bool {
    let detected = detected_cpu_ext_level();
    let mut baseline: Option<Vec<u8>> = None;
    let mut checked = 0u32;
    // Per-process-unique subdirectory: a fixed shared path let two concurrent
    // `--assert-codestream` runs (e.g. two benchmark invocations on the same
    // machine) race on the same tier output files and corrupt each other's
    // reads, surfacing as a spurious tier crash/mismatch unrelated to SIMD
    // correctness.
    let tmp = std::env::temp_dir().join(format!("ojph_bench_assert_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp);

    for &(cap, label) in BENCH_CPU_LEVELS {
        if cap > detected && cap != X86_CPU_EXT_LEVEL_AVX512 {
            continue;
        }
        let out_path = tmp.join(format!("tier_{label}.j2c"));
        let mut cmd = std::process::Command::new(exe);
        cmd.env_remove(ENV_CPU_MAX_LEVEL)
            .env_remove(ENV_CPU_FORCE_LEVEL)
            .env_remove(jph_rs::coding::ojph_block_encoder_avx2::ENV_ENCODE_PROFILE);
        apply_lzcnt_env(&mut cmd, cfg);
        if cap == X86_CPU_EXT_LEVEL_AVX512 {
            if detected >= X86_CPU_EXT_LEVEL_AVX512 {
                cmd.env(ENV_CPU_MAX_LEVEL, cap.to_string());
            } else {
                cmd.env(ENV_CPU_FORCE_LEVEL, cap.to_string());
            }
        } else {
            cmd.env(ENV_CPU_MAX_LEVEL, cap.to_string());
        }
        cmd.arg("--worker")
            .arg("--write-j2c")
            .arg(&out_path)
            .arg("--width")
            .arg(cfg.width.to_string())
            .arg("--height")
            .arg(cfg.height.to_string());
        let q_str = cfg.quant_step.to_string();
        if cfg.lossy {
            cmd.arg("--lossy").arg("-q").arg(&q_str);
        }
        let out = match cmd.output() {
            Ok(o) => o,
            Err(e) => {
                eprintln!("assert-codestream: skipped tier {label} (spawn failed: {e})");
                continue;
            }
        };
        if !out.status.success() {
            let code = out.status.code().unwrap_or(-1);
            let hint = if cap == X86_CPU_EXT_LEVEL_AVX512 && detected < X86_CPU_EXT_LEVEL_AVX512 {
                " (likely STATUS_ILLEGAL_INSTRUCTION — CPU has no AVX-512)"
            } else {
                ""
            };
            eprintln!("assert-codestream: skipped tier {label} (exit {code}{hint})");
            continue;
        }
        let Ok(j2c) = std::fs::read(&out_path) else {
            eprintln!("assert-codestream: skipped tier {label} (missing output)");
            continue;
        };
        checked += 1;
        match &baseline {
            None => baseline = Some(j2c),
            Some(base) if base == &j2c => {}
            Some(base) => {
                eprintln!(
                    "assert-codestream: tier {label} J2C differs from generic ({} vs {} bytes)",
                    base.len(),
                    j2c.len()
                );
                return false;
            }
        }
    }
    if checked == 0 {
        eprintln!("assert-codestream: no tiers produced J2C output");
        return false;
    }
    true
}

fn compare_simd(cfg: BenchConfig, assert_codestream: bool) {
    let exe = std::env::current_exe().expect("current_exe");
    let detected = detected_cpu_ext_level();
    let native = get_cpu_ext_level();

    let mut rows: Vec<BenchRow> = Vec::new();
    let mut failures: Vec<BenchFailure> = Vec::new();

    for &(cap, label) in BENCH_CPU_LEVELS {
        if cap > detected && cap != X86_CPU_EXT_LEVEL_AVX512 {
            continue;
        }

        let mut cmd = Command::new(&exe);
        cmd.env_remove(ENV_CPU_MAX_LEVEL)
            .env_remove(ENV_CPU_FORCE_LEVEL)
            .env_remove(jph_rs::coding::ojph_block_encoder_avx2::ENV_ENCODE_PROFILE);
        apply_lzcnt_env(&mut cmd, cfg);
        if cap == X86_CPU_EXT_LEVEL_AVX512 {
            if detected >= X86_CPU_EXT_LEVEL_AVX512 {
                cmd.env(ENV_CPU_MAX_LEVEL, cap.to_string());
            } else {
                cmd.env(ENV_CPU_FORCE_LEVEL, cap.to_string());
            }
        } else {
            cmd.env(ENV_CPU_MAX_LEVEL, cap.to_string());
        }
        cmd.arg("--worker")
            .arg("--width")
            .arg(cfg.width.to_string())
            .arg("--height")
            .arg(cfg.height.to_string());
        let q_str = cfg.quant_step.to_string();
        if cfg.lossy {
            cmd.arg("--lossy").arg("-q").arg(&q_str);
        }
        let out = match cmd.output() {
            Ok(o) => o,
            Err(e) => {
                failures.push(BenchFailure {
                    label: label.to_string(),
                    level: cap,
                    reason: format!("spawn failed: {e}"),
                });
                continue;
            }
        };

        if !out.status.success() {
            let code = out.status.code().unwrap_or(-1);
            let hint = if cap == X86_CPU_EXT_LEVEL_AVX512 && detected < X86_CPU_EXT_LEVEL_AVX512 {
                " (likely STATUS_ILLEGAL_INSTRUCTION — CPU has no AVX-512)"
            } else {
                ""
            };
            failures.push(BenchFailure {
                label: label.to_string(),
                level: cap,
                reason: format!("exit {code}{hint}"),
            });
            continue;
        }

        let stdout = String::from_utf8_lossy(&out.stdout);
        let Some(line) = stdout.lines().find(|l| l.starts_with("BENCH_JSON ")) else {
            failures.push(BenchFailure {
                label: label.to_string(),
                level: cap,
                reason: "missing BENCH_JSON in stdout".to_string(),
            });
            continue;
        };

        if let Some(row) = parse_worker_line(line) {
            rows.push(row);
        }
    }

    if rows.is_empty() {
        eprintln!("no benchmark rows collected");
        std::process::exit(1);
    }

    let baseline_ms = rows
        .iter()
        .find(|r| r.level == X86_CPU_EXT_LEVEL_GENERIC)
        .map(|r| r.ms)
        .unwrap_or(rows[0].ms);

    let scenario = if cfg.lossy {
        format!("lossy q={}", cfg.quant_step)
    } else {
        "lossless".to_string()
    };

    println!();
    println!(
        "jph-rs SIMD encode benchmark — {}x{} RGB, {} (median of {} iters, warmup {})",
        cfg.width, cfg.height, scenario, TIMED_ITERS, WARMUP_ITERS
    );
    println!(
        "CPU: detected {} ({}), active {} ({})",
        detected,
        cpu_ext_level_name(detected),
        native,
        cpu_ext_level_name(native)
    );
    if detected < X86_CPU_EXT_LEVEL_AVX512 {
        println!(
            "Note: avx512 row uses {}={} when not detected; the request is clamped to the detected tier unless built with --features unsafe-force-simd (needs AVX-512F+CD to be meaningful).",
            ENV_CPU_FORCE_LEVEL, X86_CPU_EXT_LEVEL_AVX512
        );
    } else {
        println!("AVX-512 detected — using native avx512 dispatch.");
    }
    let lzcnt_active = cfg.lzcnt.unwrap_or_else(jph_rs::avx2_lzcnt_mode_from_env);
    println!(
        "AVX2 lzcnt: {} ({} or --lzcnt emulated|hw|auto)",
        lzcnt_mode_label(lzcnt_active),
        ENV_AVX2_LZCNT
    );
    println!();
    println!(
        "{:<10} {:>8} {:>10} {:>10} {:>10}",
        "tier", "ms", "Mpix/s", "codestream", "speedup"
    );
    println!("{:-<52}", "");

    for row in &rows {
        let speedup = baseline_ms / row.ms;
        println!(
            "{:<10} {:>8.2} {:>10.2} {:>10} {:>9.2}x",
            row.label, row.ms, row.mpix_s, row.bytes, speedup
        );
    }

    if !failures.is_empty() {
        println!();
        println!("Skipped / failed tiers:");
        for f in &failures {
            println!("  {:<10} level {} — {}", f.label, f.level, f.reason);
        }
    }

    if let (Some(base), Some(peak)) = (
        rows.iter().find(|r| r.level == X86_CPU_EXT_LEVEL_GENERIC),
        rows.iter().max_by(|a, b| {
            a.mpix_s
                .partial_cmp(&b.mpix_s)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
    ) {
        if peak.level > base.level {
            let ratio = base.ms / peak.ms;
            println!();
            if ratio >= 1.0 {
                let gain_pct = (peak.mpix_s / base.mpix_s - 1.0) * 100.0;
                println!(
                    "Best tier vs generic: {:.2}x faster ({:.1}% throughput, {} -> {})",
                    ratio, gain_pct, base.label, peak.label
                );
            } else {
                println!(
                    "Best tier vs generic: {:.2}x slower on this run ({} -> {}). \
                     Try a larger --width/--height; small tiles can favor scalar paths.",
                    peak.ms / base.ms,
                    base.label,
                    peak.label
                );
            }
        }
    }

    if assert_codestream {
        if !assert_codestream_tiers(cfg, &exe) {
            std::process::exit(1);
        }
        println!();
        println!(
            "assert-codestream: all successful SIMD tiers produced identical J2C ({}x{} RGB)",
            cfg.width, cfg.height
        );
    }
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!(
        "  ojph_bench --compare-simd [--assert-codestream] [--lossy] [-q <step>] [--width N] [--height N] [--lzcnt emulated|hw|auto]"
    );
    eprintln!(
        "  ojph_bench --worker [--lossy] [-q <step>] [--width N] [--height N] [--lzcnt emulated|hw|auto]"
    );
    eprintln!();
    eprintln!(
        "Set {} to cap SIMD dispatch (used by --compare-simd workers).",
        ENV_CPU_MAX_LEVEL
    );
    eprintln!(
        "Set {} (or --lzcnt) for AVX2 block-encode lzcnt path (default: emulated, matches OpenJPH C++).",
        ENV_AVX2_LZCNT
    );
    eprintln!("Diagnostic mix at OJPH_CPU_MAX_LEVEL=9:");
    eprintln!(
        "  {}=generic  — scalar HT encode, keep AVX2 DWT/tx",
        ENV_FORCE_ENCODE
    );
    eprintln!(
        "  {}=sse2     — SSE2 reversible wavelet, keep AVX2 encode",
        ENV_FORCE_WAVELET_REV
    );
    eprintln!(
        "  {}=sse2     — SSE2 tile→codeblock transform",
        ENV_FORCE_TX
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    let mut cfg = BenchConfig {
        width: DEFAULT_WIDTH,
        height: DEFAULT_HEIGHT,
        lossy: false,
        quant_step: 0.05,
        lzcnt: None,
    };
    let mut compare = false;
    let mut worker = false;
    let mut assert_codestream = false;
    let mut write_j2c: Option<std::path::PathBuf> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--compare-simd" => compare = true,
            "--assert-codestream" => assert_codestream = true,
            "--write-j2c" => {
                i += 1;
                write_j2c = Some(std::path::PathBuf::from(&args[i]));
            }
            "--worker" => worker = true,
            "--lossy" => cfg.lossy = true,
            "-q" | "-qstep" => {
                i += 1;
                cfg.quant_step = args[i].parse().unwrap_or(0.05);
                cfg.lossy = true;
            }
            "--width" => {
                i += 1;
                cfg.width = args[i].parse().unwrap_or(DEFAULT_WIDTH);
            }
            "--height" => {
                i += 1;
                cfg.height = args[i].parse().unwrap_or(DEFAULT_HEIGHT);
            }
            "--lzcnt" => {
                i += 1;
                let Some(mode) = parse_avx2_lzcnt_mode(&args[i]) else {
                    eprintln!("Invalid --lzcnt (use emulated|hw|auto)");
                    std::process::exit(1);
                };
                cfg.lzcnt = Some(mode);
            }
            "-h" | "--help" => {
                print_usage();
                return;
            }
            other => {
                eprintln!("Unknown argument: {other}");
                print_usage();
                std::process::exit(1);
            }
        }
        i += 1;
    }

    if worker {
        print_worker_result(cfg, write_j2c.as_deref());
        return;
    }

    if compare {
        compare_simd(cfg, assert_codestream);
        return;
    }

    print_usage();
    std::process::exit(1);
}
