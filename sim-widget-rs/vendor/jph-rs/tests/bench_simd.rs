//! Integration test: SIMD benchmark harness prints a speedup table.
//!
//! Run manually (slow, release recommended):
//!   cargo test -p jph-rs --test bench_simd -- --ignored --nocapture
//! Or directly:
//!   cargo run -p jph-rs --release --bin ojph_bench -- --compare-simd

use std::process::Command;

#[test]
#[ignore = "manual SIMD benchmark; run with --ignored --nocapture"]
fn bench_simd_compare_prints_speedup_table() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir.parent().unwrap().parent().unwrap();

    let status = Command::new("cargo")
        .current_dir(workspace)
        .args([
            "run",
            "-p",
            "jph-rs",
            "--release",
            "--bin",
            "ojph_bench",
            "--",
            "--compare-simd",
        ])
        .status()
        .expect("spawn ojph_bench");

    assert!(
        status.success(),
        "ojph_bench --compare-simd failed with status {status:?}"
    );
}
