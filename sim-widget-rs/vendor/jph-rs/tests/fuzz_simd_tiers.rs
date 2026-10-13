//! Differential fuzz harness: every available SIMD tier (generic/sse2/avx/avx2,
//! plus avx512 when the hardware supports it) must encode the SAME input into a
//! byte-identical JPEG 2000 codestream. This is the "SIMD tiers must be
//! bit-exact against the scalar source of truth" invariant (repo `CLAUDE.md`)
//! exercised end-to-end (real subbands/codeblocks/HT entropy coding), not just
//! at the single-primitive level (cf. `coding::ojph_block_encoder_avx2::tests`).
//!
//! Drives `ojph_bench --compare-simd --assert-codestream` (each tier forced into
//! its own child process via `OJPH_CPU_MAX_LEVEL`/`OJPH_CPU_FORCE_LEVEL`, so the
//! wavelet/HT dispatch tables are initialized once per cap — see
//! `src/bin/ojph_bench.rs`) across several width/height/lossy combinations. The
//! synthetic pixel pattern is a deterministic function of (width, height)
//! (`synthetic_rgb_u8`), so varying the geometry varies the content driven
//! through the codeblocks — different code-block counts, different partial
//! edge blocks, different bit-plane counts — without needing a PRNG seed knob
//! in the bench tool itself.
//!
//! Run manually (slow, spawns ~4-5 child processes per case):
//!   cargo test -p jph-rs --test fuzz_simd_tiers -- --ignored --nocapture

use std::process::Command;

/// (width, height, lossy, quant_step) — geometries chosen to vary code-block
/// partial-edge shape (64x64 default block size) and, via `lossy`, exercise
/// both the reversible (5/3) and irreversible (9/7 + quantization) wavelet
/// paths through every tier.
const CASES: &[(u32, u32, bool, f32)] = &[
    (64, 64, false, 0.0),    // exactly one code-block, lossless
    (65, 63, false, 0.0),    // partial-edge blocks both axes, lossless
    (256, 128, false, 0.0),  // multi-block grid, lossless
    (300, 200, true, 0.05),  // odd geometry, lossy (irreversible 9/7)
    (1024, 64, true, 0.02),  // wide-and-short, lossy, different quant step
];

#[test]
#[ignore = "manual differential SIMD fuzz harness; run with --ignored --nocapture"]
fn simd_tiers_agree_across_geometries_and_quant_modes() {
    let exe = std::env::current_exe().expect("current_exe");
    // `current_exe` is this test binary; the sibling `ojph_bench` binary lives
    // in the same `target/{profile}/` directory.
    let bench_exe = exe
        .parent()
        .expect("test binary has a parent dir")
        .join(if cfg!(windows) { "ojph_bench.exe" } else { "ojph_bench" });
    assert!(
        bench_exe.is_file(),
        "ojph_bench not found at {bench_exe:?} — build it first: \
         cargo build -p jph-rs --release --bin ojph_bench --features cli"
    );

    let mut failures = Vec::new();
    for &(w, h, lossy, q) in CASES {
        let mut cmd = Command::new(&bench_exe);
        cmd.arg("--compare-simd")
            .arg("--assert-codestream")
            .arg("--width")
            .arg(w.to_string())
            .arg("--height")
            .arg(h.to_string());
        if lossy {
            cmd.arg("--lossy").arg("-q").arg(q.to_string());
        }
        let out = cmd.output().expect("spawn ojph_bench");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        println!("=== {w}x{h} lossy={lossy} q={q} ===\n{stdout}");
        if !stderr.trim().is_empty() {
            println!("(stderr)\n{stderr}");
        }
        let differs = stdout.contains("differs from generic") || stderr.contains("differs from generic");
        let no_tiers = stdout.contains("no tiers produced J2C output");
        if !out.status.success() || differs || no_tiers {
            failures.push(format!(
                "{w}x{h} lossy={lossy} q={q}: status={:?}",
                out.status.code()
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "SIMD tiers disagreed (or the harness itself failed) on: {failures:#?}"
    );
}
