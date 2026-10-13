//! Helpers shared by the test suites of every crate in the stack. Not part of the library API.

/// Environment variable that turns "no GPU here" from a skip into a test failure. Set it wherever
/// a GPU is expected (a GPU CI runner, a workstation), so GPU tests cannot pass by not running.
pub const REQUIRE_GPU_VAR: &str = "VFX_REQUIRE_GPU";

/// Reports that a GPU test cannot run here and returns, so the caller can `return` early.
///
/// With `VFX_REQUIRE_GPU=1` it panics instead. Otherwise it prints one `SKIPPED` line to stderr,
/// visible with `--nocapture` and in CI logs, so an empty pass is at least announced. Every GPU
/// test in the workspace goes through this; a bare `return` on a missing device is the pattern it
/// replaces.
#[track_caller]
pub fn gpu_unavailable(test: &str, why: impl std::fmt::Debug) {
    let required = std::env::var(REQUIRE_GPU_VAR).is_ok_and(|v| v == "1");
    assert!(
        !required,
        "{REQUIRE_GPU_VAR}=1 but {test} has no GPU: {why:?}"
    );
    eprintln!("SKIPPED {test}: no GPU ({why:?}); set {REQUIRE_GPU_VAR}=1 to make this a failure");
}

/// Environment variable that turns "no CUDA device" into a failure (separate from
/// [`REQUIRE_GPU_VAR`]: a machine can have a wgpu adapter and no CUDA).
pub const REQUIRE_CUDA_VAR: &str = "VFX_REQUIRE_CUDA";

/// [`gpu_unavailable`] for CUDA tests, keyed on [`REQUIRE_CUDA_VAR`].
#[track_caller]
pub fn cuda_unavailable(test: &str, why: impl std::fmt::Debug) {
    let required = std::env::var(REQUIRE_CUDA_VAR).is_ok_and(|v| v == "1");
    assert!(
        !required,
        "{REQUIRE_CUDA_VAR}=1 but {test} has no CUDA device: {why:?}"
    );
    eprintln!("SKIPPED {test}: no CUDA ({why:?}); set {REQUIRE_CUDA_VAR}=1 to make this a failure");
}
